// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 归档结构解析：从文件里定位数据、反推真实偏移
//!
//! 这一层只做「读懂结构、算准偏移」，**不做解压**。
//! 解压交给 `unpack` 层，两层分离便于各自测试。
//!
//! # 为什么必须自己解析（实测依据，见 03-技术验证.md）
//! - `zip` crate 的自动检测要扫完整个 8.7 GB 文件：**113 秒**
//! - 我们自己算出基准偏移后喂给它：**69 微秒**（快 1600 倍）
//! - 结论：**定位自己做，解压交给库**

pub mod zip;

pub use zip::ZipLayout;

/// 归档格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    /// ZIP / ZIP64
    Zip,
    /// 7-Zip
    SevenZ,
    /// RAR 1.5–4.x
    Rar4,
    /// RAR 5+
    Rar5,
    /// TAR
    Tar,
    /// gzip 单流
    Gzip,
    /// xz 单流
    Xz,
    /// zstd 单流
    Zstd,
    /// bzip2 单流
    Bzip2,
    /// Brotli 单流（**没有魔数**，只能按扩展名 `.br` 认）
    Brotli,
    /// LZ4 帧格式（魔数 `04 22 4D 18`）
    Lz4,
    /// 未知
    Unknown,
}

impl ArchiveFormat {
    /// 机器可读标识（日志/诊断用）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZ => "7z",
            Self::Rar4 => "rar4",
            Self::Rar5 => "rar5",
            Self::Tar => "tar",
            Self::Gzip => "gzip",
            Self::Xz => "xz",
            Self::Zstd => "zstd",
            Self::Bzip2 => "bzip2",
            Self::Brotli => "brotli",
            Self::Lz4 => "lz4",
            Self::Unknown => "unknown",
        }
    }
}

/// 条目信息（用于「解压前预览」，见 UI-SPEC §14.4）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// 条目在归档内的相对路径
    pub path: String,
    /// 是否为目录
    pub is_dir: bool,
    /// 原始大小（字节）
    pub size: u64,
    /// 压缩后大小（未知时为 None）
    pub compressed_size: Option<u64>,
    /// 是否加密 —— 界面上的 🔒 徽标就读这个字段
    pub encrypted: bool,
    /// 加密方式：`Some("zipcrypto")` / `Some("aes")`；未加密为 `None`
    ///
    /// ZIP 用「通用标志 bit0」表示传统 ZipCrypto、「压缩方法 99」表示 WinZip AES。
    pub enc_kind: Option<&'static str>,
    /// AES 强度（1=AES-128 / 2=AES-192 / 3=AES-256），来自 extra field 0x9901
    pub aes_strength: Option<u8>,
    /// CRC-32（校验用）
    pub crc32: Option<u32>,
    /// **声明的解压尺寸不可信**（审计 A-6）
    ///
    /// 中央目录是包自己写的，坏包/伪造包会声明出「724 B 的包里有 1.17 GB」这种数字。
    /// 判据是**数学硬上限**而不是拍脑袋的比例：DEFLATE 的理论极限是 1032:1
    /// （32 KiB 窗口 + 最长匹配 258 字节），超过它的声明不可能是真的。
    ///
    /// 该字段为 `true` 时，界面/命令行**不许把这个数字当真实大小显示**，
    /// 只能说「声明的尺寸异常」。目前只有 ZIP 会置位（别的格式没有同样的硬判据）。
    pub size_suspect: bool,
}

impl EntryInfo {
    /// 文件名（去掉目录部分），用于界面显示
    pub fn file_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }
}
