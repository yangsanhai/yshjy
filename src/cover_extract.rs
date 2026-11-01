//! 封面分离：把伪装包里的「原版视频/图片」也交出来（DESIGN §3 / §17.5）
//!
//! 伪装包的结构是 `[封面区][压缩层]`：定位阶段已经算出了压缩层的真实起点
//! （真实样本 8.7 GB 包：`base = 119,248,031`），所以封面区就是 `[0, base)`。
//! 把这几十~几百 MB 原样切出来，就是一个能正常播放的视频（或图片）。
//!
//! 三条硬约束：
//! 1. **绝不覆盖原包** —— 输出名带 `.封面` 后缀，万一重名就加序号
//! 2. **只切不猜** —— 不做转码/修复；ffmpeg 的 remux 是可选增强，没有就跳过
//! 3. **失败不影响解压** —— 封面是附赠品，切不出来不该让主流程报错

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorCode, Result};

/// 切出来的封面
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    /// 落盘路径（`原文件名.封面.mp4`）
    pub path: PathBuf,
    /// 字节数
    pub size: u64,
    /// `video` / `image` / `other`（外壳据此显示不同文案）
    pub kind: &'static str,
}

/// 按扩展名粗分封面类型（只用于文案，不影响切法）
pub fn cover_kind(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" => "video",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avif" => "image",
        _ => "other",
    }
}

/// 同上，但落点目录可以指定（`-o <目录>` / 协议 `dest`）。
///
/// 为什么需要它：`-o` 是「产物放这儿」的通用承诺，封面也是产物。不认 `-o` 的后果实测过 ——
/// 副本全撒在源包旁边：目录可写时越跑越多（`封面 (35).mp4`），只读目录 / 网盘挂载点直接失败。
/// `dir = None` 保持历史行为（包旁边），别改回去。
pub fn cover_path_in(input: &Path, dir: Option<&Path>) -> PathBuf {
    let parent = dir
        .or_else(|| input.parent())
        .unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "封面".to_string());
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let build = |n: Option<usize>| -> PathBuf {
        let name = match (n, ext.is_empty()) {
            (None, true) => format!("{stem}.封面"),
            (None, false) => format!("{stem}.封面.{ext}"),
            (Some(i), true) => format!("{stem}.封面 ({i})"),
            (Some(i), false) => format!("{stem}.封面 ({i}).{ext}"),
        };
        parent.join(name)
    };

    let first = build(None);
    if first != input && !first.exists() {
        return first;
    }
    for i in 1..10_000 {
        let cand = build(Some(i));
        if cand != input && !cand.exists() {
            return cand;
        }
    }
    first
}

/// 封面区的「体检结果」：只算大小和类型，**不落盘**
///
/// 界面要在预览阶段就把「伪装封面」当成清单里一行（带大小），但预览绝不能
/// 顺手切出一个几百 MB 的文件 —— 所以先只回答「有多大、是什么」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverInfo {
    /// 封面区字节数
    pub size: u64,
    /// `video` / `image` / `other`
    pub kind: &'static str,
    /// 扩展名（小写，不含点）
    pub ext: String,
}

/// 探一下封面区（`base == 0` 或越界 → 没有封面）
pub fn probe_zip_cover(input: &Path, base: u64) -> Option<CoverInfo> {
    if base == 0 {
        return None;
    }
    let total = std::fs::metadata(input).ok()?.len();
    if base > total {
        return None;
    }
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    Some(CoverInfo {
        size: base,
        kind: cover_kind(&ext),
        ext,
    })
}

/// 切出封面区 `[0, base)`（落在包旁边）
///
/// - `base == 0` → 没有封面区（包就在文件开头），返回 `Ok(None)`
/// - 输出已存在 → 自动改名，不覆盖
/// - 只复制 `base` 字节，绝不多读
pub fn extract_zip_cover(input: &Path, base: u64) -> Result<Option<Cover>> {
    extract_zip_cover_to(input, base, None)
}

/// 切出封面区 `[0, base)`，落到 `dest`（`None` = 包旁边）
///
/// 比上一版多两件事，都是为了不在用户磁盘上留垃圾：
/// - **落点目录不存在就建**（`-o 新目录/` 是常见用法），切失败时把这个新建的空目录收回去；
/// - **先写 `.yshjy-part` 再改名**（与解压六条落盘路径同一套）：中途失败不会留下
///   顶着正式名的半截视频 —— 半截 mp4 用户会当成「切出来的原片」，其实打不开。
pub fn extract_zip_cover_to(input: &Path, base: u64, dest: Option<&Path>) -> Result<Option<Cover>> {
    if base == 0 {
        return Ok(None);
    }
    let src = File::open(input)
        .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open input: {e}")))?;
    let total = src
        .metadata()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("stat: {e}")))?
        .len();
    if base > total {
        // 定位结果比文件还大 = 不可信，宁可不动
        return Err(Error::new(ErrorCode::Internal)
            .with("base", base.to_string())
            .with("size", total.to_string())
            .context("cover base out of range"));
    }

    let out_path = cover_path_in(input, dest);

    // 落点目录不存在就建；记下「是我们建的」，失败时把这个空目录收回去
    let mut 新建目录 = false;
    if let Some(d) = dest
        && !d.exists()
    {
        std::fs::create_dir_all(d).map_err(|e| {
            Error::new(ErrorCode::PermissionDenied).context(format!("create cover dir: {e}"))
        })?;
        新建目录 = true;
    }
    // 收尾用：删掉刚落下的半成品 + 我们自己建的空目录
    let 清理 = |临时: &Path| {
        let _ = std::fs::remove_file(临时);
        if 新建目录 && let Some(d) = dest {
            let _ = std::fs::remove_dir(d); // 非空就失败，忽略即可
        }
    };

    let 临时: PathBuf = {
        let mut s = out_path.clone().into_os_string();
        s.push(".yshjy-part");
        PathBuf::from(s)
    };
    let out = File::create(&临时).map_err(|e| {
        清理(&临时);
        Error::new(ErrorCode::PermissionDenied).context(format!("create cover: {e}"))
    })?;
    let mut w = BufWriter::with_capacity(1 << 20, out);

    // `take(base)` 把读取上限钉死：即使源文件后面还有 8 GB 压缩数据也不会多读
    let copied = std::io::copy(&mut src.take(base), &mut w).map_err(|e| {
        清理(&临时);
        Error::new(ErrorCode::Internal).context(format!("copy cover: {e}"))
    })?;
    if let Err(e) = w.flush() {
        清理(&临时);
        return Err(Error::new(ErrorCode::Internal).context(format!("flush cover: {e}")));
    }
    drop(w); // 关句柄再改名（Windows 上不关改不了）
    std::fs::rename(&临时, &out_path).map_err(|e| {
        清理(&临时);
        Error::new(ErrorCode::Internal).context(format!("rename cover: {e}"))
    })?;

    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(Some(Cover {
        path: out_path,
        size: copied,
        kind: cover_kind(&ext),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yshjy_cover_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn base为0时不产出封面() {
        let d = scratch("zero");
        let f = d.join("a.mp4");
        std::fs::write(&f, b"not a cover").unwrap();
        assert!(extract_zip_cover(&f, 0).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 切出的封面字节与源文件前缀一致且不覆盖原包() {
        let d = scratch("cut");
        let f = d.join("游戏.mp4");
        let head = b"FAKE-VIDEO-HEADER-0123456789";
        let tail = b"PK\x03\x04-compressed-payload";
        let mut data = Vec::new();
        data.extend_from_slice(head);
        data.extend_from_slice(tail);
        std::fs::write(&f, &data).unwrap();

        let c = extract_zip_cover(&f, head.len() as u64).unwrap().unwrap();
        assert_eq!(c.size, head.len() as u64);
        assert_eq!(c.kind, "video");
        assert_eq!(std::fs::read(&c.path).unwrap(), head);
        // 原包一个字节都没动
        assert_eq!(std::fs::read(&f).unwrap(), data);
        assert_ne!(c.path, f, "绝不能覆盖原包");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 重名时自动加序号() {
        let d = scratch("dupe");
        let f = d.join("x.mp4");
        std::fs::write(&f, b"AAAAbbbb").unwrap();
        let c1 = extract_zip_cover(&f, 4).unwrap().unwrap();
        let c2 = extract_zip_cover(&f, 4).unwrap().unwrap();
        assert_ne!(c1.path, c2.path, "第二次必须换名");
        assert!(c2.path.to_string_lossy().contains("(1)"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 图片扩展名识别为image() {
        assert_eq!(cover_kind("PNG"), "image");
        assert_eq!(cover_kind("mp4"), "video");
        assert_eq!(cover_kind("bin"), "other");
    }

    #[test]
    fn 指定目录时封面跟着_o走而且目录自动建() {
        let d = scratch("dest");
        let f = d.join("游戏.mp4");
        let head = b"FAKE-VIDEO-HEADER";
        let mut data = head.to_vec();
        data.extend_from_slice(b"PK\x03\x04payload");
        std::fs::write(&f, &data).unwrap();

        let out = d.join("输出目录").join("更深一层");
        assert!(!out.exists(), "前提：目标目录还不存在");
        let c = extract_zip_cover_to(&f, head.len() as u64, Some(&out))
            .unwrap()
            .unwrap();
        assert_eq!(
            c.path.parent(),
            Some(out.as_path()),
            "封面必须落在 -o 目录里，实际：{}",
            c.path.display()
        );
        assert_eq!(std::fs::read(&c.path).unwrap(), head);
        // 源包旁边不能出现副本 —— 老 bug 就是副本全撒在这儿（只读目录直接失败）
        let 旁边有封面 = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("封面"));
        assert!(!旁边有封面, "包旁边不该有封面副本");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 落点目录建不出来时报权限错误且两边都不动() {
        let d = scratch("baddest");
        let f = d.join("x.mp4");
        std::fs::write(&f, b"AAAAbbbb").unwrap();
        // dest 指到「已经是文件」的路径上 → create_dir_all 必失败
        let 挡住 = d.join("占位");
        std::fs::write(&挡住, b"x").unwrap();

        let e = extract_zip_cover_to(&f, 4, Some(&挡住)).unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied, "{e}");
        assert_eq!(std::fs::read(&f).unwrap(), b"AAAAbbbb", "原包不能被碰");
        assert_eq!(std::fs::read(&挡住).unwrap(), b"x", "占位文件不能被碰");
        let _ = std::fs::remove_dir_all(&d);
    }
}
