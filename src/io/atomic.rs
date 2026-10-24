//! 解压落盘的「原子写入」守卫
//!
//! # 为什么需要它（阶段 0 体检 T9-P1 / T8-P6 的修复）
//!
//! 以前每个条目都是直接 `File::create(正式路径)` 边读边写。一旦写到一半被中断——
//! 磁盘满、拔盘、用户取消、进程被杀、崩溃——磁盘上留下的是**顶着正式文件名**的半截文件，
//! 外观和正常文件一模一样；用户下次重试时，因为冲突策略是「同名改名」，半截文件被绕开，
//! 目录里反而长出 `part1.bin` + `part1 (1).bin` 两份，坏的那份永远修不好。
//! 实测：1 GB 的包被杀一次再重试，目录变成 12 个文件、占 1.15 GB，且 `part1.bin` 永远是坏的。
//!
//! # 做法
//!
//! 先写 `<名字>.yshjy-part`，字节数对得上、属性还原完成后再 `rename` 成正式名。
//! 同一个目录内的 `rename` 是原子的，因此：
//!
//! - **进程被杀 / 断电**：磁盘上只留 `.yshjy-part`，一眼能看出「没写完」
//! - **正常失败**（读错、校验不过、限额中止）：`Drop` 自动删掉 part，目录保持干净
//! - **重试**：part 直接覆盖重写，不再产生 `(1)` 副本，坏文件会被替换掉
//!
//! # 取舍（诚实记录）
//!
//! 这里**没有**在改名之前 `sync_all()`。真断电（不是进程被杀）时，文件内容可能还在页缓存里，
//! 极端情况下会得到一个 0 字节的正式文件——但那种场景需要 fsync 才能避免，而 fsync 在
//! 机械盘上解 10 万个碎文件会从 8.6 秒涨到几十秒级。当前优先解决的是「进程被杀 / 磁盘满 /
//! 取消」这三类真人最常撞的情况；断电属于 T8 里标注为「需要额外权衡」的已知缺口。

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// part 文件后缀。短、可辨识，且不会被当成正常产物（`*.yshjy-part`）。
pub const PART_SUFFIX: &str = ".yshjy-part";

/// 文件名上限（ext4/xfs/btrfs 都是 255 字节）
const NAME_MAX: usize = 255;

/// 原子写入守卫。
///
/// 用法：`create` → 往 [`PartFile::as_file`] 写 → 成功后 [`PartFile::finish`]。
/// 忘了 `finish` 就走掉（包括 `?` 提前返回）时，`Drop` 会把 part 删掉，不会留垃圾。
#[derive(Debug)]
pub struct PartFile {
    part: PathBuf,
    target: PathBuf,
    file: Option<File>,
    finished: bool,
}

impl PartFile {
    /// 在 `target` 旁边创建 part 文件（父目录必须已存在）。
    pub fn create(target: impl AsRef<Path>) -> io::Result<Self> {
        let target = target.as_ref().to_path_buf();
        let part = part_path_for(&target);
        let file = File::create(&part)?;
        Ok(Self {
            part,
            target,
            file: Some(file),
            finished: false,
        })
    }

    /// part 文件的路径（排错 / 测试用）
    pub fn part_path(&self) -> &Path {
        &self.part
    }

    /// 最终路径
    pub fn target_path(&self) -> &Path {
        &self.target
    }

    /// 拿到可写的文件句柄
    pub fn as_file(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("PartFile 已经在 finish/abort 之后，不能再写")
    }

    /// 写完并原子改名成正式文件。失败时 part 由 `Drop` 清掉。
    pub fn finish(mut self) -> io::Result<()> {
        // 先把句柄关掉再改名：Windows 上改名要求没有打开的句柄，Linux 也习惯这么做
        self.file.take();
        fs::rename(&self.part, &self.target)?;
        self.finished = true;
        Ok(())
    }

    /// 主动放弃（等价于 `drop`，但语义更清楚）
    pub fn abort(mut self) {
        self.file.take();
        // 由 Drop 收尾
    }

    /// 不落到正式名，而是改名到**另一个名字**上（救援模式用它保留 `.partial` 残片）。
    ///
    /// 与 [`PartFile::finish`] 的唯一区别是目标路径可控；同样是原子的同目录改名。
    /// 用 `&mut self` 是为了让调用方在分支里改名后仍能继续持有这个守卫（失败时由 Drop 清理）。
    pub fn rename_to(&mut self, target: impl AsRef<Path>) -> io::Result<()> {
        self.file.take();
        fs::rename(&self.part, target)?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.file.take();
        let _ = fs::remove_file(&self.part);
    }
}

/// 由目标路径推出 part 路径；文件名过长时按字符边界截断再拼后缀。
fn part_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    if name.len() + PART_SUFFIX.len() <= NAME_MAX {
        return target.with_file_name(format!("{name}{PART_SUFFIX}"));
    }

    // 中文名是多字节，截断必须落在字符边界上，否则会 panic
    let mut cut = NAME_MAX - PART_SUFFIX.len();
    while cut > 0 && !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut base = name;
    base.truncate(cut);
    target.with_file_name(format!("{base}{PART_SUFFIX}"))
}

/// 把 [`PartFile`] 当成 `Write` 用。
///
/// 给「你给它一个 writer，它自己往里写」的库用（`unrar` 的解压回调、`lzma-rs` 的 xz 解码器）。
/// 用法：`create` → 把 `&mut PartWriter` 交给库 → 确认成功后再 [`PartWriter::finish`]。
/// 忘了 `finish` 就走掉时，part 由 [`PartFile`] 的 `Drop` 删掉。
#[derive(Debug)]
pub struct PartWriter {
    part: Option<PartFile>,
}

impl PartWriter {
    /// 同 [`PartFile::create`]
    pub fn create(target: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            part: Some(PartFile::create(target)?),
        })
    }

    /// 写完并原子改名成正式文件
    pub fn finish(&mut self) -> io::Result<()> {
        match self.part.take() {
            Some(p) => p.finish(),
            None => Ok(()),
        }
    }

    /// 是否还没 finish
    pub fn is_open(&self) -> bool {
        self.part.is_some()
    }
}

impl io::Write for PartWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.part.as_mut() {
            Some(p) => p.as_file().write(buf),
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "PartWriter 已经 finish 过，不能再写",
            )),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.part.as_mut() {
            Some(p) => p.as_file().flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn 临时目录(标签: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yshjy_part_{标签}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 写完才出现正式文件() {
        let dir = 临时目录("正常");
        let target = dir.join("文件.txt");
        let mut part = PartFile::create(&target).unwrap();
        assert!(!target.exists(), "还没 finish 就不该出现正式文件");
        assert!(part.part_path().exists(), "应该先有 part 文件");
        part.as_file().write_all(b"hello").unwrap();
        part.finish().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
        assert!(!fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(PART_SUFFIX)));
    }

    #[test]
    fn 中途放弃不留痕迹() {
        let dir = 临时目录("放弃");
        let target = dir.join("半截.bin");
        {
            let mut part = PartFile::create(&target).unwrap();
            part.as_file().write_all(b"only half").unwrap();
        }
        assert!(!target.exists(), "不该出现正式文件");
        assert!(
            fs::read_dir(&dir).unwrap().flatten().count() == 0,
            "part 应该被 Drop 清掉"
        );
    }

    #[test]
    fn 超长文件名能建出part() {
        let dir = 临时目录("超长");
        let long = format!("{}.bin", "啊".repeat(200)); // 600 字节，远超 255
        let target = dir.join(&long);
        // 正式名本身就可能建不出来（这类超长条目本来就由路径校验拦下）；
        // 这里只要求 part 名推导不 panic、不越界
        let part = part_path_for(&target);
        let part_name = part.file_name().unwrap().to_string_lossy().into_owned();
        assert!(part_name.len() <= NAME_MAX, "part 名必须在 NAME_MAX 之内");
        assert!(part_name.ends_with(PART_SUFFIX));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 目标已存在时被原子替换() {
        let dir = 临时目录("替换");
        let target = dir.join("同名.txt");
        fs::write(&target, b"old").unwrap();
        let mut part = PartFile::create(&target).unwrap();
        part.as_file().write_all(b"new").unwrap();
        part.finish().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn writer包装也能原子落地() {
        let dir = 临时目录("writer");
        let target = dir.join("w.txt");
        {
            let mut w = PartWriter::create(&target).unwrap();
            w.write_all(b"abc").unwrap();
            assert!(!target.exists(), "finish 之前不该出现正式文件");
            w.finish().unwrap();
        }
        assert_eq!(fs::read(&target).unwrap(), b"abc");
    }

    #[test]
    fn writer没finish就不落地() {
        let dir = 临时目录("writer放弃");
        let target = dir.join("w2.txt");
        {
            let mut w = PartWriter::create(&target).unwrap();
            w.write_all(b"half").unwrap();
        }
        assert!(!target.exists());
        assert!(
            fs::read_dir(&dir).unwrap().flatten().count() == 0,
            "part 应该被清掉"
        );
    }

    #[test]
    fn 父目录不存在时返回错误而不是panic() {
        let dir = 临时目录("缺目录");
        let target = dir.join("不存在子目录/文件.txt");
        assert!(PartFile::create(&target).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
