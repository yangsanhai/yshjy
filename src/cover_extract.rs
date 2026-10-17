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

/// 由原包路径推出封面路径：`a.mp4` → `a.封面.mp4`
///
/// 重名时加 ` (1)`（与解压的 rename 策略一致），**永不返回原包路径**。
pub fn cover_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
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

/// 切出封面区 `[0, base)`
///
/// - `base == 0` → 没有封面区（包就在文件开头），返回 `Ok(None)`
/// - 输出已存在 → 自动改名，不覆盖
/// - 只复制 `base` 字节，绝不多读
pub fn extract_zip_cover(input: &Path, base: u64) -> Result<Option<Cover>> {
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

    let out_path = cover_path(input);
    let out = File::create(&out_path)
        .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("create cover: {e}")))?;
    let mut w = BufWriter::with_capacity(1 << 20, out);

    // `take(base)` 把读取上限钉死：即使源文件后面还有 8 GB 压缩数据也不会多读
    let copied = std::io::copy(&mut src.take(base), &mut w)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("copy cover: {e}")))?;
    w.flush()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("flush cover: {e}")))?;

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

/// 可选的 ffmpeg remux：把封面视频的 moov 索引修好（不装 ffmpeg 就跳过）
///
/// 不做转码，只 `-c copy` 重写容器；失败一律忽略（封面已经能播，修不了也不影响）。
pub fn try_remux_with_ffmpeg(cover: &Cover) -> bool {
    if cover.kind != "video" {
        return false;
    }
    let tmp = cover.path.with_extension("remux.tmp.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-i",
            &cover.path.to_string_lossy(),
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            &tmp.to_string_lossy(),
        ])
        .status();
    match status {
        Ok(s) if s.success() && tmp.exists() => {
            // 用 remux 结果替换（保留原封面名）
            let _ = std::fs::rename(&tmp, &cover.path);
            true
        }
        _ => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
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
}
