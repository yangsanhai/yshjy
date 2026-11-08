// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 文件类型探测 —— 「你给我的是啥文件」
//!
//! # 为什么需要它
//! 用户拖进来的可能根本不是压缩包（真的电影、真的照片、一个 ELF 程序…）。
//! 以前这种情况只会得到一句「没找到压缩包」—— 对新手等于什么都没说。
//! 这个模块**只看文件头**（最多几 KB），如实回答"它是什么"，让外壳能说：
//! 「这是一个 MP4 视频（约 800 MB），里面没找到压缩包」。
//!
//! # 边界（别长歪）
//! - **只判类型，不做鉴定**：不看内容是否完整、不判断时长/分辨率/是否加密。
//!   那些属于解码器的事，做了就变成一个不可靠的 `file(1)`。
//! - **类型名是机器标识**（`mp4` / `png` / `elf`），**不是人话** ——
//!   文案由外壳查语言包渲染（与 `error.rs` 同一条原则）。
//! - **找压缩包是 `job::identify` 的事**；这里只回答"这个文件本身是什么"。
//!   两者配合：定位失败时，用这里的结果告诉用户"所以你给的是个视频"。

use std::path::Path;

/// 文件类型（稳定标识 → 外壳查文案）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    // 视频容器
    /// MP4 视频（ISO BMFF，brand 多为 isom/mp42）
    Mp4,
    /// QuickTime 视频（.mov）
    Mov,
    /// Matroska 视频（.mkv）
    Mkv,
    /// WebM 视频
    Webm,
    /// AVI 视频
    Avi,
    /// FLV 视频
    Flv,
    /// MPEG-TS 流（电视频道、录播常见）
    Ts,
    // 图片
    /// PNG 图片
    Png,
    /// JPEG 图片
    Jpeg,
    /// GIF 图片（含动图）
    Gif,
    /// WebP 图片
    Webp,
    /// BMP 图片
    Bmp,
    /// TIFF 图片
    Tiff,
    // 音频
    /// MP3 音频
    Mp3,
    /// FLAC 无损音频
    Flac,
    /// OGG 音频
    Ogg,
    /// WAV 音频
    Wav,
    // 文档 / 程序
    /// PDF 文档
    Pdf,
    /// Linux 可执行程序
    Elf,
    /// Windows 可执行程序（PE）
    Exe,
    // 归档（与 `locate` 失败时的"其实是裸归档"区分开）
    /// ZIP 压缩包（也可能是 docx/apk 的容器）
    Zip,
    /// 7z 压缩包
    SevenZ,
    /// RAR 压缩包
    Rar,
    /// gzip 压缩流
    Gzip,
    /// xz 压缩流
    Xz,
    /// zstd 压缩流
    Zstd,
    /// bzip2 压缩流
    Bzip2,
    /// LZ4 压缩流
    Lz4,
    // 兜底
    /// 文本文件
    Text,
    /// 认不出（二进制 / 加密 / 文件头损坏）
    Unknown,
}

impl FileKind {
    /// 稳定标识（日志 / JSON / 前端查文案用）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Avi => "avi",
            Self::Flv => "flv",
            Self::Ts => "ts",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Wav => "wav",
            Self::Pdf => "pdf",
            Self::Elf => "elf",
            Self::Exe => "exe",
            Self::Zip => "zip",
            Self::SevenZ => "7z",
            Self::Rar => "rar",
            Self::Gzip => "gzip",
            Self::Xz => "xz",
            Self::Zstd => "zstd",
            Self::Bzip2 => "bzip2",
            Self::Lz4 => "lz4",
            Self::Text => "text",
            Self::Unknown => "unknown",
        }
    }

    /// 是不是"视频/图片/音频"这类**媒体**（伪装包最常用的外壳）
    pub fn is_media(self) -> bool {
        matches!(
            self,
            Self::Mp4
                | Self::Mov
                | Self::Mkv
                | Self::Webm
                | Self::Avi
                | Self::Flv
                | Self::Ts
                | Self::Png
                | Self::Jpeg
                | Self::Gif
                | Self::Webp
                | Self::Bmp
                | Self::Tiff
                | Self::Mp3
                | Self::Flac
                | Self::Ogg
                | Self::Wav
        )
    }

    /// 是不是**可执行/脚本**（外壳该给安全提醒）
    pub fn is_executable(self) -> bool {
        matches!(self, Self::Elf | Self::Exe)
    }

    /// 粗类别：`archive` / `media` / `document` / `program` / `unknown`
    ///
    /// 外壳拿它和扩展名暗示的类别对比，就能发现"名字与内容不符" ——
    /// 而这正是伪装包最典型的特征。
    pub fn category(self) -> &'static str {
        if self.is_archive() {
            "archive"
        } else if self.is_media() {
            "media"
        } else if self.is_executable() {
            "program"
        } else if matches!(self, Self::Pdf | Self::Text) {
            "document"
        } else {
            "unknown"
        }
    }

    /// 是不是归档（"本来就是压缩包，只是没定位到"）
    pub fn is_archive(self) -> bool {
        matches!(
            self,
            Self::Zip
                | Self::SevenZ
                | Self::Rar
                | Self::Gzip
                | Self::Xz
                | Self::Zstd
                | Self::Bzip2
                | Self::Lz4
        )
    }
}

/// 探测结果
#[derive(Debug, Clone)]
pub struct Probe {
    /// 类型
    pub kind: FileKind,
    /// 常见 MIME（给网页/日志用；不必强求精确）
    pub mime: &'static str,
    /// 补充细节（如 MP4 的 brand、MKV 的 doctype）——键值都是机器标识
    pub detail: Vec<(&'static str, String)>,
}

impl Probe {
    fn new(kind: FileKind, mime: &'static str) -> Self {
        Self {
            kind,
            mime,
            detail: Vec::new(),
        }
    }
}

/// 探测一个文件（只读文件头，最坏情况读 4 KiB）
pub fn probe_path(path: &Path) -> std::io::Result<Probe> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 4096];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    let len = f.metadata().map(|m| m.len()).unwrap_or(n as u64);
    Ok(probe_bytes(&buf, len))
}

/// 探测一段文件头（`len` 是文件总长度，用于"空文件"判断）
pub fn probe_bytes(head: &[u8], len: u64) -> Probe {
    if len == 0 || head.is_empty() {
        return Probe::new(FileKind::Unknown, "application/x-empty");
    }
    let at = |i: usize, n: usize| -> Option<&[u8]> { head.get(i..i + n) };

    // ── 图片 ──
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Probe::new(FileKind::Png, "image/png");
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Probe::new(FileKind::Jpeg, "image/jpeg");
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Probe::new(FileKind::Gif, "image/gif");
    }
    if head.len() > 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Probe::new(FileKind::Webp, "image/webp");
    }
    if head.starts_with(b"BM") && head.len() > 6 {
        return Probe::new(FileKind::Bmp, "image/bmp");
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return Probe::new(FileKind::Tiff, "image/tiff");
    }

    // ── ISO BMFF：MP4 / MOV / M4A 都是 `....ftyp<brand>` ──
    if let Some(t) = at(4, 4)
        && t == b"ftyp"
    {
        let brand = at(8, 4).map(|b| String::from_utf8_lossy(b).to_string());
        let is_qt = brand.as_deref() == Some("qt  ");
        let mut p = if is_qt {
            Probe::new(FileKind::Mov, "video/quicktime")
        } else {
            Probe::new(FileKind::Mp4, "video/mp4")
        };
        if let Some(b) = brand {
            p.detail.push(("brand", b.trim().to_string()));
        }
        return p;
    }

    // ── Matroska / WebM：EBML 头 1A45DFA3，DocType 在头几十字节里 ──
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let 窗口 = &head[..head.len().min(256)];
        let 是_webm = 窗口.windows(4).any(|w| w == b"webm");
        let 是_mkv = 窗口.windows(8).any(|w| w == b"matroska");
        let mut p = if 是_webm {
            Probe::new(FileKind::Webm, "video/webm")
        } else if 是_mkv {
            Probe::new(FileKind::Mkv, "video/x-matroska")
        } else {
            // EBML 也可能是别的（如某些加密文件）；不硬说是 mkv
            Probe::new(FileKind::Unknown, "application/octet-stream")
        };
        if 是_mkv || 是_webm {
            p.detail.push((
                "doctype",
                if 是_webm { "webm" } else { "matroska" }.to_string(),
            ));
        }
        return p;
    }

    // ── AVI / WAV（都是 RIFF）──
    if head.len() > 12 && &head[..4] == b"RIFF" {
        match &head[8..12] {
            b"AVI " => return Probe::new(FileKind::Avi, "video/x-msvideo"),
            b"WAVE" => return Probe::new(FileKind::Wav, "audio/wav"),
            _ => {}
        }
    }

    if head.starts_with(b"FLV") {
        return Probe::new(FileKind::Flv, "video/x-flv");
    }
    // MPEG-TS：同步字节 0x47，隔 188 再来一个（单看一个字节误判率太高）
    if head.first() == Some(&0x47) && head.get(188) == Some(&0x47) {
        return Probe::new(FileKind::Ts, "video/mp2t");
    }

    // ── 音频 ──
    if head.starts_with(b"fLaC") {
        return Probe::new(FileKind::Flac, "audio/flac");
    }
    if head.starts_with(b"OggS") {
        return Probe::new(FileKind::Ogg, "audio/ogg");
    }
    if head.starts_with(b"ID3")
        || (head.len() > 1 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0)
    {
        return Probe::new(FileKind::Mp3, "audio/mpeg");
    }

    // ── 文档 / 程序 ──
    if head.starts_with(b"%PDF") {
        return Probe::new(FileKind::Pdf, "application/pdf");
    }
    if head.starts_with(b"\x7FELF") {
        return Probe::new(FileKind::Elf, "application/x-executable");
    }
    if head.starts_with(b"MZ") {
        return Probe::new(FileKind::Exe, "application/x-msdownload");
    }

    // ── 归档（这些是"本来就是压缩包"，只是没定位成功）──
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Probe::new(FileKind::Zip, "application/zip");
    }
    if head.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Probe::new(FileKind::SevenZ, "application/x-7z-compressed");
    }
    if head.starts_with(b"Rar!\x1A\x07") {
        return Probe::new(FileKind::Rar, "application/vnd.rar");
    }
    if head.starts_with(&[0x1F, 0x8B, 0x08]) {
        return Probe::new(FileKind::Gzip, "application/gzip");
    }
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return Probe::new(FileKind::Xz, "application/x-xz");
    }
    if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Probe::new(FileKind::Zstd, "application/zstd");
    }
    if head.starts_with(b"BZh") {
        return Probe::new(FileKind::Bzip2, "application/x-bzip2");
    }
    if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        return Probe::new(FileKind::Lz4, "application/x-lz4");
    }

    // ── 文本？看可打印比例（UTF-8 允许高位字节，所以只把控制字符当"不可打印"）──
    let 样本 = &head[..head.len().min(2048)];
    let 控制字符 = 样本
        .iter()
        .filter(|b| **b < 0x09 || (**b > 0x0D && **b < 0x20))
        .count();
    let 比例 = 控制字符 as f64 / 样本.len().max(1) as f64;
    if 比例 < 0.02 && std::str::from_utf8(样本).is_ok() {
        return Probe::new(FileKind::Text, "text/plain");
    }

    Probe::new(FileKind::Unknown, "application/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 认出常见媒体() {
        // MP4（ftyp isom）
        let mut mp4 = vec![0, 0, 0, 0x20];
        mp4.extend_from_slice(b"ftypisom");
        mp4.extend_from_slice(&[0u8; 32]);
        assert_eq!(probe_bytes(&mp4, 100).kind, FileKind::Mp4);

        // QuickTime
        let mut mov = vec![0, 0, 0, 0x14];
        mov.extend_from_slice(b"ftypqt  ");
        assert_eq!(probe_bytes(&mov, 100).kind, FileKind::Mov);

        // PNG / JPEG / GIF
        assert_eq!(
            probe_bytes(b"\x89PNG\r\n\x1a\nxxxx", 12).kind,
            FileKind::Png
        );
        assert_eq!(probe_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0], 100).kind, FileKind::Jpeg);
        assert_eq!(probe_bytes(b"GIF89a......", 11).kind, FileKind::Gif);
    }

    #[test]
    fn 认出_webm_与_mkv_的区别() {
        let mut webm = vec![0x1A, 0x45, 0xDF, 0xA3];
        webm.extend_from_slice(&[0u8; 40]);
        webm.extend_from_slice(b"webm");
        let p = probe_bytes(&webm, 100);
        assert_eq!(p.kind, FileKind::Webm);
        assert_eq!(p.detail[0].1, "webm");

        let mut mkv = vec![0x1A, 0x45, 0xDF, 0xA3];
        mkv.extend_from_slice(&[0u8; 40]);
        mkv.extend_from_slice(b"matroska");
        assert_eq!(probe_bytes(&mkv, 100).kind, FileKind::Mkv);
    }

    #[test]
    fn 认出程序和归档() {
        assert_eq!(probe_bytes(b"\x7FELF\x02\x01\x01", 100).kind, FileKind::Elf);
        assert!(probe_bytes(b"\x7FELF\x02", 100).kind.is_executable());
        assert_eq!(probe_bytes(b"PK\x03\x04rest", 100).kind, FileKind::Zip);
        assert_eq!(probe_bytes(b"Rar!\x1A\x07\x00", 100).kind, FileKind::Rar);
        assert_eq!(probe_bytes(&[0x1F, 0x8B, 0x08, 0], 100).kind, FileKind::Gzip);
        // 7z 的魔数是 37 7A BC AF 27 1C
        assert_eq!(
            probe_bytes(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C], 100).kind,
            FileKind::SevenZ
        );
    }

    #[test]
    fn 文本与二进制分得开() {
        assert_eq!(probe_bytes("你好，世界\nhello\n".as_bytes(), 30).kind, FileKind::Text);
        assert_eq!(
            probe_bytes(&[0x00, 0x01, 0x02, 0x7F, 0x1B, 0x07], 6).kind,
            FileKind::Unknown
        );
    }

    #[test]
    fn 空文件不崩() {
        assert_eq!(probe_bytes(&[], 0).kind, FileKind::Unknown);
    }

    #[test]
    fn ts_要两处同步字节() {
        let mut ts = vec![0u8; 400];
        ts[0] = 0x47;
        ts[188] = 0x47;
        assert_eq!(probe_bytes(&ts, 400).kind, FileKind::Ts);
        // 只有一个 0x47 不能认
        let 假 = vec![0x47u8, 0, 0];
        assert_ne!(probe_bytes(&假, 3).kind, FileKind::Ts);
    }

    #[test]
    fn 分类判定合理() {
        assert!(FileKind::Mp4.is_media());
        assert!(!FileKind::Mp4.is_archive());
        assert!(FileKind::Zip.is_archive());
        assert!(FileKind::Elf.is_executable());
        assert!(!FileKind::Png.is_executable());
    }
}

/// 扩展名"暗示"的类别（与 [`FileKind::category`] 对比用）
///
/// 只收常见的、不会误伤的：`jpg`/`jpeg` 都算 `media`，
/// 不会因为扩展名写法不同就把正常文件报成"名字不符"。
pub fn extension_category(ext: &str) -> Option<&'static str> {
    let e = ext.trim_start_matches('.').to_ascii_lowercase();
    Some(match e.as_str() {
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "zst" | "zstd" | "bz2" | "lz4" | "br"
        | "z01" | "tgz" | "txz" => "archive",
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "ts" | "wmv" | "m4v" | "png" | "jpg"
        | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "mp3" | "flac" | "ogg" | "wav"
        | "m4a" | "aac" => "media",
        "pdf" | "txt" | "md" | "log" | "csv" | "doc" | "docx" | "xls" | "xlsx" | "ppt"
        | "pptx" => "document",
        "exe" | "dll" | "so" | "sh" | "run" | "appimage" => "program",
        _ => return None,
    })
}

#[cfg(test)]
mod category_tests {
    use super::*;

    #[test]
    fn 类别判定() {
        assert_eq!(FileKind::Mp4.category(), "media");
        assert_eq!(FileKind::Zip.category(), "archive");
        assert_eq!(FileKind::Pdf.category(), "document");
        assert_eq!(FileKind::Elf.category(), "program");
        assert_eq!(FileKind::Unknown.category(), "unknown");
    }

    #[test]
    fn 扩展名暗示的类别() {
        assert_eq!(extension_category("MP4"), Some("media"));
        assert_eq!(extension_category(".jpg"), Some("media"));
        assert_eq!(extension_category("7z"), Some("archive"));
        assert_eq!(extension_category("pdf"), Some("document"));
        assert_eq!(extension_category("xyz"), None);
    }
}
