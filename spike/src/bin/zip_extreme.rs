//! 变态场景 ZIP 实验：**全文件签名扫描 + 虚接（Ranges 拼接器）+ zip crate 打开与逐条校验**。
//!
//! 这是「我们的方案」在 Rust 侧的落地：不把整文件读进内存，只记签名位置；
//! 用 Ranges 把「不连续但在逻辑上连续」的字节段拼成一个 Read+Seek 逻辑流交给 zip crate。
//!
//! 覆盖：
//!   E1 前后长垃圾   -> 尾部垃圾使 EOCD 远离文件尾，必须全文件扫描
//!   E2 视频中间夹包 -> 包前后都有数据
//!   E3 包被视频切断 -> 虚接跳过视频段（关键）
//!   E4 中间段真丢   -> 虚接后校验失败 => 只能救援模式（本 bin 只报失败，救援在 python 侧）
//!   E6 一个文件两个包 -> 逐个 EOCD 候选都要试
//!
//! 用法: zip_extreme <path>
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use yshjy_spike::rd_u32;

const LFH: u32 = 0x0403_4b50;
const CDFH: u32 = 0x0201_4b50;
const EOCD: u32 = 0x0605_4b50;

#[derive(Clone, Copy, Debug)]
struct Sig {
    kind: u8, // 0 LFH, 1 CDFH, 2 EOCD
    pos: u64,
}

fn read_at(f: &mut File, pos: u64, n: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(pos))?;
    let mut b = vec![0u8; n];
    let mut got = 0usize;
    while got < n {
        let k = f.read(&mut b[got..])?;
        if k == 0 {
            break;
        }
        got += k;
    }
    b.truncate(got);
    Ok(b)
}

fn u32_at(f: &mut File, pos: u64) -> io::Result<u32> {
    let b = read_at(f, pos, 4)?;
    if b.len() < 4 {
        return Ok(0);
    }
    Ok(rd_u32(&b, 0))
}

/// 流式全文件扫描签名（1 MiB 块 + 3 字节重叠，峰值内存 ~1 MiB）。
fn scan(path: &str) -> io::Result<(u64, Vec<Sig>)> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut sigs: Vec<Sig> = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut base = 0u64;
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut data = Vec::with_capacity(carry.len() + n);
        data.extend_from_slice(&carry);
        data.extend_from_slice(&buf[..n]);
        let dbase = base - carry.len() as u64;
        if data.len() >= 4 {
            let lim = data.len() - 3;
            for i in 0..lim {
                let v = rd_u32(&data, i);
                let k = if v == LFH {
                    0u8
                } else if v == CDFH {
                    1
                } else if v == EOCD {
                    2
                } else {
                    continue;
                };
                sigs.push(Sig {
                    kind: k,
                    pos: dbase + i as u64,
                });
            }
        }
        let keep = data.len().min(3);
        carry = data[data.len() - keep..].to_vec();
        base += n as u64;
    }
    Ok((len, sigs))
}

/// 从 `from` 起在 `(from, to)` 内找最近的下一个 PK 签名（分块读）。
fn find_next_sig(f: &mut File, from: u64, to: u64, eocd_end: u64) -> io::Result<Option<u64>> {
    let mut s = from;
    let buf = vec![0u8; 64 * 1024];
    while s < to {
        let want = ((to - s) as usize).min(buf.len());
        let b = read_at(f, s, want)?;
        if b.len() < 4 {
            return Ok(None);
        }
        for i in 0..b.len() - 3 {
            let v = rd_u32(&b, i);
            if v == LFH || v == CDFH {
                return Ok(Some(s + i as u64));
            }
        }
        // 保留 3 字节重叠
        s += (b.len() - 3) as u64;
        if s + 4 > eocd_end {
            break;
        }
    }
    Ok(None)
}

/// 从第一条 LFH 走到 CD 起点，把「洞」（成员之间的非 ZIP 字节）挖掉，返回拼接段。
/// 返回 (ranges, total_hole, walkable)
fn build_ranges(
    f: &mut File,
    first_lfh: u64,
    eocd_end: u64,
) -> io::Result<(Vec<(u64, u64)>, u64, bool)> {
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let mut seg_start = first_lfh;
    let mut p = first_lfh;
    let mut hole = 0u64;

    loop {
        if p + 4 > eocd_end {
            break;
        }
        let hdr = read_at(f, p, 30)?;
        if hdr.len() < 30 || rd_u32(&hdr, 0) != LFH {
            break;
        }
        let flags = u16::from_le_bytes([hdr[6], hdr[7]]);
        let csize = rd_u32(&hdr, 18) as u64;
        let nl = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
        let el = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;
        if flags & 0x08 != 0 {
            // data descriptor：LFH 里没有长度，无法按长度走 → 退化为「整段不切」
            return Ok((vec![(first_lfh, eocd_end - first_lfh)], 0, false));
        }
        let data_end = p + 30 + nl + el + csize;
        if data_end > eocd_end {
            // 成员被截断
            return Ok((vec![(first_lfh, eocd_end - first_lfh)], 0, false));
        }
        let sig = u32_at(f, data_end)?;
        if sig == CDFH {
            break;
        }
        if sig == LFH {
            p = data_end;
            continue;
        }
        // 洞：data_end 处不是签名 → 向后找下一个签名
        match find_next_sig(f, data_end + 1, eocd_end, eocd_end)? {
            Some(x) => {
                ranges.push((seg_start, data_end - seg_start));
                hole += x - data_end;
                seg_start = x;
                p = x;
            }
            None => {
                ranges.push((seg_start, data_end - seg_start));
                hole += eocd_end - data_end;
                seg_start = eocd_end;
                break;
            }
        }
    }
    if seg_start < eocd_end {
        ranges.push((seg_start, eocd_end - seg_start));
    }
    Ok((ranges, hole, true))
}

// ───────────────────── 虚接拼接器：同文件多个区间当成一个逻辑流 ─────────────────────
struct Ranges {
    f: File,
    ranges: Vec<(u64, u64)>,
    prefix: Vec<u64>,
    idx: usize,
    off: u64,
    total: u64,
}

impl Ranges {
    fn new(path: &str, ranges: Vec<(u64, u64)>) -> io::Result<Self> {
        let f = File::open(path)?;
        let mut prefix = Vec::with_capacity(ranges.len());
        let mut acc = 0u64;
        for (_, l) in &ranges {
            acc += l;
            prefix.push(acc);
        }
        Ok(Self {
            f,
            ranges,
            prefix,
            idx: 0,
            off: 0,
            total: acc,
        })
    }
    fn global(&self) -> u64 {
        let base = if self.idx == 0 {
            0
        } else {
            self.prefix[self.idx - 1]
        };
        base + self.off
    }
}

impl Read for Ranges {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.idx >= self.ranges.len() {
                return Ok(0);
            }
            let (b, l) = self.ranges[self.idx];
            if self.off >= l {
                self.idx += 1;
                self.off = 0;
                continue;
            }
            let want = ((l - self.off) as usize).min(buf.len());
            self.f.seek(SeekFrom::Start(b + self.off))?;
            let n = self.f.read(&mut buf[..want])?;
            if n == 0 {
                self.idx += 1;
                self.off = 0;
                continue;
            }
            self.off += n as u64;
            return Ok(n);
        }
    }
}

impl Seek for Ranges {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let cur = self.global();
        let t: i128 = match from {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(p) => self.total as i128 + p as i128,
            SeekFrom::Current(p) => cur as i128 + p as i128,
        };
        if t < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Ranges: seek<0"));
        }
        let t = (t as u64).min(self.total);
        for (i, &(b, l)) in self.ranges.iter().enumerate() {
            let base = if i == 0 { 0 } else { self.prefix[i - 1] };
            if t <= base + l {
                self.idx = i;
                self.off = t - base;
                self.f.seek(SeekFrom::Start(b + self.off))?;
                return Ok(t);
            }
        }
        self.idx = self.ranges.len();
        self.off = 0;
        Ok(self.total)
    }
}

fn vm_hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM ?")
        .trim()
        .to_string()
}

/// 用 zip crate 打开一个 Read+Seek，逐条流式解压 + CRC 校验，返回 (成功数, 总数, 明细)
fn open_and_verify<R: Read + Seek>(r: R) -> (usize, usize, Vec<String>) {
    match zip::ZipArchive::new(r) {
        Ok(mut a) => {
            let n = a.len();
            let mut ok = 0usize;
            let mut lines = Vec::new();
            for i in 0..n {
                match a.by_index(i) {
                    Ok(mut e) => {
                        let name = e
                            .name()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|_| "?".to_string());
                        let want_crc = e.crc32();
                        let mut buf = Vec::new();
                        match e.read_to_end(&mut buf) {
                            Ok(_) => {
                                let mut h = crc32fast::Hasher::new();
                                h.update(&buf);
                                let got = h.finalize();
                                if got == want_crc {
                                    ok += 1;
                                    lines.push(format!("   ✔ {name} {} B CRC OK", buf.len()));
                                } else {
                                    lines.push(format!(
                                        "   ✘ {name} CRC 不符 期望={want_crc:08x} 实际={got:08x}"
                                    ));
                                }
                            }
                            Err(err) => lines.push(format!("   ✘ {name} 读数据失败: {err}")),
                        }
                    }
                    Err(err) => lines.push(format!("   ✘ [{i}] 打不开: {err}")),
                }
            }
            (ok, n, lines)
        }
        Err(e) => (0, 0, vec![format!("   打开失败: {e}")]),
    }
}

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            println!("用法: zip_extreme <path>");
            return;
        }
    };
    println!("═══ 文件: {path}");
    let (len, sigs) = match scan(&path) {
        Ok(v) => v,
        Err(e) => {
            println!("  扫描失败: {e}");
            return;
        }
    };
    let n_lfh = sigs.iter().filter(|s| s.kind == 0).count();
    let n_cd = sigs.iter().filter(|s| s.kind == 1).count();
    let n_eocd = sigs.iter().filter(|s| s.kind == 2).count();
    println!(
        "   file_len={len}  扫描命中: LFH={n_lfh} CDFH={n_cd} EOCD={n_eocd}  (峰值内存 {})",
        vm_hwm()
    );

    // 对照：直接把整个文件喂给 zip crate（不做任何偏移处理），照样做 CRC 校验
    if let Ok(f) = File::open(&path) {
        let (ok, tot, lines) = open_and_verify(f);
        if tot > 0 {
            println!("   [对照 naive ZipArchive::new(File)] 打开成功 entries={tot}  CRC 通过 {ok}/{tot}");
        } else {
            println!("   [对照 naive ZipArchive::new(File)] 打开失败: {}", lines.join(" | "));
        }
    }

    let mut eocds: Vec<u64> = sigs
        .iter()
        .filter(|s| s.kind == 2)
        .map(|s| s.pos)
        .collect();
    eocds.sort_unstable();
    eocds.dedup();
    if eocds.is_empty() {
        println!("   ✗ 全文件找不到 EOCD（尾部被截断？）→ 只能走救援模式（无目录表）");
        return;
    }
    let mut lfhs: Vec<u64> = sigs
        .iter()
        .filter(|s| s.kind == 0)
        .map(|s| s.pos)
        .collect();
    lfhs.sort_unstable();
    lfhs.dedup();

    for j in (0..eocds.len()).rev() {
        let eo = eocds[j];
        let lo = if j == 0 { 0 } else { eocds[j - 1] + 22 };
        let hdr = read_at(&mut File::open(&path).unwrap(), eo, 22).unwrap();
        if hdr.len() < 22 {
            continue;
        }
        let entries = u16::from_le_bytes([hdr[10], hdr[11]]);
        let size_cd = rd_u32(&hdr, 12) as u64;
        let off_cd = rd_u32(&hdr, 16) as u64;
        let clen = u16::from_le_bytes([hdr[20], hdr[21]]) as u64;
        let eocd_end = eo + 22 + clen;
        let guess = eo as i128 - size_cd as i128 - off_cd as i128;
        println!(
            "\n── EOCD @ {eo} (距EOF {}) entries={entries} size_cd={size_cd} offset_cd={off_cd} comment={clen}",
            len - eo
        );
        println!("   朴素推算 ZIP 起点(EOF−size_cd−offset_cd)={guess}");

        let cands: Vec<u64> = lfhs.iter().cloned().filter(|&p| p >= lo && p <= eo).collect();
        let mut best: Option<(usize, usize, Vec<String>, Vec<(u64,u64)>, u64, u64)> = None;
        let mut hit = false;
        for &c in &cands {
            let (ranges, hole, _walk) = build_ranges(&mut File::open(&path).unwrap(), c, eocd_end).unwrap();
            let reader = Ranges::new(&path, ranges.clone()).unwrap();
            let (ok, tot, lines) = open_and_verify(reader);
            if best.as_ref().map_or(true, |b| ok > b.0) {
                best = Some((ok, tot, lines, ranges.clone(), hole, c));
            }
            if tot > 0 && ok == tot {
                let kept: u64 = ranges.iter().map(|r| r.1).sum();
                println!(
                    "   ✅ 起点 LFH @{c}（Δ={}）: 虚接 + zip crate 打开成功 entries={tot} CRC 全过 {ok}/{tot}",
                    c as i128 - guess
                );
                println!("      段={:?}  保留={kept}  跳过(洞/插入)={hole}", ranges);
                hit = true;
                break;
            }
        }
        if !hit {
            match best {
                Some((ok, tot, lines, ranges, hole, c)) => {
                    println!(
                        "   ❌ 全部 {} 个 LFH 候选虚接后都无法全过 CRC。最好一次: 起点 @{c} entries={tot} CRC {ok}/{tot}",
                        cands.len()
                    );
                    println!("      段={:?}  跳过(洞/插入)={hole}", ranges);
                    for l in lines.iter().take(10) {
                        println!("{l}");
                    }
                    println!("       ⇒ 判定：结构自洽性被破坏 = 真丢数据 → 走救援模式（无目录表）");
                }
                None => println!("   ❌ [{lo}, {eo}] 内没有 LFH 候选"),
            }
        }
        println!("   (峰值内存 {})", vm_hwm());
    }
}
