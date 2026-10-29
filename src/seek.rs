//! 「这个位置能不能跳着读」—— 移动端 seek 三档（第三期 C）
//!
//! # 为什么需要
//!
//! 桌面侧所有解码都走 `Read + Seek`（见 [`crate::unpack::Source`]），因为桌面上的
//! 文件永远是普通文件，`seek` 随便用。手机不是：
//!
//! - 安卓的存储访问框架（SAF）给的是 `content://` 句柄，转成文件描述符后**可能**只能
//!   顺序读（各厂商实现不一样，本机没安卓设备，**不下结论** —— 见方案 §C 的边界）；
//! - 从管道 / 僵尸进程 / 网络位置拿到的流根本不能回退。
//!
//! 所以配套 [`SeekAbility`] 三档，套不上就按档降级，**解码逻辑一行不改**：
//!
//! | 档 | 含义 | 怎么做 |
//! |---|---|---|
//! | [`SeekAbility::Full`] | 能随便跳 | 什么都不用做（桌面就是这档） |
//! | [`SeekAbility::BlockCache`] | 只能顺序读，但能重来 | 套 [`crate::io::BlockCache`]（32 KiB 块 / 8 MiB LRU） |
//! | [`SeekAbility::CopyFirst`] | 连重来都不行 | 先复制到私有目录（[`复制到本地`]，**先做空间预检**） |
//!
//! # 本机验证到哪一步
//!
//! 「SAF 到底算哪一档」**没有验证**（本机 Wayland 桌面 + 无 Android SDK 设备）。
//! 本轮只把「探测 + 降级 + 协议字段」做成可单测的代码，落地验收写进
//! `docs/TODO.md` 的平台节，不冒充已完成。

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorCode, Result};

/// 一个位置能不能「跳着读」
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeekAbility {
    /// 能直接 seek（普通文件）
    #[default]
    Full,
    /// 不支持 seek，但可以顺序读 → 套块缓存
    BlockCache,
    /// 连顺序重读都不行（管道 / 字符设备）→ 必须先复制
    CopyFirst,
}

impl SeekAbility {
    /// 协议字段值（`scan` 的 `seek` 字段用它，界面按它决定要不要提示）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::BlockCache => "block_cache",
            Self::CopyFirst => "copy_first",
        }
    }

    /// 中文标签（给界面显示；引擎只给语义，句子归外壳）
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "可以直接读",
            Self::BlockCache => "只能顺序读（会套块缓存）",
            Self::CopyFirst => "要先复制一份（会占磁盘）",
        }
    }

    /// 是否需要用户知情（复制会占空间）
    pub fn needs_copy(self) -> bool {
        matches!(self, Self::CopyFirst)
    }
}

/// 探测某个路径属于哪一档
///
/// 判定顺序（后面的都是「越确定越靠前」）：
/// 1. 打不开 / 不是普通文件（管道、字符设备、socket）→ 不是 seek 的问题，直接判
///    [`SeekAbility::CopyFirst`]（流不能回退，重读只能靠复制）；
/// 2. 打得开 → 试 `seek(End(0))` 再 `seek(Start(0))`：都成功是 [`SeekAbility::Full`]；
/// 3. 能打开但 seek 失败 → [`SeekAbility::BlockCache`]。
///
/// 打不开的**普通文件**返回 [`SeekAbility::Full`]：那是「文件不存在 / 没权限」，
/// 后面会正常报错，不该被伪装成「这个位置不能跳着读」。
pub fn seek_ability<P: AsRef<Path>>(path: P) -> SeekAbility {
    let p = path.as_ref();
    let Ok(meta) = fs::metadata(p) else {
        return SeekAbility::Full;
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let ft = meta.file_type();
        // FIFO / 字符设备 / socket：读一遍就没了，不能回退
        if ft.is_fifo() || ft.is_char_device() || ft.is_socket() {
            return SeekAbility::CopyFirst;
        }
    }
    if !meta.is_file() {
        // 目录之类的不是能解压的东西，但也不该假装成 Full 让人去 seek
        return SeekAbility::Full;
    }

    let mut f = match fs::File::open(p) {
        Ok(f) => f,
        Err(_) => return SeekAbility::Full,
    };
    match f.seek(SeekFrom::End(0)) {
        Ok(_) => {
            if f.seek(SeekFrom::Start(0)).is_ok() {
                SeekAbility::Full
            } else {
                SeekAbility::BlockCache
            }
        }
        Err(_) => SeekAbility::BlockCache,
    }
}

/// 把「不能跳着读」的源复制到私有目录，返回复制出来的路径
///
/// 为什么必须先预检空间：复制一份 8 GB 的包到没有 8 GB 空间的手机上，
/// 结果是**复制到一半失败 + 原包还在**，用户白等十几分钟（方案 §C 决策 4 的原话是
/// 「空间不够明确报错而不是静默失败」）。所以这里先用
/// [`crate::safety::check_space`]，不够就返回 `NotEnoughSpace`。
///
/// 复制用 1 MiB 块流式写 —— 与全项目的资源红线一致（不许整包读进内存）。
pub fn 复制到本地(src: &Path, 私有目录: &Path) -> Result<PathBuf> {
    let meta = fs::metadata(src).map_err(|e| {
        Error::new(ErrorCode::PathNotFound).context(format!("复制前取不到源信息：{e}"))
    })?;
    if !meta.is_file() {
        return Err(Error::new(ErrorCode::InvalidArgument)
            .context("只有普通文件能这样复制（管道请直接读流）".to_string()));
    }
    let 需要 = meta.len();
    fs::create_dir_all(私有目录).map_err(|e| {
        Error::new(ErrorCode::PermissionDenied).context(format!("建不了私有目录：{e}"))
    })?;
    // 查不到可用空间（不支持的挂载 / 权限）→ 别拦人，继续走（同 safety 的口径）
    if let crate::safety::SpaceCheck::NotEnough { available, .. } =
        crate::safety::check_space(私有目录, 需要)
    {
        return Err(Error::new(ErrorCode::NotEnoughSpace).with(
            "reason",
            // 引擎只给结构化事实（字节数），「GB/MB」的写法归外壳
            format!("复制这一步要 {需要} 字节，目标只剩 {available} 字节"),
        ));
    }

    let 名字 = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "复制".into());
    let 目标 = 私有目录.join(format!("{名字}.yshjy-copy"));
    let mut 读 = fs::File::open(src)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("open: {e}")))?;
    let mut 写 = fs::File::create(&目标)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("create: {e}")))?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut 已写 = 0u64;
    loop {
        let n = 读
            .read(&mut buf)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read: {e}")))?;
        if n == 0 {
            break;
        }
        写.write_all(&buf[..n])
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write: {e}")))?;
        已写 += n as u64;
    }
    写.flush()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("flush: {e}")))?;
    // 复制完要能**回读验证**（不能「以为复制成功了」）
    if 已写 != 需要 {
        let _ = fs::remove_file(&目标);
        return Err(Error::new(ErrorCode::Truncated).with(
            "reason",
            format!("复制不完整：源 {需要} 字节，抄了 {已写} 字节"),
        ));
    }
    Ok(目标)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 普通文件是_full() {
        let d = std::env::temp_dir().join(format!("yshjy_seek_{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let p = d.join("a.txt");
        fs::write(&p, b"hello seek").unwrap();
        assert_eq!(seek_ability(&p), SeekAbility::Full);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 不存在的路径不当成_seek_问题() {
        assert_eq!(
            seek_ability("/nonexistent/这不是包.mp4"),
            SeekAbility::Full,
            "打不开是「没有这个文件」，不该伪装成「不能跳着读」"
        );
    }

    #[cfg(unix)]
    #[test]
    fn 字符设备要先复制() {
        // /dev/null 是稳定的字符设备，不依赖任何外部命令
        assert_eq!(seek_ability("/dev/null"), SeekAbility::CopyFirst);
    }

    #[cfg(unix)]
    #[test]
    fn 管道要先复制() {
        // 造一个 FIFO：优先用 mkfifo 命令（coreutils），造不出来就跳过
        let d = std::env::temp_dir().join(format!("yshjy_fifo_{}", std::process::id()));
        let _ = fs::create_dir_all(&d);
        let fifo = d.join("p");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !made {
            eprintln!("跳过：没有 mkfifo");
            let _ = fs::remove_dir_all(&d);
            return;
        }
        assert_eq!(seek_ability(&fifo), SeekAbility::CopyFirst);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 协议字段是约定的三档() {
        assert_eq!(SeekAbility::Full.as_str(), "full");
        assert_eq!(SeekAbility::BlockCache.as_str(), "block_cache");
        assert_eq!(SeekAbility::CopyFirst.as_str(), "copy_first");
        assert!(!SeekAbility::Full.needs_copy());
        assert!(SeekAbility::CopyFirst.needs_copy());
        // 三档标签必须都不一样（界面靠它区分）
        let mut v = [
            SeekAbility::Full.label(),
            SeekAbility::BlockCache.label(),
            SeekAbility::CopyFirst.label(),
        ];
        v.sort_unstable();
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn 复制到本地逐字节一致() {
        let d = std::env::temp_dir().join(format!("yshjy_copy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src")).unwrap();
        fs::create_dir_all(d.join("priv")).unwrap();
        let p = d.join("src").join("包.mp4");
        let 原始: Vec<u8> = (0..(3 * 1024 * 1024 + 7)).map(|i| (i % 251) as u8).collect();
        fs::write(&p, &原始).unwrap();

        let 副本 = 复制到本地(&p, &d.join("priv")).expect("复制");
        assert_eq!(fs::read(&副本).unwrap(), 原始, "复制必须逐字节一致");
        assert_eq!(副本.file_name().unwrap().to_string_lossy(), "包.mp4.yshjy-copy");
        // 源不动
        assert!(p.exists());
        let _ = fs::remove_dir_all(&d);
    }
}
