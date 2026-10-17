//! 内置封面素材
//!
//! 打包（伪装）时当"外壳"用。用户没指定就用内置的 ——
//! 免得用户手里没有合适的封面文件，卡在第一步。
//!
//! # 一体化设计
//! 素材用 `include_bytes!` 编进二进制，**不依赖用户下载任何东西**。
//! 而且都做得极小（合计约 2.3 KB），不会撑大程序。
//!
//! # 一条血泪规则：素材格式必须和输出扩展名一致
//! v0.9 之前 `ys -o 图.png` 产出的是「MP4 头 + .png 后缀」的假图片 ——
//! 因为外壳从没把扩展名传进来，一律用了内置小视频。看图软件按魔数一认就露馅。
//! 现在：扩展名决定素材（见 [`BuiltinCover::for_extension`]），
//! 并且 `pack::build` 会按封面魔数兜底降级布局（见那里的防呆注释）。

/// 内置封面素材
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinCover {
    /// 极小的黑屏视频（H.264 / 64×64 / 2 秒 / 约 1.7 KB）—— 最像真的，推荐
    TinyVideo,
    /// 极小图片（PNG / 64×64 / 约 270 B）
    TinyImage,
    /// 极小图片（JPEG / 64×64 / 约 320 B）—— `.jpg` 用它，别拿 PNG 冒充
    TinyJpeg,
    /// 文本文件（伪装成"说明.txt"）
    TinyText,
}

impl BuiltinCover {
    /// 全部内置素材
    pub const ALL: &'static [BuiltinCover] = &[
        Self::TinyVideo,
        Self::TinyImage,
        Self::TinyJpeg,
        Self::TinyText,
    ];

    /// 素材内容（已编译进二进制）
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::TinyVideo => include_bytes!("../../assets/cover/mini.mp4"),
            Self::TinyImage => include_bytes!("../../assets/cover/mini.png"),
            Self::TinyJpeg => include_bytes!("../../assets/cover/mini.jpg"),
            Self::TinyText => TEXT.as_bytes(),
        }
    }

    /// 素材大小（字节）
    pub fn size(self) -> usize {
        self.bytes().len()
    }

    /// 建议的输出文件名（用户看到的样子）
    pub fn default_name(self) -> &'static str {
        match self {
            Self::TinyVideo => "video.mp4",
            Self::TinyImage => "image.png",
            Self::TinyJpeg => "image.jpg",
            Self::TinyText => "readme.txt",
        }
    }

    /// 一句话描述（给界面显示用）
    pub fn description(self) -> &'static str {
        match self {
            Self::TinyVideo => "内置小视频（最像真的，推荐）",
            Self::TinyImage => "内置小图片（PNG）",
            Self::TinyJpeg => "内置小图片（JPEG）",
            Self::TinyText => "内置文本文件",
        }
    }

    /// 是不是视频类素材
    ///
    /// 只有 MP4 才有 ISO 盒子结构（`free` 原子），所以**只有它**能用
    /// [`crate::pack::PackLayout::FreeAtom`]；图片/文本塞 ISO 盒子会直接损坏。
    pub fn is_video(self) -> bool {
        matches!(self, Self::TinyVideo)
    }

    /// 根据目标扩展名挑选合适的素材
    ///
    /// 例：想伪装成 `.png` → 用内置 PNG；`.jpg` → 用内置 JPEG；
    /// 想伪装成 `.txt` → 用内置文本。
    ///
    /// ⚠ `gif` / `webp` / `bmp` / `tif` 暂时共用 PNG 素材：能靠魔数打开，
    /// 但严格按扩展名解析的软件可能不认。要精确匹配请用 `--cover` 指定自己的封面。
    pub fn for_extension(ext: &str) -> Self {
        let e = ext.trim_start_matches('.').to_ascii_lowercase();
        match e.as_str() {
            "png" => Self::TinyImage,
            "jpg" | "jpeg" => Self::TinyJpeg,
            "gif" | "webp" | "bmp" | "tif" | "tiff" => Self::TinyImage,
            "txt" | "md" | "log" | "ini" | "cfg" | "csv" => Self::TinyText,
            _ => Self::TinyVideo,
        }
    }
}

/// 文本素材：伪装成"这是个说明文件"的样子
const TEXT: &str = "\u{feff}使用说明\r\n========\r\n\r\n本文件由 yshjy 自动生成。\r\n\r\n这是一个占位用的文本外壳文件。\r\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 素材都非空且体积可控() {
        let mut total = 0usize;
        for c in BuiltinCover::ALL {
            let n = c.size();
            assert!(n > 0, "{:?} 不能为空", c);
            total += n;
        }
        // 全部素材合计应远小于 100 KB（避免撑大程序）
        assert!(total < 100 * 1024, "内置素材合计 {total} 字节，太大了");
    }

    #[test]
    fn 视频素材是合法mp4() {
        let b = BuiltinCover::TinyVideo.bytes();
        assert!(b.len() > 12);
        // MP4 的第 4..8 字节是 'ftyp'
        assert_eq!(
            &b[4..8],
            b"ftyp",
            "内置视频必须是合法 MP4（前 8 字节 %PDF 之外要有 ftyp）"
        );
    }

    #[test]
    fn 图片素材是合法png() {
        let b = BuiltinCover::TinyImage.bytes();
        assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "内置图片必须是合法 PNG 头");
    }

    #[test]
    fn jpg_素材是合法jpeg() {
        let b = BuiltinCover::TinyJpeg.bytes();
        assert!(b.len() > 100);
        assert_eq!(&b[..3], &[0xFF, 0xD8, 0xFF], "内置 .jpg 必须是合法 JPEG（FFD8FF）");
    }

    #[test]
    fn 按扩展名挑选合理() {
        assert_eq!(BuiltinCover::for_extension("png"), BuiltinCover::TinyImage);
        assert_eq!(BuiltinCover::for_extension(".PNG"), BuiltinCover::TinyImage);
        assert_eq!(BuiltinCover::for_extension("jpg"), BuiltinCover::TinyJpeg);
        assert_eq!(BuiltinCover::for_extension(".Jpeg"), BuiltinCover::TinyJpeg);
        assert_eq!(BuiltinCover::for_extension("txt"), BuiltinCover::TinyText);
        assert_eq!(BuiltinCover::for_extension("mp4"), BuiltinCover::TinyVideo);
        assert_eq!(BuiltinCover::for_extension("mkv"), BuiltinCover::TinyVideo);
    }

    #[test]
    fn 只有视频素材允许_free_原子布局() {
        assert!(BuiltinCover::TinyVideo.is_video());
        assert!(!BuiltinCover::TinyImage.is_video());
        assert!(!BuiltinCover::TinyJpeg.is_video());
        assert!(!BuiltinCover::TinyText.is_video());
    }

    #[test]
    fn 文件名与描述都不为空() {
        for c in BuiltinCover::ALL {
            assert!(!c.default_name().is_empty());
            assert!(!c.description().is_empty());
        }
    }
}
