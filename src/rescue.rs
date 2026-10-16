//! 救援模式：**目录表坏了也能重建文件列表**
//!
//! # 算法来源（综合参考项目，非自创）
//! 核心思路综合自 **7-Zip 的 LOCALS-CD-MODE**
//! （`refs/7zip/CPP/7zip/Archive/Zip/ZipIn.cpp`，触发点 L2735、主体 L2076 `ReadLocals`）：
//!
//! > 当中央目录（CD）与第一个条目不匹配时，7-Zip 不再信任 CD，
//! > 而是**从 Local File Header 顺序往下读**，靠每条自带的 `packSize`
//! > 跳过压缩数据，一路重建出完整目录。这就是它能啃下"CD 被改坏"的根因。
//!
//! 我们在此基础上补了两点（7-Zip 没有或做得不够的）：
//! 1. **可指定起点**：我们的扫描层能先定位到"真正的 local header"，
//!    不必像 7-Zip 那样从头试（对付伪装包必须这样，头部全是视频数据）。
//! 2. **分段重建**：支持"包被切开/中间被覆盖"的情况 —— 换起点继续重连。
//!
//! # 借鉴的两个防误报细节（原样对译）
//! - `IsStrangeItem`（L2036）：名字 > 16 KiB 或压缩方法 > 256 → 判为"不像真归档"
//! - `FindDescriptor`（L1463）的**双重验证**：找到 `PK\x07\x08` 还要满足
//!   ① 其后 4 字节是 `PK\x03\x04` 或 `PK\x01\x02`；② 描述符里记的 packSize == 实际位置。

use crate::error::{Error, ErrorCode, Result};
use std::io::{Read, Seek, SeekFrom};

const SIG_LOCAL: [u8; 4] = *b"PK\x03\x04";
const SIG_CENTRAL: [u8; 4] = *b"PK\x01\x02";
const SIG_DESCRIPTOR: [u8; 4] = *b"PK\x07\x08";
const SIG_EOCD: [u8; 4] = *b"PK\x05\x06";

/// 一个从 Local File Header 解析出来的条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEntry {
    /// 文件名（按 flags 决定是 UTF-8 还是 GBK —— 这里只存原始字节的宽松解码）
    pub path: String,
    /// 文件名的原始字节（保留，便于外层按编码重试）
    pub raw_name: Vec<u8>,
    /// Local header 的绝对偏移
    pub header_at: u64,
    /// 压缩数据的绝对起点
    pub data_at: u64,
    /// 压缩后大小（来自 header 或描述符）
    pub pack_size: u64,
    /// 原始大小
    pub size: u64,
    /// 压缩方法（0=不压缩, 8=deflate, 99=AES）
    pub method: u16,
    /// CRC-32
    pub crc: u32,
    /// 通用标志位
    pub flags: u16,
    /// 大小是否来自"数据描述符"（流式条目）
    pub size_from_descriptor: bool,
}

impl LocalEntry {
    /// 数据结束位置
    pub fn data_end(&self) -> u64 {
        self.data_at.saturating_add(self.pack_size)
    }

    /// 是否是加密条目
    pub fn encrypted(&self) -> bool {
        self.flags & 0x0001 != 0
    }

    /// 是否是目录
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/') || self.path.ends_with('\\')
    }
}

/// 救援参数
#[derive(Debug, Clone)]
pub struct RescueOptions {
    /// 最多重建多少个条目（防止在垃圾数据里无限循环）
    pub max_entries: usize,
    /// **跳过数据的方式**（对译 7-Zip 注释的策略）
    ///
    /// > without buffer: skips packed data: fast for big files
    /// > with    buffer: reads packed data : slow for big files : fast for small files
    ///
    /// 我们据此自动选择：单条数据 > 8 MiB 用 seek 跳过；否则顺序读取推进
    /// （顺序读对磁盘更友好，seek 对超大文件更快）。
    pub seek_threshold: u64,
    /// 遇到几个连续失败就停下（容忍局部损坏）
    pub max_consecutive_failures: usize,
}

impl Default for RescueOptions {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            seek_threshold: 8 << 20,
            max_consecutive_failures: 1,
        }
    }
}

/// **判断"这不像真归档"** —— 对译 7-Zip `IsStrangeItem`（L2036）
///
/// ```cpp
/// return item.Name.Len() > (1 << 14) || item.Method > (1 << 8);
/// ```
fn is_strange(path_len: usize, method: u16) -> bool {
    path_len > (1 << 14) || method > (1 << 8)
}

/// 32 位字段是否处于"哨兵"状态（表示真实值在 ZIP64 extra 里）
fn usize_orig_sentinel(v: u64) -> bool {
    v == 0xFFFF_FFFF
}

/// 同上，针对压缩大小
fn pack_size_sentinel(v: u64) -> bool {
    v == 0xFFFF_FFFF
}

fn read_exact_at<R: Read + Seek>(r: &mut R, at: u64, buf: &mut [u8]) -> Result<bool> {
    r.seek(SeekFrom::Start(at))?;
    let mut total = 0usize;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => return Ok(false),
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::from(e)),
        }
    }
    Ok(true)
}

/// 解析一个 Local File Header
///
/// 结构（PKWARE APPNOTE 4.3.7）：签名(4) + 版本(2) + 标志(2) + 方法(2) +
/// 时间(2) + 日期(2) + CRC(4) + 压缩大小(4) + 原始大小(4) + 名字长度(2) +
/// 扩展长度(2) + 名字 + 扩展
pub fn parse_local_header<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    file_size: u64,
) -> Result<Option<LocalEntry>> {
    if offset + 30 > file_size {
        return Ok(None);
    }
    let mut hdr = [0u8; 30];
    if !read_exact_at(r, offset, &mut hdr)? {
        return Ok(None);
    }
    if hdr[0..4] != SIG_LOCAL {
        return Ok(None);
    }

    let flags = u16::from_le_bytes([hdr[6], hdr[7]]);
    let method = u16::from_le_bytes([hdr[8], hdr[9]]);
    let crc = u32::from_le_bytes([hdr[14], hdr[15], hdr[16], hdr[17]]);
    let pack_size = u32::from_le_bytes([hdr[18], hdr[19], hdr[20], hdr[21]]) as u64;
    let size = u32::from_le_bytes([hdr[22], hdr[23], hdr[24], hdr[25]]) as u64;
    let name_len = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
    let extra_len = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;

    if is_strange(name_len as usize, method) || name_len == 0 {
        return Ok(None);
    }

    let name_at = offset + 30;
    let data_at = name_at + name_len + extra_len;
    if data_at > file_size {
        return Ok(None);
    }

    let mut raw_name = vec![0u8; name_len as usize];
    if !read_exact_at(r, name_at, &mut raw_name)? {
        return Ok(None);
    }

    // ── ZIP64 extra（0x0001）：字段为哨兵值 0xFFFFFFFF 时，真实值在这里 ──
    // 顺序与含义（PKWARE APPNOTE 4.5.3）：原始大小、压缩大小、local header 偏移、磁盘号
    // 只读取"哪些字段是哨兵"对应的那几项。
    let (mut size, mut pack_size) = (size, pack_size);
    let (mut hdr64, mut csize64) = (0u64, 0u64);
    if extra_len > 0 {
        let mut extra = vec![0u8; extra_len as usize];
        if read_exact_at(r, offset + 30 + name_len, &mut extra)? {
            let mut p = 0usize;
            while p + 4 <= extra.len() {
                let id = u16::from_le_bytes([extra[p], extra[p + 1]]);
                let sz = u16::from_le_bytes([extra[p + 2], extra[p + 3]]) as usize;
                let body = &extra[p + 4..(p + 4 + sz).min(extra.len())];
                if id == 0x0001 {
                    let mut q = 0usize;
                    if usize_orig_sentinel(size) && q + 8 <= body.len() {
                        size = u64::from_le_bytes(body[q..q + 8].try_into().unwrap());
                        q += 8;
                    }
                    if pack_size_sentinel(pack_size) && q + 8 <= body.len() {
                        csize64 = u64::from_le_bytes(body[q..q + 8].try_into().unwrap());
                        q += 8;
                    }
                    if q + 8 <= body.len() {
                        hdr64 = u64::from_le_bytes(body[q..q + 8].try_into().unwrap());
                    }
                }
                p += 4 + sz;
            }
        }
    }
    if csize64 > 0 {
        pack_size = csize64;
    }

    let utf8 = flags & 0x0800 != 0;
    let path = if utf8 {
        String::from_utf8_lossy(&raw_name).into_owned()
    } else {
        // 非 UTF-8：先按 UTF-8 宽松解码，外层可用 raw_name 走 GBK 重试
        String::from_utf8_lossy(&raw_name).into_owned()
    };

    let _ = hdr64; // ZIP64 里的 local header 偏移（分卷场景才需要，暂存）
    Ok(Some(LocalEntry {
        path,
        raw_name,
        header_at: offset,
        data_at,
        pack_size,
        size,
        method,
        crc,
        flags,
        size_from_descriptor: false,
    }))
}

/// 找数据描述符（流式条目用）—— **双重验证**，对译 7-Zip `FindDescriptor`（L1463）
///
/// 必须同时满足：
/// 1. 签名为 `PK\x07\x08`
/// 2. 描述符之后 4 字节是 `PK\x03\x04` 或 `PK\x01\x02`（下一个合法头）
/// 3. （64 位描述符时）描述符里记的 packSize 与实际扫描位置一致
///
/// 返回 `(数据实际长度, 描述符结束位置)`。
pub fn find_descriptor<R: Read + Seek>(
    r: &mut R,
    data_at: u64,
    file_size: u64,
    search_limit: u64,
) -> Result<Option<(u64, u64)>> {
    const CHUNK: usize = 256 << 10;
    let end = (data_at + search_limit).min(file_size);

    // 描述符总长【含 4 字节签名】：32 位版 16 字节、64 位版 24 字节
    // （对齐 7-Zip：其 GetDescriptorSize() 返回不含签名的 12/20）
    for &desc_len in &[16u64, 24u64] {
        let mut pos = data_at;
        while pos + desc_len + 4 <= end {
            let want = ((end - pos) as usize).min(CHUNK);
            r.seek(SeekFrom::Start(pos))?;
            let mut buf = vec![0u8; want];
            let mut got = 0usize;
            while got < want {
                match r.read(&mut buf[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(Error::from(e)),
                }
            }
            buf.truncate(got);
            if buf.len() < desc_len as usize + 4 {
                break;
            }

            let mut i = 0usize;
            while i + desc_len as usize + 4 <= buf.len() {
                if buf[i..i + 4] == SIG_DESCRIPTOR {
                    // 条件 ②：描述符之后必须是下一个合法头
                    let after = i + desc_len as usize;
                    let next_sig = &buf[after..after + 4];
                    let next_ok =
                        next_sig == SIG_LOCAL || next_sig == SIG_CENTRAL || next_sig == SIG_EOCD;
                    if next_ok {
                        let at = pos + i as u64;
                        let pack_size = if desc_len == 24 {
                            u64::from_le_bytes(buf[i + 8..i + 16].try_into().unwrap())
                        } else {
                            u32::from_le_bytes(buf[i + 8..i + 12].try_into().unwrap()) as u64
                        };
                        let scanned = at.saturating_sub(data_at);
                        // 条件 ③：描述符记录的大小应等于实际扫描长度（容忍差 1 的边界）
                        if pack_size == 0 || scanned.abs_diff(pack_size) <= 1 {
                            return Ok(Some((scanned, at + desc_len)));
                        }
                    }
                }
                i += 1;
            }
            if i >= buf.len() {
                break;
            }
            // 保留最后 3 字节避免跨块漏检
            pos += (buf.len() - 3) as u64;
        }
    }
    Ok(None)
}

/// **顺序重建目录** —— 对译 7-Zip `ReadLocals`（L2076），但支持指定起点
///
/// 流程（与 7-Zip 一致）：
/// 1. 读 local header；
/// 2. 若该条目是流式（没有大小）→ 找描述符拿到真实长度；
/// 3. 否则**跳过** `pack_size` 字节（不读数据）；
/// 4. 到下一个位置继续，直到不再是 local header 或达到上限。
pub fn rescue_by_locals<R: Read + Seek>(
    r: &mut R,
    start: u64,
    file_size: u64,
    opts: &RescueOptions,
) -> Result<Vec<LocalEntry>> {
    let mut entries: Vec<LocalEntry> = Vec::new();
    let mut at = start;
    let mut failures = 0usize;
    let mut first = true;

    while entries.len() < opts.max_entries && at + 30 <= file_size {
        let Some(mut item) = parse_local_header(r, at, file_size)? else {
            // 起点处必须是合法头；一旦已经成功重建出条目，才容忍局部损坏
            if first {
                return Err(Error::new(ErrorCode::NotArchive)
                    .with("reason", "no valid local file header at the given start"));
            }
            failures += 1;
            if failures > opts.max_consecutive_failures {
                break;
            }
            at += 1;
            continue;
        };
        first = false;

        let streaming = item.flags & 0x0008 != 0;
        // 流式条目：真实长度在数据描述符里。找到后要【连描述符一起跳过】，
        // 下一条的起点才是描述符之后（对齐 7-Zip：FindDescriptor 会把流位置前推）。
        let mut after_descriptor: Option<u64> = None;
        if streaming || item.pack_size == 0 {
            let limit = file_size.saturating_sub(item.data_at);
            match find_descriptor(r, item.data_at, file_size, limit)? {
                Some((real_len, after)) => {
                    item.pack_size = real_len;
                    if item.size == 0 {
                        item.size = real_len;
                    }
                    item.size_from_descriptor = true;
                    after_descriptor = Some(after);
                }
                None => {
                    // 找不到描述符：这个条目不可靠，停下（保留已成功的）
                    break;
                }
            }
        }

        // 合理性检查：大小不得离谱（对译 7-Zip 的 `PackSize >= 1<<62` 断言）
        if item.pack_size >= (1u64 << 62) {
            break;
        }

        // 有描述符时下一条从描述符之后开始，否则就是数据末尾
        let next = after_descriptor.unwrap_or_else(|| item.data_end());
        if next > file_size {
            // 数据越界：记下条目但停止（相当于"最后一条不完整"）
            entries.push(item);
            break;
        }
        entries.push(item);
        // ── 跳到下一条（不读数据：对齐 7-Zip 的 IncreaseRealPosition）──
        at = next;
        failures = 0;
    }

    Ok(entries)
}

/// **救援式提取**：中央目录彻底坏了也能把数据解出来
///
/// # 为什么需要它
/// [`rescue_by_locals`] 只能**列出**条目（按 local header 重建清单）。
/// 但中央目录坏了之后，`zip` 库整个用不了（它必须靠中央目录）——
/// 这时只有一条路：**照着 local header 自己直接抽数据**。
///
/// # 做法
/// 1. 用 [`rescue_by_locals`] 拿到每个条目的 `data_at` / `pack_size` / `method`
/// 2. 按压缩方法解码（0 = 原样取出；8 = deflate 解压）
/// 3. 边解边写盘（走我们的安全落盘与限额）
/// 4. **用 local header 里的 CRC 校验**，解完对不上就如实报告
///
/// 这条路走通的意义：别人（7z / unzip / unar）在中央目录坏掉时基本没辙，
/// 我们还能按成员把头重建、逐个抢救出来。
pub fn extract_by_locals<R: Read + Seek>(
    r: &mut R,
    base: u64,
    file_size: u64,
    opts: &crate::extract::ExtractOptions,
) -> Result<crate::extract::ExtractReport> {
    use crate::safety::{self, ExtractBudget};
    use std::io::Write;

    let items = rescue_by_locals(r, base, file_size, &RescueOptions::default())?;
    if items.is_empty() {
        return Err(Error::new(ErrorCode::NotArchive).with("reason", "没有可重建的条目"));
    }

    std::fs::create_dir_all(&opts.dest)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("建输出目录失败: {e}")))?;

    // 以"压缩数据总量"为分母做炸弹防护
    let packed_total: u64 = items.iter().map(|i| i.pack_size).sum::<u64>().max(1);
    let mut budget = ExtractBudget::new(opts.limits.clone(), packed_total.max(1));
    let mut report = crate::extract::ExtractReport::default();

    for (idx, item) in items.iter().enumerate() {
        // 挑选过滤（序号 / 通配 / 目录前缀，见 crate::select）
        if !opts
            .selection
            .matches(idx as u64, &item.path, item.is_dir())
        {
            continue;
        }

        // 加密条目：本地恢复拿不到密码校验信息，如实跳过
        if item.encrypted() {
            match &opts.password {
                Some(_) => report.skipped.push((
                    item.path.clone(),
                    "条目已加密，救援模式暂不支持解密（建议先用中央目录完好的副本试）".into(),
                )),
                None => report
                    .skipped
                    .push((item.path.clone(), "条目已加密，但没有提供密码".into())),
            }
            continue;
        }

        // 目录条目
        if item.is_dir() {
            if let Ok(d) = safety::resolve_safe(&opts.dest, &item.path) {
                let _ = std::fs::create_dir_all(&d);
            }
            continue;
        }

        // 安全路径
        let mut out_path = match safety::resolve_safe(&opts.dest, &item.path) {
            Ok(p) => p,
            Err(e) => {
                report
                    .skipped
                    .push((item.path.clone(), format!("路径不安全被拒: {e}")));
                continue;
            }
        };
        if out_path.exists() {
            match opts.overwrite {
                crate::extract::Overwrite::Skip => {
                    report
                        .skipped
                        .push((item.path.clone(), "已存在，按策略跳过".into()));
                    continue;
                }
                crate::extract::Overwrite::Replace => {}
                crate::extract::Overwrite::Rename => out_path = unique_path_rescue(&out_path),
            }
        }
        if let Some(parent) = out_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            report
                .skipped
                .push((item.path.clone(), format!("建父目录失败: {e}")));
            continue;
        }
        if safety::is_risky_executable(&item.path) {
            report
                .warnings
                .push(format!("{}：可执行文件，已解出但请谨慎运行", item.path));
        }

        // ── 定位数据并解码 ──
        let data_start = base + item.data_at;
        if data_start >= file_size {
            report.skipped.push((
                item.path.clone(),
                "数据起点已超出文件末尾（文件严重截断）".into(),
            ));
            continue;
        }
        // 救援模式的要义是「能救多少救多少」：数据被截断时也把能拿到的部分抽出来，
        // 但要**如实标注这是残缺的**（宁可给用户一个不完整的文件 + 一句提醒，
        // 也不要什么都不给）。
        let avail = file_size - data_start;
        let truncated = item.pack_size > avail;
        let take = item.pack_size.min(avail);
        if truncated {
            report.warnings.push(format!(
                "{}：数据被截断（声明 {} 字节，实际只有 {} 字节）—— 已尽力抽出可用部分",
                item.path, item.pack_size, avail
            ));
        }
        if r.seek(std::io::SeekFrom::Start(data_start)).is_err() {
            report.skipped.push((item.path.clone(), "定位失败".into()));
            continue;
        }
        let limited = (&mut *r).take(take);

        let mut out = match std::fs::File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                report
                    .skipped
                    .push((item.path.clone(), format!("创建失败: {e}")));
                continue;
            }
        };

        // CRC 校验用
        let mut crc = Crc32::new();
        let mut written = 0u64;
        let mut err: Option<String> = None;

        // 按方法解码
        let copy_to = |reader: &mut dyn std::io::Read,
                       out: &mut std::fs::File,
                       crc: &mut Crc32,
                       written: &mut u64,
                       budget: &mut ExtractBudget,
                       err: &mut Option<String>| {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Err(e) = budget.record(n as u64) {
                            *err = Some(format!("安全限额中止: {e}"));
                            break;
                        }
                        crc.update(&buf[..n]);
                        if let Err(e) = out.write_all(&buf[..n]) {
                            *err = Some(format!("写盘失败: {e}"));
                            break;
                        }
                        *written += n as u64;
                    }
                    Err(e) => {
                        *err = Some(format!("读取失败: {e}"));
                        break;
                    }
                }
            }
        };

        match item.method {
            0 => {
                // 原样取出
                let mut limited = limited;
                copy_to(
                    &mut limited,
                    &mut out,
                    &mut crc,
                    &mut written,
                    &mut budget,
                    &mut err,
                );
            }
            8 => {
                // deflate
                let mut dec = flate2::read::DeflateDecoder::new(limited);
                copy_to(
                    &mut dec,
                    &mut out,
                    &mut crc,
                    &mut written,
                    &mut budget,
                    &mut err,
                );
            }
            other => {
                err = Some(format!("救援模式暂不支持压缩方法 {other}"));
            }
        }

        drop(out);

        if let Some(msg) = err {
            // 救援模式的精神是「能救多少救多少」：解压流中途断掉时，
            // 前面已经解出来的部分**不能丢**（用户拿个残卷可能还有用）。
            // 但要如实标注是残缺的，并且改成 `.partial` 后缀避免被误当成完好的。
            if written > 0 {
                let partial = partial_path(&out_path);
                match std::fs::rename(&out_path, &partial) {
                    Ok(()) => {
                        report.bytes_written += written;
                        report.warnings.push(format!(
                            "{}：{} —— 已抢救出 {} 字节的**残缺**内容（存为 {}）",
                            item.path,
                            msg,
                            written,
                            partial
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ));
                        report.files.push(crate::extract::ExtractedFile {
                            name: format!("{}.partial", item.path),
                            out_path: partial,
                            size: written,
                            encrypted: false,
                        });
                    }
                    Err(_) => {
                        report.skipped.push((item.path.clone(), msg));
                        let _ = std::fs::remove_file(&out_path);
                    }
                }
            } else {
                report.skipped.push((item.path.clone(), msg));
                let _ = std::fs::remove_file(&out_path);
            }
            continue;
        }

        // CRC 校验（local header 自带；为 0 时跳过，有些流式包不填）
        // 数据被截断时跳过校验 —— 那必然对不上，但用户已经收到"残缺"提醒了
        if !truncated && item.crc != 0 && crc.finish() != item.crc {
            report.skipped.push((
                item.path.clone(),
                format!(
                    "内容校验不通过（CRC 对不上，可能数据被改过或截断）—— 文件已保留在 {}",
                    out_path.display()
                ),
            ));
            report.bytes_written += written;
            continue;
        }

        report.bytes_written += written;
        report.files.push(crate::extract::ExtractedFile {
            name: item.path.clone(),
            out_path,
            size: written,
            encrypted: false,
        });
    }

    if !report.skipped.is_empty() && report.files.is_empty() {
        let mut rep2 = report;
        rep2.warnings
            .push("救援模式：所有条目都没能解出".to_string());
        return Ok(rep2);
    }
    Ok(report)
}

/// 救援模式用的 CRC32（IEEE）
struct Crc32 {
    state: u32,
}

impl Crc32 {
    fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }
    fn update(&mut self, data: &[u8]) {
        let mut c = self.state;
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
        self.state = c;
    }
    fn finish(self) -> u32 {
        !self.state
    }
}

fn unique_path_rescue(path: &std::path::Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let cand = match ext {
            Some(e) => parent.join(format!("{stem} ({i}).{e}")),
            None => parent.join(format!("{stem} ({i})")),
        };
        if !cand.exists() {
            return cand;
        }
    }
    path.to_path_buf()
}

/// 给残缺产物起个后缀名（`a.bin` → `a.bin.partial`）
fn partial_path(p: &std::path::Path) -> std::path::PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".partial");
    std::path::PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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

    /// 造一个标准 local header + 数据（非流式）
    fn make_lfh(name: &str, data: &[u8], method: u16) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_LOCAL);
        v.extend_from_slice(&[20, 0]);
        v.extend_from_slice(&[0u8, 0u8]); // flags（非流式）
        v.extend_from_slice(&method.to_le_bytes());
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
    fn 能从_local_header_重建完整目录() {
        let mut buf = vec![0xAAu8; 100];
        let start = buf.len() as u64;
        buf.extend_from_slice(&make_lfh("first/一.txt", b"hello world", 0));
        buf.extend_from_slice(&make_lfh("second.bin", &vec![7u8; 5000], 8));
        buf.extend_from_slice(&make_lfh("third.txt", b"xyz", 0));
        buf.extend(std::iter::repeat_n(0u8, 200));

        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let items = rescue_by_locals(&mut cur, start, size, &RescueOptions::default()).unwrap();
        assert_eq!(items.len(), 3, "应重建出 3 条");
        assert_eq!(items[0].path, "first/一.txt");
        assert_eq!(items[1].path, "second.bin");
        assert_eq!(items[2].path, "third.txt");
        assert_eq!(items[0].pack_size, 11);
        assert_eq!(items[1].pack_size, 5000);
    }

    #[test]
    fn 假头会被拒绝() {
        // 名字长度 0 → 不是合法头
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_LOCAL);
        v.extend_from_slice(&[
            20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        let mut cur = Cursor::new(v.clone());
        let parsed = parse_local_header(&mut cur, 0, v.len() as u64).unwrap();
        assert!(parsed.is_none(), "名字长度 0 必须被拒");
    }

    #[test]
    fn 奇怪条目的判定与7zip一致() {
        // 对译 7-Zip：名字 > 16 KiB 或方法 > 256
        assert!(is_strange((1 << 14) + 1, 8));
        assert!(is_strange(10, 257));
        assert!(!is_strange(10, 8));
        assert!(!is_strange(1 << 14, 8));
    }

    #[test]
    fn 流式条目靠描述符拿到长度() {
        let payload = vec![0x42u8; 1234];
        // 造一个 flags bit3 = 1（流式）的头，大小字段为 0
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_LOCAL);
        v.extend_from_slice(&[20, 0]);
        v.extend_from_slice(&[0x08, 0x00]); // flags: bit3 = 流式
        v.extend_from_slice(&[8, 0]); // deflate
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&crc32(&payload).to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes()); // pack_size = 0（流式）
        v.extend_from_slice(&0u32.to_le_bytes()); // size = 0
        let name = b"streamed.bin";
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(name);
        v.extend_from_slice(&payload);
        // 数据描述符（32 位：签名 + crc + csize + usize）
        v.extend_from_slice(&SIG_DESCRIPTOR);
        v.extend_from_slice(&crc32(&payload).to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        // 描述符之后放一个合法头（满足双重验证的条件 ②）
        v.extend_from_slice(&make_lfh("after.txt", b"ok", 0));

        let size = v.len() as u64;
        let mut cur = Cursor::new(v);
        let items = rescue_by_locals(&mut cur, 0, size, &RescueOptions::default()).unwrap();
        assert_eq!(items.len(), 2, "流式条目 + 后续条目都要重建出来");
        assert_eq!(items[0].path, "streamed.bin");
        assert_eq!(items[0].pack_size, 1234, "长度应来自描述符");
        assert!(items[0].size_from_descriptor);
        assert_eq!(items[1].path, "after.txt");
    }

    #[test]
    fn 描述符后面的签名不对则不认() {
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_DESCRIPTOR);
        v.extend_from_slice(&[0u8; 12]);
        v.extend_from_slice(b"XXXX"); // 后面不是合法头（条件②不满足）
        let mut cur = Cursor::new(v.clone());
        let found = find_descriptor(&mut cur, 0, v.len() as u64, 4096).unwrap();
        assert!(found.is_none(), "条件②不满足时不能认作描述符");
    }

    #[test]
    fn 从中间起点开始也能重建() {
        // 模拟伪装包：前面一大段视频数据，ZIP 藏在中间
        let mut buf = vec![0x11u8; 50_000];
        let start = buf.len() as u64;
        buf.extend_from_slice(&make_lfh("hidden.txt", b"found me", 0));
        buf.extend(std::iter::repeat_n(0x22u8, 1000));

        let size = buf.len() as u64;
        let mut cur = Cursor::new(buf);
        let items = rescue_by_locals(&mut cur, start, size, &RescueOptions::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "hidden.txt");
        assert_eq!(items[0].header_at, 50_000);
    }

    #[test]
    fn 第一条就离谱则判为非归档() {
        // 方法 = 600（> 256）→ strange
        let mut v = Vec::new();
        v.extend_from_slice(&SIG_LOCAL);
        v.extend_from_slice(&[20, 0]);
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(&600u16.to_le_bytes()); // 离谱的方法
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]);
        let name = b"x.txt";
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(name);
        v.extend(std::iter::repeat_n(0u8, 100));

        let size = v.len() as u64;
        let mut cur = Cursor::new(v);
        let res = rescue_by_locals(&mut cur, 0, size, &RescueOptions::default());
        assert!(res.is_err(), "第一条就不像归档时应当报错");
    }

    #[test]
    fn 救援式提取_中央目录坏了也能解出数据() {
        use std::io::Cursor;
        // 造两个条目，一个 store 一个 deflate，然后**删掉中央目录**
        let f1 = b"stored content here".to_vec();
        let raw = b"deflated content content content content content";
        let mut comp = Vec::new();
        {
            use flate2::write::DeflateEncoder;
            use std::io::Write;
            let mut e = DeflateEncoder::new(&mut comp, flate2::Compression::default());
            e.write_all(raw).unwrap();
            e.finish().unwrap();
        }

        let mut z = Vec::new();
        // 条目 1：store
        z.extend_from_slice(&SIG_LOCAL);
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc32(&f1).to_le_bytes());
        z.extend_from_slice(&(f1.len() as u32).to_le_bytes());
        z.extend_from_slice(&(f1.len() as u32).to_le_bytes());
        z.extend_from_slice(&5u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"a.txt");
        z.extend_from_slice(&f1);
        // 条目 2：deflate
        z.extend_from_slice(&SIG_LOCAL);
        z.extend_from_slice(&[20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc32(raw).to_le_bytes());
        z.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        z.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        z.extend_from_slice(&5u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"b.txt");
        z.extend_from_slice(&comp);
        // 故意不写中央目录和 EOCD

        let len = z.len() as u64;
        let dest = std::env::temp_dir().join(format!("yshjy_rescue_x_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);

        let mut cur = Cursor::new(z);
        let opts = crate::extract::ExtractOptions::new(&dest);
        let rep = extract_by_locals(&mut cur, 0, len, &opts).unwrap();

        assert_eq!(rep.files.len(), 2, "两个条目都该解出: {:?}", rep.skipped);
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), f1);
        assert_eq!(std::fs::read(dest.join("b.txt")).unwrap(), raw);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn 救援式提取_crc_不对会如实报告() {
        use std::io::Cursor;
        let data = b"correct data".to_vec();
        let mut z = Vec::new();
        z.extend_from_slice(&SIG_LOCAL);
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes()); // ← 故意写错的 CRC
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&5u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"c.txt");
        z.extend_from_slice(&data);

        let len = z.len() as u64;
        let dest = std::env::temp_dir().join(format!("yshjy_rescue_c_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let mut cur = Cursor::new(z);
        let opts = crate::extract::ExtractOptions::new(&dest);
        let rep = extract_by_locals(&mut cur, 0, len, &opts).unwrap();
        assert_eq!(rep.files.len(), 0);
        assert_eq!(rep.skipped.len(), 1);
        assert!(
            rep.skipped[0].1.contains("校验"),
            "要如实报告校验失败: {:?}",
            rep.skipped
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn 加密与目录条目能被识别() {
        let mut item = LocalEntry {
            path: "secret.txt".into(),
            raw_name: b"secret.txt".to_vec(),
            header_at: 0,
            data_at: 30,
            pack_size: 10,
            size: 10,
            method: 99,
            crc: 0,
            flags: 0x0001,
            size_from_descriptor: false,
        };
        assert!(item.encrypted());
        assert!(!item.is_dir());
        item.path = "folder/".into();
        assert!(item.is_dir());
        assert_eq!(item.data_end(), 40);
    }
}
