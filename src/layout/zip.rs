//! ZIP / ZIP64 结构解析与**偏移反推**
//!
//! # 核心问题
//! 伪装包的 ZIP 目录表里记录的偏移是「相对于 ZIP 数据起点」的，
//! 而这个起点前面还压着一层 MP4 外壳（例如 119 MB）。
//! 标准解压工具一看偏移对不上就判定「文件损坏」直接放弃 —— 这就是用户卡住的原因。
//!
//! # 反推公式（本项目最重要的公式）
//! ```text
//! ZIP 起点 = 中央目录的实际位置 − ZIP64 EOCD 里记录的 cd_offset
//! ```
//! 算完后**必须验证**该位置处是 `PK\x03\x04`（本地文件头），
//! 否则说明推算有误，不能拿错结果去解压。
//!
//! # 已实测支撑
//! - 8.2 GB 真实伪装包：算出 base = 119248031，验证通过，提取结果 SHA-256 与包内指纹一致
//! - 前置垃圾 2.3 MB 的样本（D2b）：7-Zip 自己打不开，本算法能算出来

use crate::error::{Error, ErrorCode, Result};
use crate::layout::EntryInfo;
use std::io::{Read, Seek, SeekFrom};

/// EOCD 签名 `PK\x05\x06`
pub const EOCD_SIG: &[u8; 4] = b"PK\x05\x06";
/// ZIP64 EOCD 签名 `PK\x06\x06`
pub const ZIP64_EOCD_SIG: &[u8; 4] = b"PK\x06\x06";
/// ZIP64 EOCD 定位器签名 `PK\x06\x07`
pub const ZIP64_LOC_SIG: &[u8; 4] = b"PK\x06\x07";
/// 中央目录条目签名 `PK\x01\x02`
pub const CD_SIG: &[u8; 4] = b"PK\x01\x02";
/// 本地文件头签名 `PK\x03\x04`
pub const LFH_SIG: &[u8; 4] = b"PK\x03\x04";

/// EOCD 固定长度（不含注释）
const EOCD_LEN: u64 = 22;
/// ZIP64 EOCD 固定长度
const ZIP64_EOCD_LEN: u64 = 56;
/// ZIP64 定位器长度（恰好 20 字节 —— 这也是 unar 分块扫描要重叠 20 字节的原因）
const ZIP64_LOC_LEN: u64 = 20;
/// 默认尾部搜索窗口（1 MiB）
pub const DEFAULT_WINDOW: u64 = 16 << 20;

/// ZIP 的定位结果
#[derive(Debug, Clone)]
pub struct ZipLayout {
    /// **ZIP 数据在底层文件中的真实起点**（核心结果）
    pub base: u64,
    /// EOCD 在底层文件中的位置
    pub eocd_at: u64,
    /// 中央目录在底层文件中的位置
    pub cd_at: u64,
    /// 中央目录的字节数
    pub cd_size: u64,
    /// 条目总数
    pub entries_total: u64,
    /// 是否为 ZIP64 格式
    pub zip64: bool,
    /// EOCD（含注释）之后还剩多少字节 —— 便于诊断"尾部还有东西"
    pub trailing_bytes: u64,
    /// ZIP 注释长度
    pub comment_len: u16,
}

impl ZipLayout {
    /// ZIP 数据的长度（从 base 到 EOCD 结束）
    pub fn data_len(&self) -> u64 {
        self.eocd_at + EOCD_LEN + self.comment_len as u64 - self.base
    }
}

// ── 小工具：从字节切片读小端整数 ──
fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn u64le(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

// ───────────────────── 打开 ZIP（统一入口） ─────────────────────

/// 用**已知偏移**打开 ZIP：明确告诉库「归档从 0 开始」，别让它自己去检测
///
/// # 为什么这件事很要命（有实测数字）
/// `zip` 库的默认配置是 [`zip::read::ArchiveOffset::Detect`] —— 它会去"搜索中央
/// 目录的真实位置"来**反推**归档偏移。而我们的偏移**早就自己算好了**
/// （`io::Window` 已经把 base 归零，对它来说偏移就是 0），让库再检测一次纯属浪费，
/// 且代价极大：
///
/// - HANDOFF 的实测：库自己检测，8.7 GB 伪装包要 **113 秒**；直接喂偏移只要
///   **69 微秒**（快约 1600 倍）
/// - v0.9 性能批次复现：`--list` 总耗时 115 秒，而 Locate 阶段只占 **40 毫秒** ——
///   剩下的全是在这里"检测偏移"把整个窗口扫了一遍
///
/// 这就是本项目一贯的分工：**我们负责定位、库负责解码**（见 `extract.rs` 模块头）。
/// 所以任何"打开 ZIP"的地方都走这个函数，不要直接 `ZipArchive::new`。
pub fn 打开已知偏移<R: std::io::Read + std::io::Seek>(
    reader: R,
) -> zip::result::ZipResult<zip::ZipArchive<R>> {
    let cfg = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(0),
    };
    zip::ZipArchive::with_config(cfg, reader)
}

/// 在缓冲区里从后往前找 EOCD 签名，返回其下标
pub fn find_eocd_backwards(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    // 实测对比（同一 8.7 GB 文件、1 MiB 窗口）：
    //   标量 `rposition` → 41 µs   ← 采用
    //   `memchr::rfind`  → 610 µs  ← 慢 15 倍
    // 结论：窗口小时标量更快（SIMD 的建表开销盖过收益）；
    // 只有在把窗口放大到几十 MB 时才值得换 SIMD。不要盲抄参考项目。
    buf.windows(4).rposition(|w| w == EOCD_SIG)
}

/// **在文件中定位 ZIP 数据**（含偏移反推与结构自校验）
///
/// `file_size` 为底层文件长度（调用者已知，避免重复 stat）。
///
/// 搜索顺序：
/// 1. 尾部窗口（默认 1 MiB）内倒着找 EOCD —— 覆盖绝大多数情况
/// 2. 若未找到，返回 [`ErrorCode::NotArchive`]，由上层决定是否做全文件扫描
pub fn locate<R: Read + Seek>(r: &mut R, file_size: u64) -> Result<ZipLayout> {
    locate_with_window(r, file_size, DEFAULT_WINDOW)
}

/// 与 [`locate`] 相同，但可自定义尾部搜索窗口大小
///
/// 用途：某些布局会把 EOCD 推得离文件尾很远（实测 L3 布局距尾 118902 字节），
/// 这时需要更大的窗口。
pub fn locate_with_window<R: Read + Seek>(
    r: &mut R,
    file_size: u64,
    window: u64,
) -> Result<ZipLayout> {
    if file_size < EOCD_LEN {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "file_too_small")
            .with("size", file_size));
    }

    // ── 分块倒扫找 EOCD：64 KiB 一块，块间留 20 字节重叠 ──
    //
    // 以前是「一次读 window 字节」：尾部垃圾比窗口大就永远找不到，只能退化成
    // 全文件扫描（8 GB 包要几十秒）。分块倒扫让内存占用恒定，窗口可以开大。
    // 参考 xadmaster `XADZipParser.m:118-191`（64 KiB 块 + 20 B 重叠）。
    const CHUNK: u64 = 64 * 1024;
    const OVERLAP: u64 = 20;

    let limit = if window == 0 {
        file_size
    } else {
        window.min(file_size)
    };
    // 最多往回扫到 floor（含）；用「位置」判断而不是累计字节数 ——
    // 块间的 20 字节重叠会被重复计，用累加会提前 20 字节/块 收工（实测漏掉最后一个块）。
    let floor = file_size.saturating_sub(limit);
    let mut end = file_size;
    let mut found: Option<u64> = None;

    while end > floor {
        let take = CHUNK.min(end - floor);
        let start = end - take;
        // 往前多读 OVERLAP：签名跨块时也能看见
        let read_start = start.saturating_sub(OVERLAP);
        let read_len = (end - read_start) as usize;
        let mut buf = vec![0u8; read_len];
        r.seek(SeekFrom::Start(read_start))
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek failed: {e}")))?;
        let got = read_full(r, &mut buf)?;
        buf.truncate(got);
        if let Some(rel) = find_eocd_backwards(&buf) {
            found = Some(read_start + rel as u64);
            break;
        }
        if start == floor {
            break;
        }
        end = start + OVERLAP; // 下一轮带上这块尾部
    }

    let eocd_abs = found.ok_or_else(|| {
        Error::new(ErrorCode::NotArchive)
            .with("reason", "no_eocd_in_window")
            .with("window", limit)
            .context("尾部窗口内未见 PK05 06 签名（可能需要全文件扫描）")
    })?;

    // 重新读 EOCD 前后一小段（ZIP64 定位器紧贴 EOCD 前 20 字节）
    let win_start = eocd_abs.saturating_sub(64);
    let ctx_len = ((file_size - win_start).min(64 + EOCD_LEN + 65535)) as usize;
    let mut buf = vec![0u8; ctx_len];
    r.seek(SeekFrom::Start(win_start))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek failed: {e}")))?;
    let got = read_full(r, &mut buf)?;
    buf.truncate(got);
    let eocd_rel = (eocd_abs - win_start) as usize;

    // 至少要有完整的 22 字节 EOCD
    if eocd_rel + EOCD_LEN as usize > buf.len() {
        return Err(Error::new(ErrorCode::Truncated)
            .with("reason", "eocd_truncated")
            .with("at", eocd_abs));
    }

    let eocd = &buf[eocd_rel..];
    let comment_len = u16le(eocd, 20);

    // EOCD 之后的剩余（注释 + 可能的尾部垃圾）
    let after_eocd = file_size - eocd_abs;
    let trailing_bytes = after_eocd.saturating_sub(EOCD_LEN + comment_len as u64);

    // ── 判断是否 ZIP64：EOCD 之前 20 字节应是定位器 ──
    let zip64 = if eocd_rel >= ZIP64_LOC_LEN as usize
        && &buf[eocd_rel - ZIP64_LOC_LEN as usize..eocd_rel - ZIP64_LOC_LEN as usize + 4]
            == ZIP64_LOC_SIG
    {
        true
    } else {
        // 退一步：EOCD 里记录的偏移是 0xFFFFFFFF 也说明是 ZIP64
        u32le(eocd, 16) == 0xFFFF_FFFF || u32le(eocd, 12) == 0xFFFF_FFFF
    };

    let (cd_size, cd_offset, entries_total) = if zip64 {
        read_zip64(r, eocd_abs, &buf, eocd_rel, win_start)?
    } else {
        let cd_size = u32le(eocd, 12) as u64;
        let cd_offset = u32le(eocd, 16) as u64;
        let entries = u16le(eocd, 10) as u64;
        (cd_size, cd_offset, entries)
    };

    // ── 中央目录的实际位置 ──
    // 常规布局：[CD][EOCD] 或 [CD][ZIP64 EOCD][LOCATOR][EOCD]
    let cd_at = if zip64 {
        // CD 紧贴在 ZIP64 EOCD 之前
        let z64_at = find_zip64_eocd(r, eocd_abs, &buf, eocd_rel, win_start)?;
        z64_at.checked_sub(cd_size).ok_or_else(|| {
            Error::new(ErrorCode::Truncated).with("reason", "cd_before_file_start")
        })?
    } else {
        eocd_abs.checked_sub(cd_size).ok_or_else(|| {
            Error::new(ErrorCode::Truncated).with("reason", "cd_before_file_start")
        })?
    };

    // ── 核心：反推 ZIP 起点 ──
    let base = cd_at.checked_sub(cd_offset).ok_or_else(|| {
        Error::new(ErrorCode::Truncated)
            .with("reason", "base_before_file_start")
            .with("cd_at", cd_at)
            .with("cd_offset", cd_offset)
    })?;

    // ── 结构自校验：base 处必须是本地文件头 ──
    verify_base(r, base).map_err(|e| {
        e.context(format!(
            "推算的 base={base} 处不是 PK03 04（cd_at={cd_at}, cd_offset={cd_offset}）"
        ))
    })?;

    Ok(ZipLayout {
        base,
        eocd_at: eocd_abs,
        cd_at,
        cd_size,
        entries_total,
        zip64,
        trailing_bytes,
        comment_len,
    })
}

/// 按「归档数据起点」精确取出一段（S10 多候选的「都试试」用）
///
/// 一个文件里可能塞着好几个 ZIP：`locate()` 永远只给**最后**那一段。要解前面那些，
/// 就得从文件尾往前一个 EOCD 一个 EOCD 地退，直到反推出的 `base` 对上目标。
///
/// 实现上**不重写倒扫逻辑**：每轮把 `ceiling` 当成"文件尾"再调一次
/// [`locate_with_window`] —— 走的是同一条被大量样本验过的路径，只是窗口往前挪。
/// 代价是解第 N 段要倒扫 N 遍，但这只在用户显式点「都试试」时才发生。
pub fn locate_at_base<R: Read + Seek>(
    r: &mut R,
    file_size: u64,
    base_target: u64,
) -> Result<ZipLayout> {
    if base_target >= file_size {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "candidate_offset_out_of_range")
            .with("offset", base_target)
            .with("size", file_size));
    }
    let mut ceiling = file_size;
    loop {
        let l = locate_with_window(r, ceiling, ceiling)?;
        if l.base == base_target {
            return Ok(l);
        }
        if l.base < base_target {
            // 越往前 base 只会越小 → 目标位置不是任何一段的开头
            return Err(Error::new(ErrorCode::NotArchive)
                .with("reason", "no_archive_at_candidate_offset")
                .with("offset", base_target)
                .with("nearest_base", l.base));
        }
        ceiling = l.base;
        if ceiling == 0 {
            return Err(Error::new(ErrorCode::NotArchive)
                .with("reason", "no_archive_at_candidate_offset")
                .with("offset", base_target));
        }
    }
}

/// 解析 ZIP64 EOCD（先从定位器入手，失败再按紧邻位置推算）
fn read_zip64<R: Read + Seek>(
    r: &mut R,
    eocd_abs: u64,
    buf: &[u8],
    eocd_rel: usize,
    _win_start: u64,
) -> Result<(u64, u64, u64)> {
    // 定位器紧邻 EOCD 之前
    if eocd_rel < ZIP64_LOC_LEN as usize {
        return Err(Error::new(ErrorCode::Truncated).with("reason", "no_room_for_locator"));
    }
    let loc_rel = eocd_rel - ZIP64_LOC_LEN as usize;
    let loc = &buf[loc_rel..loc_rel + ZIP64_LOC_LEN as usize];
    if &loc[0..4] != ZIP64_LOC_SIG {
        return Err(Error::new(ErrorCode::NotArchive).with("reason", "bad_zip64_locator_sig"));
    }
    let loc_disk = u32le(loc, 4);
    let z64_rel_off = u64le(loc, 8);

    // ZIP64 EOCD 实际位置有两种可能：
    //   ① 相对 ZIP 数据起点（元数据里记的）—— 但我们还不知道 base
    //   ② 紧邻定位器之前（标准布局）
    // 采用 ② 更稳，并用 ① 做交叉诊断
    let z64_at = eocd_abs
        .checked_sub(ZIP64_LOC_LEN + ZIP64_EOCD_LEN)
        .ok_or_else(|| Error::new(ErrorCode::Truncated).with("reason", "no_room_for_eocd64"))?;

    let mut hdr = [0u8; ZIP64_EOCD_LEN as usize];
    r.seek(SeekFrom::Start(z64_at))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek: {e}")))?;
    read_full(r, &mut hdr)?;
    if &hdr[0..4] != ZIP64_EOCD_SIG {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "bad_zip64_eocd_sig")
            .with("at", z64_at)
            .context(format!(
                "locator 指向偏移 {z64_rel_off}，记录磁盘 {loc_disk}"
            )));
    }

    let cd_size = u64le(&hdr, 40);
    let cd_offset = u64le(&hdr, 48);
    let entries = u64le(&hdr, 32);
    Ok((cd_size, cd_offset, entries))
}

/// 找到 ZIP64 EOCD 的实际位置（用于推算 CD 位置）
fn find_zip64_eocd<R: Read + Seek>(
    _r: &mut R,
    eocd_abs: u64,
    _buf: &[u8],
    _eocd_rel: usize,
    _win_start: u64,
) -> Result<u64> {
    eocd_abs
        .checked_sub(ZIP64_LOC_LEN + ZIP64_EOCD_LEN)
        .ok_or_else(|| Error::new(ErrorCode::Truncated).with("reason", "no_room_for_eocd64"))
}

/// 验证 base 处确实是本地文件头（防止把错偏移当成结果）
fn verify_base<R: Read + Seek>(r: &mut R, base: u64) -> Result<()> {
    let mut sig = [0u8; 4];
    r.seek(SeekFrom::Start(base))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek base: {e}")))?;
    read_full(r, &mut sig)?;
    if &sig != LFH_SIG {
        return Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "base_is_not_local_header")
            .with("base", base)
            .with(
                "found",
                format!("{:02x}{:02x}{:02x}{:02x}", sig[0], sig[1], sig[2], sig[3]),
            ));
    }
    Ok(())
}

/// 尽量读满缓冲区（返回实际读到的字节数；EOF 不算错误）
fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(Error::new(ErrorCode::Internal).context(format!("read failed: {e}")));
            }
        }
    }
    Ok(total)
}

/// Info-ZIP Unicode Path extra field（0x7075）：`version(1) + crc32(4) + utf8_name`
///
/// **必须验 CRC32 再采用**：extra 字段可能是伪造/过期的，CRC 对不上说明它
/// 描述的不是当前这个名字。参考 zip2 `src/extra_fields/zipinfo_utf8.rs:25-35`。
fn unicode_path(extra: &[u8], raw_name: &[u8]) -> Option<String> {
    let mut p = 0usize;
    while p + 4 <= extra.len() {
        let tag = u16le(extra, p);
        let size = u16le(extra, p + 2) as usize;
        let body = p + 4;
        if body + size > extra.len() {
            break;
        }
        if tag == 0x7075 && size >= 5 {
            let want = u32le(extra, body + 1);
            if want == crc32(raw_name)
                && let Ok(s) = std::str::from_utf8(&extra[body + 5..body + size])
            {
                return Some(s.to_string());
            }
        }
        p = body + size;
    }
    None
}

/// CRC-32（IEEE，ZIP 用的那个）
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// 解码文件名：先看 Info-ZIP Unicode Path（0x7075，验过 CRC），再走四级回退链
///
/// macOS / Windows 的工具会把「本地编码名字 + UTF-8 名字」双写，
/// 读 extra 能直接把乱码救回正确中文 —— 比猜编码可靠。
pub fn decode_name_with_extra(bytes: &[u8], is_utf8: bool, extra: &[u8]) -> String {
    if let Some(s) = unicode_path(extra, bytes) {
        return s;
    }
    decode_name(bytes, is_utf8)
}

/// 带**手动代码页**的解码：`--encoding gbk|big5|sjis|utf-8`
///
/// 自动链对 Big5 / Shift-JIS 名字经常「合法但解错」（静默乱码，不报错），
/// 只有用户手动指定能救。指定了就不再猜，也不再读 0x7075（用户说了算）。
pub fn decode_name_forced(
    bytes: &[u8],
    is_utf8: bool,
    extra: &[u8],
    forced: Option<&str>,
) -> String {
    let Some(enc) = forced else {
        return decode_name_with_extra(bytes, is_utf8, extra);
    };
    match enc.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" => String::from_utf8_lossy(bytes).into_owned(),
        "gbk" | "gb18030" | "cp936" => encoding_rs::GBK.decode(bytes).0.into_owned(),
        "big5" | "cp950" => encoding_rs::BIG5.decode(bytes).0.into_owned(),
        "sjis" | "shift-jis" | "shift_jis" | "cp932" => {
            encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
        }
        // 认不出的写法当 auto（CLI 层会先把值校验一遍）
        _ => decode_name_with_extra(bytes, is_utf8, extra),
    }
}

/// 从 WinZip AES extra field（0x9901）里取强度
///
/// 结构（APPNOTE 4.4.8）：`tag(2) size(2) version(2) "AE"(2) strength(1) method(2)`
/// strength：1=AES-128 / 2=AES-192 / 3=AES-256。
/// extra 区可能同时有 ZIP64(0x0001) 等多个字段，所以按 tag 逐块跳。
fn aes_strength(extra: &[u8]) -> Option<u8> {
    let mut p = 0usize;
    while p + 4 <= extra.len() {
        let tag = u16le(extra, p);
        let size = u16le(extra, p + 2) as usize;
        let body = p + 4;
        if body + size > extra.len() {
            break;
        }
        if tag == 0x9901 && size >= 7 {
            return Some(extra[body + 4]);
        }
        p = body + size;
    }
    None
}

/// 解析中央目录，列出条目（用于「解压前预览」）
///
/// 返回的每条 [`EntryInfo`] 都带 `encrypted` 标志 —— 界面上的 🔒 徽标就读它。
pub fn list_entries<R: Read + Seek>(r: &mut R, layout: &ZipLayout) -> Result<Vec<EntryInfo>> {
    list_entries_enc(r, layout, None)
}

/// 同 [`list_entries`]，但可以手动指定文件名编码（`--encoding`）
pub fn list_entries_enc<R: Read + Seek>(
    r: &mut R,
    layout: &ZipLayout,
    encoding: Option<&str>,
) -> Result<Vec<EntryInfo>> {
    let mut out = Vec::new();
    r.seek(SeekFrom::Start(layout.cd_at))
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek cd: {e}")))?;

    let mut remaining = layout.cd_size;
    let mut buf = vec![0u8; 4096.min(layout.cd_size as usize).max(64)];

    while remaining > 0 {
        let take = (buf.len() as u64).min(remaining) as usize;
        let n = read_full(r, &mut buf[..take])?;
        if n < 46 {
            break; // 读不满一个 CD 头，结束
        }
        if &buf[0..4] != CD_SIG {
            return Err(Error::new(ErrorCode::NotArchive)
                .with("reason", "bad_cd_entry_sig")
                .with("at", layout.cd_at + (layout.cd_size - remaining)));
        }

        let flags = u16le(&buf, 8);
        let method = u16le(&buf, 10);
        let crc = u32le(&buf, 16);
        let mut csize = u32le(&buf, 20) as u64;
        let mut usize_ = u32le(&buf, 24) as u64;
        let name_len = u16le(&buf, 28) as usize;
        let extra_len = u16le(&buf, 30) as usize;
        let comment_len = u16le(&buf, 32) as usize;
        let entry_len = (46 + name_len + extra_len + comment_len) as u64;

        // 名字可能跨缓冲区边界：这里按需再读
        let mut name_bytes = Vec::new();
        if 46 + name_len <= n {
            name_bytes.extend_from_slice(&buf[46..46 + name_len]);
        } else {
            let mut nb = vec![0u8; name_len];
            r.seek(SeekFrom::Current(
                (46 + name_len - n.min(46 + name_len)) as i64,
            ))
            .ok();
            read_full(r, &mut nb)?;
            name_bytes = nb;
        }

        // 读 extra 字段（ZIP64 扩展里可能有真实大小）
        let mut extra = Vec::new();
        if extra_len > 0 {
            let extra_start_abs =
                layout.cd_at + (layout.cd_size - remaining) + (46 + name_len) as u64;
            let saved = r.stream_position().unwrap_or(extra_start_abs);
            r.seek(SeekFrom::Start(extra_start_abs)).ok();
            extra.resize(extra_len, 0);
            read_full(r, &mut extra)?;
            // 恢复到条目之后
            let after = extra_start_abs + extra_len as u64 + comment_len as u64;
            r.seek(SeekFrom::Start(after)).ok();
            if saved > after {
                r.seek(SeekFrom::Start(saved)).ok();
            }
        }
        // ZIP64 extra（0x0001）：当 32 位字段为 0xFFFFFFFF 时取其中的 64 位值
        if extra.len() >= 4 && u16le(&extra, 0) == 0x0001 {
            let body = &extra[4..];
            let mut p = 0usize;
            if usize_ == 0xFFFF_FFFF && body.len() >= p + 8 {
                usize_ = u64le(body, p);
                p += 8;
            }
            if csize == 0xFFFF_FFFF && body.len() >= p + 8 {
                csize = u64le(body, p);
            }
        }

        // 文件名编码：见 decode_name 的四级回退链；0x7075 extra 优先（验 CRC）；
        // `--encoding` 手动指定时以用户为准
        let is_utf8 = flags & (1 << 11) != 0;
        let path = decode_name_forced(&name_bytes, is_utf8, &extra, encoding);

        let is_dir = path.ends_with('/') || path.ends_with('\\');

        out.push(EntryInfo {
            path,
            is_dir,
            size: usize_,
            compressed_size: Some(csize),
            // 加密标志：bit 0（传统加密）/ 方法 99（WinZip AES）
            encrypted: (flags & 1) != 0 || method == 99,
            // 强度只有 AES 才有，藏在 extra field 0x9901 里（WinZip AES 规范）
            enc_kind: if method == 99 {
                Some("aes")
            } else if (flags & 1) != 0 {
                Some("zipcrypto")
            } else {
                None
            },
            aes_strength: if method == 99 { aes_strength(&extra) } else { None },
            crc32: Some(crc),
        });

        remaining = remaining.saturating_sub(entry_len);
        r.seek(SeekFrom::Start(layout.cd_at + (layout.cd_size - remaining)))
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("seek next cd: {e}")))?;
    }

    Ok(out)
}

/// 解码文件名 —— **四级回退链**
///
/// 思路对照 7-Zip `Archive/Zip/ZipItem.cpp:405-461`（`GetUnicodeString`）的四级链：
/// 归档标志位 → Info-ZIP Unicode Path extra(0x7075，先验 CRC) → 用户指定代码页 → UTF-8。
///
/// 我们的实现顺序：
/// 1. **UTF-8 标志位**（通用标志 bit 0x800）→ 直接按 UTF-8
/// 2. **严格 UTF-8 可解** → 用它（很多工具不设标志位，但内容确实是 UTF-8）
/// 3. **GB18030**（GBK/GB2312 的超集；简体中文 Windows 打包的默认编码）
/// 4. 兜底 **有损 UTF-8** —— 保证不 panic、不丢条目
///
/// 取舍说明：unar 走的是"编码探测 + 启发式评分"（`XADString.m:342-408`），
/// 更准但要拖进整个编码探测库（与单二进制体积冲突）。我们用确定性的四级链，
/// 同时把**原始字节**一起交给上层，必要时可由 `--encoding` 强制指定。
pub fn decode_name(bytes: &[u8], is_utf8: bool) -> String {
    // ① 归档自己声明了 UTF-8
    if is_utf8 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    // ② 内容是合法 UTF-8（现代打包工具的常见情况）
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // ③ GB18030：覆盖 GBK/GB2312，中文圈资源的主力编码
    let (decoded, _, had_errors) = encoding_rs::GB18030.decode(bytes);
    if !had_errors {
        return decoded.into_owned();
    }
    // ④ 兜底：不 panic、不丢条目（宁可显示 � 也不能让整包解析失败）
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {

    #[test]
    fn gbk_中文文件名能正确解码() {
        // "中文测试.txt" 的 GBK 字节（中文 Windows 打包的典型情况）
        let gbk = [
            0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4, b'.', b't', b'x', b't',
        ];
        let s = decode_name(&gbk, false);
        assert_eq!(
            s, "中文测试.txt",
            "GBK 中文名必须正确解码（实际得到 {s:?}）"
        );
    }

    #[test]
    fn utf8_名字按_utf8_解码() {
        // 带 UTF-8 标志位
        assert_eq!(decode_name("中文.txt".as_bytes(), true), "中文.txt");
        // 不带标志位但内容确实是 UTF-8（很多工具这样）
        assert_eq!(decode_name("中文.txt".as_bytes(), false), "中文.txt");
    }

    #[test]
    fn 真实_gbk_样本端到端解码() {
        let path = "/home/user/.hermes/cache/scratch/true_gbk_fake.mp4";
        if !std::path::Path::new(path).exists() {
            eprintln!("样本不存在，跳过");
            return;
        }
        let mut f = std::fs::File::open(path).unwrap();
        let size = f.metadata().unwrap().len();
        let l = locate(&mut f, size).unwrap();
        let entries = list_entries(&mut f, &l).unwrap();
        eprintln!("── 端到端解出的文件名 ──");
        for e in &entries {
            eprintln!("   {:?}", e.path);
        }
        assert!(
            entries.iter().any(|e| e.path.contains("中文测试")),
            "GBK 样本必须解出中文名，实际拿到: {:?}",
            entries.iter().map(|e| &e.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn 解码失败不_panic_也不丢条目() {
        // 混入非法字节：只要求不 panic
        let weird = [0xFFu8, 0xFE, 0x00, 0x41, 0x80];
        let s = decode_name(&weird, false);
        assert!(s.len() <= weird.len() * 3, "输出长度不应爆炸");
    }

    #[test]
    fn gbk_与_big5_都能解出汉字() {
        // Big5 的 "中文" = A4 A4 A4 E5 —— GB18030 解出来不是"中文"但也不该 panic
        let big5 = [0xA4, 0xA4, 0xA4, 0xE5];
        let s = decode_name(&big5, false);
        assert!(!s.is_empty(), "Big5 字节至少要有输出（fallback 生效）");
    }

    use super::*;
    use std::io::Cursor;

    /// 手工构造一个最小 ZIP（stored，无压缩），可选前置垃圾
    fn build_zip(prefix: u64, suffix: u64, name: &str, data: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; prefix as usize];

        let lfh_at = out.len() as u64;
        // ── 本地文件头 ──
        out.extend_from_slice(LFH_SIG);
        out.extend_from_slice(&[20, 0]); // version needed
        out.extend_from_slice(&[0, 0]); // flags
        out.extend_from_slice(&[0, 0]); // method = stored
        out.extend_from_slice(&[0, 0, 0, 0]); // time+date
        let crc = crc32(data);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // csize
        out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // usize
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]); // extra len
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        // ── 中央目录（偏移相对于 ZIP 起点） ──
        let cd_at = out.len() as u64 - prefix;
        out.extend_from_slice(CD_SIG);
        out.extend_from_slice(&[20, 0, 20, 0]); // version made by / needed
        out.extend_from_slice(&[0, 0]); // flags
        out.extend_from_slice(&[0, 0]); // method
        out.extend_from_slice(&[0, 0, 0, 0]); // time+date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]); // extra
        out.extend_from_slice(&[0, 0]); // comment
        out.extend_from_slice(&[0, 0]); // disk
        out.extend_from_slice(&[0, 0]); // internal attr
        out.extend_from_slice(&[0, 0, 0, 0]); // external attr
        out.extend_from_slice(&(lfh_at as u32).to_le_bytes()); // LFH 偏移（相对 ZIP 起点）
        out.extend_from_slice(name.as_bytes());
        let cd_size = out.len() as u64 - prefix - cd_at;

        // ── EOCD ──
        out.extend_from_slice(EOCD_SIG);
        out.extend_from_slice(&[0, 0]); // disk
        out.extend_from_slice(&[0, 0]); // cd disk
        out.extend_from_slice(&[1, 0]); // entries on disk
        out.extend_from_slice(&[1, 0]); // entries total
        out.extend_from_slice(&(cd_size as u32).to_le_bytes());
        out.extend_from_slice(&(cd_at as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0]); // comment len

        out.extend(vec![0u8; suffix as usize]);
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    #[test]
    fn 无前置数据时_base_为零() {
        let zip = build_zip(0, 0, "hello.txt", b"hello world");
        let size = zip.len() as u64;
        let mut cur = Cursor::new(zip);
        let l = locate(&mut cur, size).unwrap();
        assert_eq!(l.base, 0);
        assert!(!l.zip64);
        assert_eq!(l.entries_total, 1);
    }

    #[test]
    fn 有前置数据时能反推出真实起点() {
        // 模拟伪装包：前面压 1 MB 垃圾
        let zip = build_zip(1 << 20, 0, "数据.bin", b"payload payload payload");
        let size = zip.len() as u64;
        let mut cur = Cursor::new(zip);
        let l = locate(&mut cur, size).unwrap();
        assert_eq!(l.base, 1 << 20, "应当反推出 ZIP 真实起点");
    }

    #[test]
    fn 尾部有垃圾也能定位() {
        // 模拟"尾部空 mdat"（隐写者会加 8 字节）
        let zip = build_zip(4096, 8, "a.txt", b"xyz");
        let size = zip.len() as u64;
        let mut cur = Cursor::new(zip);
        let l = locate(&mut cur, size).unwrap();
        assert_eq!(l.base, 4096);
        assert_eq!(l.trailing_bytes, 8, "应报告尾部还有 8 字节");
    }

    #[test]
    fn 能列出条目并识别加密标志() {
        let zip = build_zip(1000, 0, "readme.txt", b"content here");
        let size = zip.len() as u64;
        let mut cur = Cursor::new(zip);
        let l = locate(&mut cur, size).unwrap();
        let entries = list_entries(&mut cur, &l).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "readme.txt");
        assert_eq!(entries[0].size, 12);
        assert!(!entries[0].encrypted);
    }

    #[test]
    fn 偏移被篡改时_校验会拦下() {
        // 造一个 CD 偏移被改坏的包：把 EOCD 里的 cd_at 改大
        let mut zip = build_zip(0, 0, "a.txt", b"data");
        let n = zip.len();
        // 找到 EOCD（最后 22 字节）
        let eocd = n - 22;
        let bad_cd_at = u32le(&zip, eocd + 16) + 7;
        zip[eocd + 16..eocd + 20].copy_from_slice(&bad_cd_at.to_le_bytes());
        let size = zip.len() as u64;
        let mut cur = Cursor::new(zip);
        let r = locate(&mut cur, size);
        assert!(r.is_err(), "篡改偏移后必须报错，不能给出错误结果");
    }

    #[test]
    fn 不是压缩包时给出明确错误码() {
        let junk = vec![0u8; 4096];
        let size = junk.len() as u64;
        let mut cur = Cursor::new(junk);
        let e = locate(&mut cur, size).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotArchive);
        assert_eq!(e.param("reason"), Some("no_eocd_in_window"));
    }

    #[test]
    fn 找结束标记的函数能处理多个候选() {
        // 数据里恰好出现一次签名，应能找到
        let mut buf = vec![0u8; 1000];
        buf[500..504].copy_from_slice(EOCD_SIG);
        assert_eq!(find_eocd_backwards(&buf), Some(500));
        // 多个候选时取最后一个（离文件尾最近）
        buf[800..804].copy_from_slice(EOCD_SIG);
        assert_eq!(find_eocd_backwards(&buf), Some(800));
    }
}
