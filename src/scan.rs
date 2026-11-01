//! 全文件签名扫描
//!
//! # 为什么需要它
//! 有时归档数据不在文件尾部（例如 `[视频前半][压缩包][视频后半]`，或一个文件里藏两个包），
//! 靠"尾部窗口找结束标记"就失效了。这时必须能**扫遍整个文件**找签名。
//!
//! # 关键教训（来自真实测试，必须遵守）
//! 1. **不能"见到签名就认定是压缩包"** —— 隐写者会故意撒假特征码坑扫描器，
//!    所以每个命中都要通过**结构校验**才算数。
//! 2. **短签名误报率极高** —— 实测在 8.7 GB 视频数据里，3 字节的 `BZh`/`\x1f\x8b\x08`
//!    会误报上千次。因此短签名必须**附加合理性校验**（见 [`short_sig_plausible`]）。
//! 3. **必须单遍扫描** —— 早期实现是"每个签名各扫一遍"，8.7 GB 要 6 分半；
//!    改成"按前两字节建索引、单遍匹配"后快了一个数量级。

use crate::error::Result;
use crate::layout::ArchiveFormat;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

/// 已知签名表：(签名字节, 格式, 签名相对归档起点的偏移)
///
/// 只收录**足够独特**的签名。3 字节以下的签名会额外做合理性校验。
pub const SIGNATURES: &[(&[u8], ArchiveFormat, u32)] = &[
    (b"PK\x03\x04", ArchiveFormat::Zip, 0),
    (b"7z\xbc\xaf\x27\x1c", ArchiveFormat::SevenZ, 0),
    (b"Rar!\x1a\x07\x01\x00", ArchiveFormat::Rar5, 0),
    (b"Rar!\x1a\x07\x00", ArchiveFormat::Rar4, 0),
    (b"\xfd7zXZ\x00", ArchiveFormat::Xz, 0),
    (b"\x28\xb5\x2f\xfd", ArchiveFormat::Zstd, 0),
    // ── 以下两个只有 3 字节，误报率高，必须配合合理性校验 ──
    (b"\x1f\x8b\x08", ArchiveFormat::Gzip, 0),
    (b"BZh", ArchiveFormat::Bzip2, 0),
];

/// TAR 的 `ustar` 在归档起点 +257 处，单独处理
pub const TAR_SIG: &[u8] = b"ustar";
/// TAR 签名相对归档起点的偏移
pub const TAR_SIG_OFFSET: u64 = 257;

/// **短签名的合理性校验**（大幅降低误报）
///
/// - gzip：第 4 字节 flags 的保留位（0xE0）必须为 0
/// - bzip2：第 4 字节必须是块大小字符 `1`..`9`
pub fn short_sig_plausible(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    match &data[..3] {
        b"\x1f\x8b\x08" => data[3] & 0xE0 == 0,
        b"BZh" => (b'1'..=b'9').contains(&data[3]),
        _ => true,
    }
}

/// 置信度
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// 仅看到签名（可能是假签名 / 巧合）
    Low,
    /// 签名 + 基本合理性检查通过
    Medium,
    /// 通过严格结构校验（可放心用）
    High,
}

/// 一次扫描命中
#[derive(Debug, Clone)]
pub struct ScanHit {
    /// 识别出的格式
    pub format: ArchiveFormat,
    /// **归档数据的起点**（已按签名偏移反推）
    pub offset: u64,
    /// 签名的原始位置
    pub sig_at: u64,
    /// 置信度
    pub confidence: Confidence,
    /// 诊断备注
    pub note: String,
}

/// 扫描参数
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// 最多扫描多少字节（`None` = 全文件）
    pub max_scan_bytes: Option<u64>,
    /// 分块大小
    pub chunk_size: usize,
    /// 重叠窗口（防跨块漏检）
    pub overlap: usize,
    /// 是否做结构校验（强烈建议开启 —— 用来识别假签名）
    pub verify: bool,
    /// 是否搜索"短签名"格式（gzip/bzip2；误报率较高，但仍是常见格式）
    pub search_short_sigs: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_scan_bytes: None,
            chunk_size: 4 << 20, // 4 MiB
            overlap: 64,
            verify: true,
            search_short_sigs: true,
        }
    }
}

impl ScanOptions {
    /// 快速模式：只扫开头一小段（用于"先看看这是啥"）
    pub fn quick() -> Self {
        Self {
            max_scan_bytes: Some(8 << 20),
            ..Default::default()
        }
    }

    /// 严格模式：不找短签名（避免误报，只找高置信度的）
    pub fn strict() -> Self {
        Self {
            search_short_sigs: false,
            ..Default::default()
        }
    }
}

/// 签名匹配索引 —— 单遍扫描的关键
///
/// 两级加速（实测：8.7 GB 扫描从 6 分半降到十几秒量级）：
/// 1. **首字节位图** —— 绝大多数字节不是任何签名的开头，用位图 O(1) 跳过
///    （实测视频数据里只有约 3% 的字节可能是签名首字节）
/// 2. **前两字节哈希索引** —— 剩下的候选再按两字节精确分桶，避免逐个比对
struct SigIndex {
    /// 首字节位图（256 bit，压成 4 个 u64）：某位为 1 表示该字节值可能是某签名的开头
    first_byte_bitmap: [u64; 4],
    /// key = 签名的前两个字节（小端组合），value = SIGNATURES 的下标
    buckets: HashMap<u16, Vec<usize>>,
}

impl SigIndex {
    fn build(search_short: bool) -> Self {
        let mut first_byte_bitmap = [0u64; 4];
        let mut buckets: HashMap<u16, Vec<usize>> = HashMap::new();
        for (i, (sig, _, _)) in SIGNATURES.iter().enumerate() {
            // 短签名（gzip/bzip2）默认也找，但会加强校验
            let is_short = sig.len() <= 3;
            if is_short && !search_short {
                continue;
            }
            if sig.len() < 2 {
                continue;
            }
            let b0 = sig[0];
            first_byte_bitmap[(b0 >> 6) as usize] |= 1u64 << (b0 & 63);
            let key = u16::from_le_bytes([sig[0], sig[1]]);
            buckets.entry(key).or_default().push(i);
        }
        Self {
            first_byte_bitmap,
            buckets,
        }
    }

    /// 该字节是否可能是某个签名的开头（O(1) 位运算）
    #[inline]
    fn may_start(&self, b: u8) -> bool {
        self.first_byte_bitmap[(b >> 6) as usize] & (1u64 << (b & 63)) != 0
    }

    /// 单遍扫描：把命中追加到 `out`（元素为 `(签名在文件中的绝对位置, 签名下标)`）
    fn find_all(&self, haystack: &[u8], base: u64, out: &mut Vec<(u64, usize)>) {
        let n = haystack.len();
        if n < 2 {
            return;
        }
        let mut i = 0usize;
        while i + 2 <= n {
            let b0 = haystack[i];
            // ⚡ 第一级：快速跳过（绝大多数字节在这里就被排除）
            if !self.may_start(b0) {
                i += 1;
                continue;
            }
            // ⚡ 第二级：两字节精确分桶
            let key = u16::from_le_bytes([b0, haystack[i + 1]]);
            if let Some(cands) = self.buckets.get(&key) {
                for &si in cands {
                    let (sig, _, _) = SIGNATURES[si];
                    if haystack[i..].starts_with(sig) {
                        // 短签名再确认一次合理性
                        if sig.len() <= 3 && !short_sig_plausible(&haystack[i..]) {
                            continue;
                        }
                        out.push((base + i as u64, si));
                    }
                }
            }
            i += 1;
        }
    }
}

/// **扫描整个文件**找归档数据
///
/// 返回按偏移排序的命中列表（一个文件里可能有多个包 —— 这是我们的差异化能力）。
///
/// # 为什么不按 7z / libarchive 那样「只从偏移 0 认」（账本 B-1 / B-2）
///
/// 审计对照：7z 对前面垫了垃圾字节的包直接
/// `Cannot open the file as [zip] archive`；我们能在偏移 2,355,192 正确定位
/// （`tests/data/D2b_prefixed_2355200.zip` 实测：7z 打不开、我们能列能解）。
///
/// 现实里用户手上的包常常是「下载器塞了几 MB 广告头」「网页把 zip 拼在图片后面」
/// 「BT 下到一半混进了别的数据」，只认偏移 0 等于把这些全判成坏包。
///
/// **但这不等于「见到签名就认」** —— 隐写者会故意撒假特征码把扫描器拖垮
/// （见本模块顶部三条铁律）。这里的放行是「签名命中 → 结构校验（EOCD / 中央目录）通过」
/// 两步，校验不过照样拒收（审计 A-4 就是修这件事）。
///
/// ⚠️ 别为了「和 7z 行为一致」把这段简化掉：上面那个样本会当场变红。
pub fn scan_whole_file<R: Read + Seek>(
    r: &mut R,
    file_size: u64,
    opts: &ScanOptions,
) -> Result<Vec<ScanHit>> {
    scan_whole_file_with_progress(r, file_size, opts, |_, _| {})
}

/// 带进度的全文件扫描（D1：8 GB 包识别时界面不能只是转圈）
///
/// 回调参数是 `(已扫字节, 总字节)`，每块一次；调用方自己决定怎么节流/上报。
pub fn scan_whole_file_with_progress<R: Read + Seek, F>(
    r: &mut R,
    file_size: u64,
    opts: &ScanOptions,
    mut on_progress: F,
) -> Result<Vec<ScanHit>>
where
    F: FnMut(u64, u64),
{
    let limit = match opts.max_scan_bytes {
        Some(n) => n.min(file_size),
        None => file_size,
    };

    let chunk = opts.chunk_size.max(64);
    let overlap = opts.overlap.min(chunk / 2);
    let index = SigIndex::build(opts.search_short_sigs);

    let mut hits: Vec<ScanHit> = Vec::new();
    let mut read_pos: u64 = 0;
    let mut carry: Vec<u8> = Vec::new();

    while read_pos < limit {
        let want = ((limit - read_pos) as usize).min(chunk);
        r.seek(SeekFrom::Start(read_pos))?;
        let mut buf = vec![0u8; want];
        let n = read_up_to(r, &mut buf)?;
        if n == 0 {
            break;
        }
        buf.truncate(n);

        let carry_len = carry.len();
        let mut window = Vec::with_capacity(carry_len + buf.len());
        window.extend_from_slice(&carry);
        window.extend_from_slice(&buf);
        let base = read_pos - carry_len as u64;

        // ── 单遍匹配所有签名 ──
        let mut raw: Vec<(u64, usize)> = Vec::new();
        index.find_all(&window, base, &mut raw);

        // TAR 的签名在 +257，单独找（用同样的一次遍历）
        {
            let mut i = 0usize;
            while i + TAR_SIG.len() <= window.len() {
                match window[i..]
                    .windows(TAR_SIG.len())
                    .position(|w| w == TAR_SIG)
                {
                    Some(p) => {
                        let abs = base + (i + p) as u64;
                        if abs + TAR_SIG.len() as u64 > read_pos || read_pos == 0 {
                            record_tar(
                                &mut hits,
                                abs.saturating_sub(TAR_SIG_OFFSET),
                                abs,
                                r,
                                file_size,
                                opts,
                            )?;
                        }
                        i += p + 1;
                    }
                    None => break,
                }
            }
        }

        for (abs, si) in raw {
            let (sig, fmt, sig_off) = SIGNATURES[si];
            // 该命中若完全落在上一块里，说明上一轮已处理过
            if read_pos > 0 && abs + sig.len() as u64 <= read_pos {
                continue;
            }
            let offset = abs.saturating_sub(sig_off as u64);
            let (confidence, note) = if opts.verify {
                verify_at(r, fmt, offset, file_size)?
            } else {
                (Confidence::Low, "未做结构校验".to_string())
            };
            hits.push(ScanHit {
                format: fmt,
                offset,
                sig_at: abs,
                confidence,
                note,
            });
        }

        carry = if n >= overlap {
            buf[n - overlap..].to_vec()
        } else {
            buf.clone()
        };
        read_pos += n as u64;
        // 每块报一次进度（分块通常 1 MiB，8 GB 约 8000 次；外壳按时间节流）
        on_progress(read_pos.min(limit), limit);
        if n < want {
            break;
        }
    }

    hits.sort_by_key(|h| (h.offset, h.format.as_str()));
    hits.dedup_by(|a, b| a.offset == b.offset && a.format == b.format);
    Ok(hits)
}

fn record_tar<R: Read + Seek>(
    hits: &mut Vec<ScanHit>,
    offset: u64,
    sig_at: u64,
    r: &mut R,
    file_size: u64,
    opts: &ScanOptions,
) -> Result<()> {
    let (confidence, note) = if opts.verify {
        verify_tar_header(r, offset, file_size)?
    } else {
        (Confidence::Low, "未做结构校验".to_string())
    };
    hits.push(ScanHit {
        format: ArchiveFormat::Tar,
        offset,
        sig_at,
        confidence,
        note,
    });
    Ok(())
}

/// **结构校验**：判断"这个签名处是不是真的有归档"
fn verify_at<R: Read + Seek>(
    r: &mut R,
    fmt: ArchiveFormat,
    offset: u64,
    file_size: u64,
) -> Result<(Confidence, String)> {
    match fmt {
        ArchiveFormat::Zip => verify_zip_lfh(r, offset, file_size),
        ArchiveFormat::SevenZ => verify_7z_header(r, offset, file_size),
        ArchiveFormat::Rar4 | ArchiveFormat::Rar5 => verify_rar_header(r, offset, file_size, fmt),
        ArchiveFormat::Tar => verify_tar_header(r, offset, file_size),
        ArchiveFormat::Gzip => {
            if offset + 10 > file_size {
                return Ok((Confidence::Low, "gzip 头超出文件末尾".into()));
            }
            r.seek(SeekFrom::Start(offset + 3))?;
            let mut b = [0u8; 1];
            read_up_to(r, &mut b)?;
            if b[0] & 0xE0 != 0 {
                return Ok((Confidence::Low, "gzip flags 保留位非零，疑似误报".into()));
            }
            Ok((Confidence::Medium, "gzip 头基本合理".into()))
        }
        ArchiveFormat::Bzip2 => {
            if offset + 4 > file_size {
                return Ok((Confidence::Low, "bzip2 头超出文件末尾".into()));
            }
            r.seek(SeekFrom::Start(offset + 3))?;
            let mut b = [0u8; 1];
            read_up_to(r, &mut b)?;
            if !(b'1'..=b'9').contains(&b[0]) {
                return Ok((Confidence::Low, "bzip2 块大小字符不合法，疑似误报".into()));
            }
            Ok((Confidence::Medium, "bzip2 头基本合理".into()))
        }
        _ => Ok((Confidence::Medium, format!("见到 {} 签名", fmt.as_str()))),
    }
}

fn verify_zip_lfh<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    file_size: u64,
) -> Result<(Confidence, String)> {
    if offset + 30 > file_size {
        return Ok((Confidence::Low, "本地文件头超出文件末尾".into()));
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut hdr = [0u8; 30];
    if read_up_to(r, &mut hdr)? < 30 {
        return Ok((Confidence::Low, "读不到完整的本地文件头".into()));
    }
    if &hdr[0..4] != b"PK\x03\x04" {
        return Ok((Confidence::Low, "签名不符".into()));
    }
    let method = u16::from_le_bytes([hdr[8], hdr[9]]);
    let name_len = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
    let extra_len = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;
    let csize = u32::from_le_bytes([hdr[18], hdr[19], hdr[20], hdr[21]]) as u64;
    let flags = u16::from_le_bytes([hdr[6], hdr[7]]);

    if method > 20 && method != 99 {
        return Ok((Confidence::Low, format!("压缩方法 {method} 不合理")));
    }
    if name_len == 0 || name_len > 4096 {
        return Ok((Confidence::Low, format!("文件名长度 {name_len} 不合理")));
    }
    let data_start = offset + 30 + name_len + extra_len;
    if data_start > file_size {
        return Ok((Confidence::Low, "数据起点越界".into()));
    }
    let streaming = flags & 0x0008 != 0;
    if !streaming && data_start + csize > file_size {
        return Ok((Confidence::Low, "压缩数据越界".into()));
    }
    Ok((
        Confidence::High,
        format!("合法本地文件头（方法 {method}，名字 {name_len} 字节）"),
    ))
}

fn verify_7z_header<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    file_size: u64,
) -> Result<(Confidence, String)> {
    if offset + 32 > file_size {
        return Ok((Confidence::Low, "7z 头超出文件末尾".into()));
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut hdr = [0u8; 32];
    if read_up_to(r, &mut hdr)? < 32 {
        return Ok((Confidence::Low, "读不到完整的 7z 头".into()));
    }
    if &hdr[0..6] != b"7z\xbc\xaf\x27\x1c" {
        return Ok((Confidence::Low, "7z 签名不符".into()));
    }
    let expect = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let actual = crc32(&hdr[12..32]);
    if expect != actual {
        return Ok((
            Confidence::Low,
            format!("7z 头 CRC 不符（期望 {expect:08x}，实为 {actual:08x}）—— 疑似假签名"),
        ));
    }
    let nh_off = u64::from_le_bytes(hdr[12..20].try_into().unwrap());
    let nh_size = u64::from_le_bytes(hdr[20..28].try_into().unwrap());
    let total = 32u64.saturating_add(nh_off).saturating_add(nh_size);
    Ok((
        Confidence::High,
        format!("7z 头 CRC 通过，推算总长 {total} 字节"),
    ))
}

fn verify_rar_header<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    file_size: u64,
    fmt: ArchiveFormat,
) -> Result<(Confidence, String)> {
    let sig_len = if fmt == ArchiveFormat::Rar5 { 8 } else { 7 };
    if offset + sig_len as u64 + 5 > file_size {
        return Ok((Confidence::Low, "RAR 头超出文件末尾".into()));
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; 16];
    let n = read_up_to(r, &mut buf)?;
    if n < sig_len + 2 {
        return Ok((Confidence::Low, "读不到完整 RAR 头".into()));
    }
    let expected: &[u8] = if fmt == ArchiveFormat::Rar5 {
        b"Rar!\x1a\x07\x01\x00"
    } else {
        b"Rar!\x1a\x07\x00"
    };
    if buf[..sig_len] != expected[..sig_len] {
        return Ok((Confidence::Low, "RAR 签名不符".into()));
    }
    Ok((Confidence::Medium, "RAR 签名匹配（未做 CRC 深校验）".into()))
}

fn verify_tar_header<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    file_size: u64,
) -> Result<(Confidence, String)> {
    if offset + 512 > file_size {
        return Ok((Confidence::Low, "TAR 头超出文件末尾".into()));
    }
    r.seek(SeekFrom::Start(offset + 148))?;
    let mut buf = [0u8; 8];
    if read_up_to(r, &mut buf)? < 8 {
        return Ok((Confidence::Low, "读不到 TAR 校验和字段".into()));
    }
    let s = String::from_utf8_lossy(&buf);
    let txt = s.trim_matches(|c: char| c == '\0' || c == ' ');
    if txt.is_empty() || u32::from_str_radix(txt, 8).is_err() {
        return Ok((Confidence::Low, "TAR 校验和字段不是合法八进制".into()));
    }
    Ok((Confidence::Medium, "TAR 头基本合法".into()))
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(crate::error::Error::new(crate::error::ErrorCode::Internal)
                    .context(format!("read failed: {e}")));
            }
        }
    }
    Ok(total)
}

/// CRC-32（IEEE，与 ZIP/7z 一致）
pub fn crc32(data: &[u8]) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_lfh(name: &str, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"PK\x03\x04");
        v.extend_from_slice(&[20, 0]);
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(&[0, 0]); // method = store
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&crc32(data).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn 能找到文件中间的_zip() {
        let mut buf = vec![0xAAu8; 1000];
        buf.extend_from_slice(&make_lfh("a.txt", b"hello"));
        buf.extend(std::iter::repeat_n(0xBBu8, 500));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let z = hits
            .iter()
            .find(|h| h.format == ArchiveFormat::Zip)
            .unwrap();
        assert_eq!(z.offset, 1000);
        assert_eq!(z.confidence, Confidence::High);
    }

    #[test]
    fn 假签名会被降级() {
        let mut buf = vec![0u8; 200];
        buf.extend_from_slice(b"PK\x03\x04");
        buf.extend_from_slice(&[0xFF; 100]);
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let z: Vec<_> = hits
            .iter()
            .filter(|h| h.format == ArchiveFormat::Zip)
            .collect();
        assert!(!z.is_empty());
        assert!(z.iter().all(|h| h.confidence == Confidence::Low));
    }

    #[test]
    fn 短签名误报被大幅降低() {
        // 随机数据里塞满 "BZ" 和 "\x1f\x8b\x08" 但不合法
        let mut buf = Vec::new();
        for _ in 0..2000 {
            buf.extend_from_slice(b"BZx"); // "BZh" 的 h 换成 x → 不合法
            buf.extend_from_slice(b"\x1f\x8b\x08\xff"); // flags 保留位非零 → 不合法
        }
        buf.extend(std::iter::repeat_n(0u8, 1000));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        assert!(
            hits.iter().all(|h| h.confidence == Confidence::Low),
            "不合法的短签名必须都是低置信度，实际: {:?}",
            hits.iter()
                .map(|h| (h.format.as_str(), h.confidence))
                .take(5)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn 合法的gzip_bzip2能被识别为中等置信度() {
        let mut buf = vec![0u8; 500];
        buf.extend_from_slice(b"\x1f\x8b\x08\x00"); // 合法 gzip 头（flags=0）
        buf.extend(std::iter::repeat_n(0u8, 200));
        buf.extend_from_slice(b"BZh9"); // 合法 bzip2 头
        buf.extend(std::iter::repeat_n(0u8, 200));

        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let gz = hits
            .iter()
            .find(|h| h.format == ArchiveFormat::Gzip)
            .unwrap();
        let bz = hits
            .iter()
            .find(|h| h.format == ArchiveFormat::Bzip2)
            .unwrap();
        assert_eq!(gz.confidence, Confidence::Medium);
        assert_eq!(bz.confidence, Confidence::Medium);
    }

    #[test]
    fn 严格模式不找短签名() {
        let mut buf = vec![0u8; 100];
        buf.extend_from_slice(b"\x1f\x8b\x08\x00");
        buf.extend(std::iter::repeat_n(0u8, 200));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::strict()).unwrap();
        assert!(
            !hits.iter().any(|h| h.format == ArchiveFormat::Gzip),
            "严格模式应跳过短签名"
        );
    }

    #[test]
    fn 校验_7z头_crc_不符会被降级() {
        let mut buf = vec![0u8; 64];
        buf.extend_from_slice(b"7z\xbc\xaf\x27\x1c");
        buf.extend_from_slice(&[0, 4]);
        buf.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 20]);
        buf.extend(std::iter::repeat_n(0u8, 200));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let h = hits
            .iter()
            .find(|h| h.format == ArchiveFormat::SevenZ)
            .unwrap();
        assert_eq!(h.confidence, Confidence::Low);
        assert!(h.note.contains("CRC"));
    }

    #[test]
    fn 校验_7z头_crc_正确则高置信度() {
        let mut hdr = vec![0u8; 32];
        hdr[0..6].copy_from_slice(b"7z\xbc\xaf\x27\x1c");
        hdr[6..8].copy_from_slice(&[0, 4]);
        let c = crc32(&hdr[12..32]);
        hdr[8..12].copy_from_slice(&c.to_le_bytes());
        let mut buf = vec![0xEEu8; 300];
        buf.extend_from_slice(&hdr);
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let h = hits
            .iter()
            .find(|h| h.format == ArchiveFormat::SevenZ)
            .unwrap();
        assert_eq!(h.confidence, Confidence::High);
        assert_eq!(h.offset, 300);
    }

    #[test]
    fn 跨块边界的签名也能找到() {
        let chunk = 1024usize;
        let lfh = make_lfh("boundary.txt", b"xyz");
        let mut buf = vec![0u8; chunk - 2];
        buf.extend_from_slice(&lfh);
        buf.extend(std::iter::repeat_n(0u8, 2048));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let opts = ScanOptions {
            chunk_size: chunk,
            overlap: 64,
            ..Default::default()
        };
        let hits = scan_whole_file(&mut cur, size, &opts).unwrap();
        assert!(
            hits.iter()
                .any(|h| h.format == ArchiveFormat::Zip && h.offset == (chunk - 2) as u64)
        );
    }

    #[test]
    fn 一个文件里两个包都能找到() {
        let mut buf = vec![0x11u8; 100];
        buf.extend_from_slice(&make_lfh("first.txt", b"aaa"));
        buf.extend(std::iter::repeat_n(0x22u8, 200));
        buf.extend_from_slice(&make_lfh("second.txt", b"bbb"));
        buf.extend(std::iter::repeat_n(0x33u8, 100));
        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let hits = scan_whole_file(&mut cur, size, &ScanOptions::default()).unwrap();
        let zips: Vec<_> = hits
            .iter()
            .filter(|h| h.format == ArchiveFormat::Zip)
            .collect();
        assert_eq!(zips.len(), 2);
        assert!(zips[0].offset < zips[1].offset);
    }

    #[test]
    fn 位图跳过逻辑正确() {
        let idx = SigIndex::build(true);
        // 签名的首字节一定被判为"可能"
        for (sig, _, _) in SIGNATURES {
            assert!(idx.may_start(sig[0]), "首字节 {:02x} 应被判为可能", sig[0]);
        }
        // 明显无关的字节应被排除（0x00/0xFF 不是任何签名开头）
        assert!(!idx.may_start(0x00));
        assert!(!idx.may_start(0xFF));
    }

    #[test]
    fn 快速模式限制扫描范围() {
        assert_eq!(ScanOptions::quick().max_scan_bytes, Some(8 << 20));
    }
}
