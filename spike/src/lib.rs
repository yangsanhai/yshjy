//! yshjy spike 公共库：伪装包基准偏移定位 + 自定义 Read+Seek 包装器。
//!
//! 这些都是正式版 yshjy 需要的核心原语，先在 spike 里跑通。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

pub const SIG_LFH: u32 = 0x04034b50;
pub const SIG_CDFH: u32 = 0x02014b50;
pub const SIG_EOCD: u32 = 0x06054b50;
pub const SIG_EOCD64: u32 = 0x06064b50;
pub const SIG_LOC64: u32 = 0x07064b50;

pub const SEVENZ_SIG: [u8; 6] = [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
pub const RAR15_SIG: [u8; 7] = *b"Rar!\x1a\x07\x00";
pub const RAR50_SIG: [u8; 8] = *b"Rar!\x1a\x07\x01\x00";

pub fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
pub fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 在给定文件里扫描某个签名的第一个出现位置。
pub fn find_sig(path: &str, sig: &[u8], scan: u64) -> io::Result<Option<u64>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let n = len.min(scan) as usize;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf)?;
    Ok(buf
        .windows(sig.len())
        .position(|w| w == sig)
        .map(|p| p as u64))
}

// ───────────────────────── ZIP 基准定位（自研解析） ─────────────────────────

#[derive(Debug, Clone)]
pub struct ZipBaseInfo {
    pub file_len: u64,
    pub eocd_off: u64,
    pub comment_len: u16,
    pub zip64: bool,
    pub eocd64_off: Option<u64>,
    pub loc64_off: Option<u64>,
    pub loc_declared_eocd64: Option<u64>,
    /// EOCD64（ZIP64）或 EOCD（ZIP32）里记录的 CD 偏移字段
    pub cd_offset_field: u64,
    /// CD 第一条 CDFH 的真实绝对位置（扫描得到）
    pub first_cdfh_real: u64,
    /// ZIP64 EOCD 真实位置 − locator 声明值（= zip crate 的 archive_offset）
    pub delta_eocd: Option<i128>,
    /// CD 真实位置 − EOCD64/EOCD 记录值（= 我们原型用的「ZIP 起点」）
    pub delta_cd: i128,
    /// 第一条 entry 的 LFH 真实位置 − CD entry 记录值（抽样校验）
    pub delta_entry: Option<i128>,
    pub n_entries: u64,
    pub first_entry_name: Option<String>,
    pub cd_wellformed: bool,
}

/// 从 `p` 起逐条遍历 CDFH 直到恰好到达 `end`，返回结束位置（成功时 == end）。
fn walk_cd(buf: &[u8], buf_base: u64, p: u64, end: u64) -> Option<(u64, usize)> {
    let mut pos = p;
    let mut count = 0usize;
    while pos + 46 <= end && pos + 46 <= buf_base + buf.len() as u64 {
        let o = (pos - buf_base) as usize;
        if rd_u32(buf, o) != SIG_CDFH {
            return None;
        }
        let fnl = rd_u16(buf, o + 28) as u64;
        let efl = rd_u16(buf, o + 30) as u64;
        let fcl = rd_u16(buf, o + 32) as u64;
        pos += 46 + fnl + efl + fcl;
        count += 1;
        if pos == end {
            return Some((pos, count));
        }
        if pos > end {
            return None;
        }
    }
    None
}

/// 解析 CD entry 的 ZIP64 extra，拿到真实的 csize/usize/lho。
fn zip64_entry_values(entry: &[u8]) -> (Option<u64>, Option<u64>, Option<u64>) {
    let fnl = rd_u16(entry, 28) as usize;
    let efl = rd_u16(entry, 30) as usize;
    let extra = &entry[46 + fnl..46 + fnl + efl];
    let (mut cs32, mut us32) = (rd_u32(entry, 20), rd_u32(entry, 24));
    let lho32 = rd_u32(entry, 42);
    let mut out = (None, None, None);
    let mut i = 0usize;
    while i + 4 <= extra.len() {
        let hid = rd_u16(extra, i);
        let hsz = rd_u16(extra, i + 2) as usize;
        let body = &extra[i + 4..(i + 4 + hsz).min(extra.len())];
        if hid == 0x0001 {
            let mut j = 0usize;
            if us32 == 0xFFFF_FFFF && j + 8 <= body.len() {
                out.0 = Some(rd_u64(body, j));
                us32 = 0;
                j += 8;
            }
            if cs32 == 0xFFFF_FFFF && j + 8 <= body.len() {
                out.1 = Some(rd_u64(body, j));
                cs32 = 0;
                j += 8;
            }
            if lho32 == 0xFFFF_FFFF && j + 8 <= body.len() {
                out.2 = Some(rd_u64(body, j));
            }
            break;
        }
        i += 4 + hsz;
    }
    let _ = (&mut cs32, &mut us32);
    out
}

/// 自研 ZIP 基准定位：返回 Δ_cd / Δ_eocd / Δ_entry 三个量及真实 CD 位置。
pub fn detect_zip_base(path: &str) -> io::Result<Option<ZipBaseInfo>> {
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();
    const TAIL: u64 = 512 * 1024;
    let tail_len = file_len.min(TAIL);
    let tail_base = file_len - tail_len;
    f.seek(SeekFrom::Start(tail_base))?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail)?;

    // 找最后一个 EOCD
    let mut eocd_rel = None;
    for i in (0..tail.len().saturating_sub(4)).rev() {
        if rd_u32(&tail, i) == SIG_EOCD {
            eocd_rel = Some(i);
            break;
        }
    }
    let eocd_rel = match eocd_rel {
        Some(v) => v,
        None => return Ok(None),
    };
    let eocd_off = tail_base + eocd_rel as u64;
    let n_total16 = rd_u16(&tail, eocd_rel + 10);
    let cd_off32 = rd_u32(&tail, eocd_rel + 16);
    let comment_len = rd_u16(&tail, eocd_rel + 20);
    let zip64 = cd_off32 == 0xFFFF_FFFF || n_total16 == 0xFFFF;

    let mut eocd64_off = None;
    let mut loc64_off = None;
    let mut loc_declared = None;
    let mut cd_offset_field = cd_off32 as u64;
    let mut n_entries = n_total16 as u64;
    let mut cd_end = eocd_off; // ZIP 里 CD 紧邻 EOCD（ZIP64 时紧邻 EOCD64）

    if zip64 {
        // locator 就在 EOCD 前 20 字节
        if eocd_off >= 20 {
            let lo = eocd_off - 20;
            f.seek(SeekFrom::Start(lo))?;
            let mut l = [0u8; 20];
            f.read_exact(&mut l)?;
            if rd_u32(&l, 0) == SIG_LOC64 {
                loc64_off = Some(lo);
                loc_declared = Some(rd_u64(&l, 8));
            }
        }
        // 向后扫 EOCD64（限制在 locator 前 64 KiB）
        let scan_from = eocd_off.saturating_sub(64 * 1024 + 20);
        let scan_len = (eocd_off - 20 - scan_from) as usize;
        f.seek(SeekFrom::Start(scan_from))?;
        let mut w = vec![0u8; scan_len];
        f.read_exact(&mut w)?;
        let mut found = None;
        for i in (0..scan_len.saturating_sub(4)).rev() {
            if rd_u32(&w, i) == SIG_EOCD64 {
                found = Some(scan_from + i as u64);
                break;
            }
        }
        if let Some(e64) = found {
            eocd64_off = Some(e64);
            cd_end = e64;
            f.seek(SeekFrom::Start(e64))?;
            let mut b = [0u8; 56];
            f.read_exact(&mut b)?;
            n_entries = rd_u64(&b, 32);
            cd_offset_field = rd_u64(&b, 48);
        }
    }

    // 扫真实 CD：在 CD 结束位置前 1 MiB 内找能一路走通到 cd_end 的 CDFH
    let win_from = cd_end.saturating_sub(1024 * 1024);
    let win_len = (cd_end - win_from) as usize;
    f.seek(SeekFrom::Start(win_from))?;
    let mut w = vec![0u8; win_len];
    f.read_exact(&mut w)?;
    let mut cands = Vec::new();
    for i in 0..win_len.saturating_sub(4) {
        if rd_u32(&w, i) == SIG_CDFH {
            cands.push(win_from + i as u64);
        }
    }
    let mut first_cdfh_real = None;
    let mut cd_wellformed = false;
    for &c in cands.iter().rev() {
        if let Some((_end, cnt)) = walk_cd(&w, win_from, c, cd_end) {
            if cnt >= 1 {
                first_cdfh_real = Some(c);
                cd_wellformed = true;
                break;
            }
        }
    }

    let delta_cd = match first_cdfh_real {
        Some(c) => c as i128 - cd_offset_field as i128,
        None => return Ok(None),
    };
    let delta_eocd = match (eocd64_off, loc_declared) {
        (Some(e), Some(l)) => Some(e as i128 - l as i128),
        _ => None,
    };

    // 抽样第一条 entry，核对 LFH 真实位置
    let mut delta_entry = None;
    let mut first_entry_name = None;
    if let Some(c) = first_cdfh_real {
        let o = (c - win_from) as usize;
        let fnl = rd_u16(&w, o + 28) as usize;
        let name = String::from_utf8_lossy(&w[o + 46..o + 46 + fnl]).to_string();
        first_entry_name = Some(name);
        let (us, cs, lho64) = zip64_entry_values(&w[o..]);
        let lho = lho64.unwrap_or_else(|| rd_u32(&w, o + 42) as u64);
        let guess = delta_cd + lho as i128;
        if guess >= 0 && (guess as u64) + 4 <= file_len {
            f.seek(SeekFrom::Start(guess as u64))?;
            let mut s = [0u8; 4];
            f.read_exact(&mut s)?;
            if rd_u32(&s, 0) == SIG_LFH {
                delta_entry = Some(guess - lho as i128);
            }
        }
        let _ = (us, cs);
    }

    Ok(Some(ZipBaseInfo {
        file_len,
        eocd_off,
        comment_len,
        zip64,
        eocd64_off,
        loc64_off,
        loc_declared_eocd64: loc_declared,
        cd_offset_field,
        first_cdfh_real: first_cdfh_real.unwrap(),
        delta_eocd,
        delta_cd,
        delta_entry,
        n_entries,
        first_entry_name,
        cd_wellformed,
    }))
}

// ───────────────────────── Read+Seek 包装器 ─────────────────────────

/// 把底层文件 `[base, base+len)` 映射成从 0 开始的逻辑流。
pub struct Window {
    file: File,
    pub base: u64,
    pub len: u64,
}

impl Window {
    pub fn open(path: &str, base: u64, len: u64) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            base,
            len,
        })
    }
    pub fn open_to_end(path: &str, base: u64) -> io::Result<Self> {
        let len = File::open(path)?.metadata()?.len();
        Self::open(path, base, len - base)
    }
}

impl Read for Window {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let pos = self.file.stream_position()?;
        if pos < self.base {
            return Err(io::Error::new(io::ErrorKind::Other, "Window: 越界读取"));
        }
        let end = self.base + self.len;
        if pos >= end {
            return Ok(0);
        }
        let n = buf.len().min((end - pos) as usize);
        self.file.read(&mut buf[..n])
    }
}

impl Seek for Window {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let abs: i128 = match from {
            SeekFrom::Start(p) => self.base as i128 + p as i128,
            SeekFrom::End(p) => (self.base as i128 + self.len as i128) + p as i128,
            SeekFrom::Current(p) => self.file.stream_position()? as i128 + p as i128,
        };
        if abs < self.base as i128 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Window: seek 越界"));
        }
        let abs = abs as u64;
        self.file.seek(SeekFrom::Start(abs))?;
        Ok(abs - self.base)
    }
}

/// 同 `Window`，但在读取路径上「就地打补丁」：绝不改动磁盘文件。
/// 用于 8 字节 locator 补丁方案（伪装包 CD 与 locator 基准不一致时）。
pub struct PatchWindow {
    file: File,
    pub base: u64,
    pub len: u64,
    patches: Vec<(u64, Vec<u8>)>,
}

impl PatchWindow {
    pub fn open(path: &str, base: u64, len: u64, patches: Vec<(u64, Vec<u8>)>) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            base,
            len,
            patches,
        })
    }
}

impl Read for PatchWindow {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.file.stream_position()?;
        if start < self.base {
            return Err(io::Error::new(io::ErrorKind::Other, "PatchWindow: 越界读取"));
        }
        let end = self.base + self.len;
        if start >= end {
            return Ok(0);
        }
        let n = buf.len().min((end - start) as usize);
        let n = self.file.read(&mut buf[..n])?;
        for (off, data) in &self.patches {
            for (i, b) in data.iter().enumerate() {
                let a = *off + i as u64;
                if a >= start && a < start + n as u64 {
                    buf[(a - start) as usize] = *b;
                }
            }
        }
        Ok(n)
    }
}

impl Seek for PatchWindow {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let abs: i128 = match from {
            SeekFrom::Start(p) => self.base as i128 + p as i128,
            SeekFrom::End(p) => (self.base as i128 + self.len as i128) + p as i128,
            SeekFrom::Current(p) => self.file.stream_position()? as i128 + p as i128,
        };
        if abs < self.base as i128 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "PatchWindow: seek 越界"));
        }
        let abs = abs as u64;
        self.file.seek(SeekFrom::Start(abs))?;
        Ok(abs - self.base)
    }
}

/// 多个文件首尾相接当成一个逻辑流（7z 分卷用）。
pub struct Concat {
    parts: Vec<File>,
    lens: Vec<u64>,
    cur: usize,
    total: u64,
}

impl Concat {
    pub fn open(paths: &[String]) -> io::Result<Self> {
        let mut parts = Vec::new();
        let mut lens = Vec::new();
        for p in paths {
            let f = File::open(p)?;
            lens.push(f.metadata()?.len());
            parts.push(f);
        }
        let total = lens.iter().sum();
        Ok(Self {
            parts,
            lens,
            cur: 0,
            total,
        })
    }
}

impl Read for Concat {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.cur >= self.parts.len() {
                return Ok(0);
            }
            let n = self.parts[self.cur].read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            self.cur += 1;
        }
    }
}

impl Seek for Concat {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        // 当前全局逻辑位置
        let cur_base: u64 = self.lens[..self.cur].iter().sum();
        let here = cur_base + self.parts[self.cur].stream_position()?;
        let target: i128 = match from {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(p) => self.total as i128 + p as i128,
            SeekFrom::Current(p) => here as i128 + p as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Concat: seek 越界"));
        }
        let target = (target as u64).min(self.total); // 与普通文件一致：允许停在 EOF
        let mut base = 0u64;
        for (i, l) in self.lens.iter().enumerate() {
            if target <= base + *l {
                self.cur = i;
                self.parts[i].seek(SeekFrom::Start(target - base))?;
                return Ok(target); // ← 必须返回全局逻辑位置
            }
            base += *l;
        }
        Ok(self.total)
    }
}
