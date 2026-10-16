//! 偏移修复 —— 坏包能被解开，很多时候只是**头部几个字节写错了**
//!
//! # 思路（综合参考项目）
//! - zip2 的「8 字节 locator 补丁」：不改文件，只在内存里把错字段改对，让库自己算
//! - 7-Zip 的 `Overflow32bit` 模式识别（`ZipIn.cpp:3039`）：ZIP32 时代偏移超过
//!   4 GiB 会回绕，于是偏移值差整数倍的 2³²
//! - 7-Zip 的 `Cd_NumEntries_Overflow_16bit`（`ZipIn.cpp:3208`）：条目数超过 65535 的回绕
//!
//! # 我们做什么
//! 1. 读 EOCD，检查它声明的「中央目录位置 + 大小」是否自洽（`cd_offset + cd_size ≈ eocd 位置`）
//! 2. 不自洽时枚举几种最常见的原因，算出一组**补丁**（只改那几个字节）
//! 3. 交给 [`PatchWindow`] + `zip` 库重试 —— 库有多强，我们就能解多强
//!
//! # 与救援模式的分工
//! - 本模块：数据完好、**只是索引错了** → 修索引，走正常解码路径（快、完整）
//! - [`crate::rescue`]：索引没了/彻底坏 → 靠逐条 local header 重建（慢、可能缺）

use crate::error::{Error, ErrorCode, Result};
use crate::io::patch::Patch;
use std::io::{Read, Seek, SeekFrom};

/// EOCD 签名
const EOCD_SIG: [u8; 4] = *b"PK\x05\x06";
/// 中央目录签名
const CD_SIG: [u8; 4] = *b"PK\x01\x02";

/// 修复方案
#[derive(Debug, Clone)]
pub struct RepairPlan {
    /// 要打的补丁
    pub patches: Vec<Patch>,
    /// 人话说明（给日志/界面看）
    pub reason: String,
    /// 预计修正后的 cd_offset
    pub fixed_cd_offset: u64,
}

/// 修复分析结果
#[derive(Debug, Clone)]
pub enum RepairAnalysis {
    /// 自洽，不需要修
    Ok,
    /// 可以修
    Repairable(RepairPlan),
    /// 分析不出来
    Unknown(String),
}

/// 读 EOCD 关键字段
fn read_eocd<R: Read + Seek>(
    r: &mut R,
    base: u64,
    len: u64,
) -> Result<Option<(u64, u64, u64, u64)>> {
    // 返回 (eocd 相对偏移, cd_offset, cd_size, entries)
    // EOCD 在归档末尾 64 KiB 内（规范如此）
    let tail = len.min(64 * 1024);
    let start = len - tail;
    r.seek(SeekFrom::Start(base + start))?;
    let mut buf = vec![0u8; tail as usize];
    let n = read_up_to(r, &mut buf)?;
    buf.truncate(n);

    // 从后往前找 EOCD 签名（最靠后的那个有效）
    let mut found = None;
    let mut i = buf.len();
    while i >= 4 {
        i -= 1;
        if buf[i - 3..i + 1] == EOCD_SIG {
            let p = i - 3;
            if p + 22 <= buf.len() {
                found = Some(p);
                break;
            }
        }
        if i < 4 {
            break;
        }
    }
    let p = match found {
        Some(p) => p,
        None => return Ok(None),
    };

    let cd_size = u32::from_le_bytes(buf[p + 12..p + 16].try_into().unwrap()) as u64;
    let cd_off = u32::from_le_bytes(buf[p + 16..p + 20].try_into().unwrap()) as u64;
    let entries = u16::from_le_bytes(buf[p + 10..p + 12].try_into().unwrap()) as u64;
    Ok(Some((start + p as u64, cd_off, cd_size, entries)))
}

/// 判断某个位置是否真的是中央目录（看签名）
fn is_cd_at<R: Read + Seek>(r: &mut R, base: u64, off: u64) -> bool {
    if r.seek(SeekFrom::Start(base + off)).is_err() {
        return false;
    }
    let mut sig = [0u8; 4];
    matches!(read_up_to(r, &mut sig), Ok(4)) && sig == CD_SIG
}

/// **分析一个包的索引是否自洽，不自洽时给出修复方案**
///
/// 参数都相对归档起点：`len` 是归档数据长度（`base` 由调用方给出）。
pub fn analyze<R: Read + Seek>(r: &mut R, base: u64, len: u64) -> Result<RepairAnalysis> {
    let (eocd_rel, cd_off, cd_size, _entries) = match read_eocd(r, base, len)? {
        Some(v) => v,
        None => return Ok(RepairAnalysis::Unknown("找不到 EOCD".into())),
    };

    // ① 自洽判断：CD 应该在 [cd_off, cd_off + cd_size)，且紧邻 EOCD
    let expected_end = cd_off.saturating_add(cd_size);
    if expected_end <= eocd_rel && is_cd_at(r, base, cd_off) {
        return Ok(RepairAnalysis::Ok);
    }

    // ② 候选 1：CD 紧贴在 EOCD 前面（最常见 —— 前面的偏移被改过）
    let cand_tight = eocd_rel.saturating_sub(cd_size);
    if is_cd_at(r, base, cand_tight) && cand_tight != cd_off {
        return Ok(RepairAnalysis::Repairable(RepairPlan {
            patches: vec![Patch::new(
                eocd_rel + 16,
                (cand_tight as u32).to_le_bytes().to_vec(),
            )],
            reason: format!(
                "中央目录记录的偏移是 {cd_off}，实际在 {cand_tight}（把偏移改成正确值）"
            ),
            fixed_cd_offset: cand_tight,
        }));
    }

    // ③ 候选 2：32 位回绕（偏移差整数倍的 2³²）
    for k in [1u64, 2, 3] {
        for (sign, delta) in [("+", k << 32), ("-", k << 32)] {
            let cand = if sign == "+" {
                cd_off.checked_add(delta)
            } else {
                cd_off.checked_sub(delta)
            };
            let Some(cand) = cand else { continue };
            if cand >= len {
                continue;
            }
            if is_cd_at(r, base, cand) {
                return Ok(RepairAnalysis::Repairable(RepairPlan {
                    patches: vec![Patch::new(
                        eocd_rel + 16,
                        (cand as u32).to_le_bytes().to_vec(),
                    )],
                    reason: format!(
                        "偏移疑似 32 位回绕：记录的 {cd_off} 应改为 {cand}（相差 {sign}{k}×2³²）"
                    ),
                    fixed_cd_offset: cand,
                }));
            }
        }
    }

    // ④ 候选 3：整个 CD 块向后平移（前面插了数据，但没更新索引）
    //    在 CD 记录位置附近搜 PK\x01\x02
    let search_from = cd_off.saturating_sub(4096);
    let search_to = (cd_off + 4096).min(len.saturating_sub(4));
    if search_to > search_from {
        r.seek(SeekFrom::Start(base + search_from))?;
        let mut win = vec![0u8; (search_to - search_from) as usize];
        let n = read_up_to(r, &mut win)?;
        win.truncate(n);
        if let Some(p) = win.windows(4).position(|w| w == CD_SIG) {
            let cand = search_from + p as u64;
            if cand != cd_off {
                return Ok(RepairAnalysis::Repairable(RepairPlan {
                    patches: vec![Patch::new(
                        eocd_rel + 16,
                        (cand as u32).to_le_bytes().to_vec(),
                    )],
                    reason: format!("在 {cand} 处找到了真的中央目录（记录值 {cd_off} 是错的）"),
                    fixed_cd_offset: cand,
                }));
            }
        }
    }

    Ok(RepairAnalysis::Unknown(format!(
        "中央目录位置 {cd_off}（大小 {cd_size}）与 EOCD({eocd_rel}) 不自洽，且没找到可修的候选"
    )))
}

/// **从「索引坏了」的包里救数据**（高层入口，CLI 直接用这个）
///
/// 流程：
/// 1. 用 [`crate::scan`] 扫出归档真正的起点（不信任头部记录的偏移）
/// 2. [`analyze`] 判断索引错在哪
/// 3. 用 [`crate::io::patch::PatchWindow`] 在内存里把错的字节改对
/// 4. 交给解码器正常解 —— **磁盘上的源文件一个字节都不动**
///
/// 返回 `(解压报告, 修复说明)`。
pub fn rescue_broken_index<R: Read + Seek>(
    r: &mut R,
    file_size: u64,
    opts: &crate::extract::ExtractOptions,
) -> Result<(crate::extract::ExtractReport, String)> {
    // ① 找归档起点（只认高置信度的 ZIP 命中）
    let hits = crate::scan::scan_whole_file(r, file_size, &crate::scan::ScanOptions::strict())?;
    let base = hits
        .iter()
        .find(|h| h.format == crate::layout::ArchiveFormat::Zip)
        .map(|h| h.offset)
        .ok_or_else(|| {
            Error::new(ErrorCode::NotArchive).with("reason", "没找到压缩包起点（可能不是 ZIP）")
        })?;
    let len = file_size.saturating_sub(base);

    // ② 分析索引
    match analyze(r, base, len)? {
        RepairAnalysis::Ok => {
            let rep = crate::extract::extract_zip_at(&mut *r, base, len, opts)?;
            Ok((rep, "索引本身没问题，已直接解出".into()))
        }
        RepairAnalysis::Repairable(plan) => {
            let reason = plan.reason.clone();
            let patched = crate::io::patch::PatchWindow::new(&mut *r, base, len, plan.patches)
                .map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("建补丁窗口失败: {e}"))
                })?;
            let mut patched = patched;
            let rep = crate::extract::extract_zip_at(&mut patched, 0, len, opts)?;
            Ok((rep, reason))
        }
        RepairAnalysis::Unknown(why) => {
            Err(Error::new(ErrorCode::NotArchive).with("reason", format!("修不了：{why}")))
        }
    }
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(Error::new(ErrorCode::Internal).context(format!("read: {e}")));
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// 手工造一个 ZIP（可选故意写错 EOCD 里的 cd_offset）
    fn make_zip_wrong_offset(wrong: bool) -> Vec<u8> {
        let mut zip = Vec::new();
        // local header + 数据
        let data = b"payload!";
        let crc = crate::scan::crc32(data);
        zip.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        zip.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&7u16.to_le_bytes()); // name len
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(b"a.txt");
        zip.extend_from_slice(data);

        let cd_start = zip.len() as u64;
        // central directory
        zip.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        zip.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0]);
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
        zip.extend_from_slice(&7u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(b"a.txt");
        let cd_size = zip.len() as u64 - cd_start;

        // EOCD
        zip.extend_from_slice(&EOCD_SIG);
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&(cd_size as u32).to_le_bytes());
        let off = if wrong { 9999u32 } else { cd_start as u32 };
        zip.extend_from_slice(&off.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip
    }

    #[test]
    fn 正常的包判定为无需修复() {
        let z = make_zip_wrong_offset(false);
        let len = z.len() as u64;
        let mut c = Cursor::new(z);
        match analyze(&mut c, 0, len).unwrap() {
            RepairAnalysis::Ok => {}
            other => panic!("正常包不该需要修: {other:?}"),
        }
    }

    #[test]
    fn 偏移被改错的包能被修() {
        let z = make_zip_wrong_offset(true);
        let len = z.len() as u64;
        let mut c = Cursor::new(z);
        match analyze(&mut c, 0, len).unwrap() {
            RepairAnalysis::Repairable(plan) => {
                assert_eq!(plan.patches.len(), 1);
                assert!(plan.reason.contains("中央目录") || plan.reason.contains("偏移"));
            }
            other => panic!("应当可修: {other:?}"),
        }
    }

    #[test]
    fn 带前导垃圾时偏移是相对归档起点算的() {
        let mut buf = vec![0xAAu8; 500];
        let base = buf.len() as u64;
        let z = make_zip_wrong_offset(true);
        buf.extend_from_slice(&z);
        let len = z.len() as u64;
        let mut c = Cursor::new(buf);
        match analyze(&mut c, base, len).unwrap() {
            RepairAnalysis::Repairable(p) => {
                assert!(p.fixed_cd_offset > 0 && p.fixed_cd_offset < len);
            }
            other => panic!("带前导垃圾也应可修: {other:?}"),
        }
    }
}
