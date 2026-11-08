// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 分段虚接：把多个分散的数据段"拼"成一条连续流
//!
//! # 应对的场景（见 04-变态场景验证 E3）
//! 压缩包被视频切成两段：`[包前半][视频][包后半]`。
//! 两段数据都在，只是被隔开了 —— 把它们"虚接"起来就能正常解压。
//!
//! # 性能设计（见 08-三层贯通审查 B4）
//! - 构造时建立**段索引表**（按起始偏移排序 + 累计长度），seek 用二分查找定位
//! - 顺序读取时记录当前段与段内指针，避免重复定位

use super::Segment;
use std::io::{self, Read, Seek, SeekFrom};

/// 多段虚接读取器
#[derive(Debug)]
pub struct SegmentReader<R> {
    inner: R,
    /// 按 `start` 升序排列的段
    segments: Vec<Segment>,
    /// `ends[i]` = 前 i+1 个段的逻辑结束位置（用于二分查找）
    ends: Vec<u64>,
    /// 逻辑总长度
    total: u64,
    /// 当前逻辑位置
    pos: u64,
    /// 当前所在段（顺序读取缓存）
    cur: usize,
}

impl<R: Read + Seek> SegmentReader<R> {
    /// 用一组数据段构造（段会被自动排序并校验）
    ///
    /// 返回错误的情况：段列表为空、存在空段、或段区间重叠。
    pub fn new(mut inner: R, mut segments: Vec<Segment>) -> io::Result<Self> {
        if segments.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no segments given",
            ));
        }
        if segments.iter().any(|s| s.is_empty()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty segment"));
        }
        segments.sort_by_key(|s| s.start);

        // 重叠检查（重叠意味着数据来源不明确，宁可报错也不猜）
        for w in segments.windows(2) {
            if w[1].start < w[0].end() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "overlapping segments",
                ));
            }
        }

        let mut ends = Vec::with_capacity(segments.len());
        let mut acc = 0u64;
        for s in &segments {
            acc += s.len;
            ends.push(acc);
        }

        // 预定位到第一段，避免首次读取前的多余 seek
        inner.seek(SeekFrom::Start(segments[0].start))?;

        Ok(Self {
            inner,
            segments,
            ends,
            total: acc,
            pos: 0,
            cur: 0,
        })
    }

    /// 逻辑总长度（各段之和）
    pub fn len(&self) -> u64 {
        self.total
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 当前逻辑位置
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// 段的原始信息（诊断用）
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// 用二分查找定位逻辑位置所在的段
    ///
    /// 返回 `(段索引, 段内偏移)`；位置越过末尾时返回 `None`。
    fn locate(&self, pos: u64) -> Option<(usize, u64)> {
        if pos >= self.total {
            return None;
        }
        // partition_point：第一个 ends[i] > pos
        let idx = self.ends.partition_point(|&end| end <= pos);
        let base = if idx == 0 { 0 } else { self.ends[idx - 1] };
        Some((idx, pos - base))
    }

    /// 把底层读取器定位到"逻辑位置 pos"对应的物理位置
    fn seek_logical(&mut self, pos: u64) -> io::Result<()> {
        if pos > self.total {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek beyond virtual end",
            ));
        }
        if let Some((idx, off)) = self.locate(pos.min(self.total.saturating_sub(1))) {
            if pos == self.total {
                // 定位到物理末尾（合法，用于 seek(End(0))）
                let last = self.segments.last().expect("non-empty by construction");
                self.inner.seek(SeekFrom::Start(last.end()))?;
                self.cur = self.segments.len() - 1;
            } else {
                let seg = self.segments[idx];
                self.inner.seek(SeekFrom::Start(seg.start + off))?;
                self.cur = idx;
            }
        }
        self.pos = pos;
        Ok(())
    }
}

impl<R: Read + Seek> Read for SegmentReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.total || buf.is_empty() {
            return Ok(0);
        }

        let mut filled = 0usize;
        while filled < buf.len() && self.pos < self.total {
            let (idx, off) = match self.locate(self.pos) {
                Some(v) => v,
                None => break,
            };
            let seg = self.segments[idx];

            // 若跨段，先把当前段读完，下一轮循环自然切到下一段
            let seg_remain = seg.len - off;
            let want = std::cmp::min((buf.len() - filled) as u64, seg_remain) as usize;

            // 确保底层停在正确位置（顺序读时通常无需 seek，命中缓存）
            if idx != self.cur {
                self.inner.seek(SeekFrom::Start(seg.start + off))?;
                self.cur = idx;
            }

            let n = self.inner.read(&mut buf[filled..filled + want])?;
            if n == 0 {
                break; // 底层提前结束
            }
            filled += n;
            self.pos += n as u64;
        }

        Ok(filled)
    }
}

impl<R: Read + Seek> Seek for SegmentReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => self.total as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "negative seek"));
        }
        self.seek_logical(target as u64)?;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn 两段虚接后内容正确() {
        // 底层：[A(0..4)][B(4..8)][C(8..12)]，只取 A 与 C
        let data = b"AAAABBBBCCCC".to_vec();
        let segs = vec![Segment::new(0, 4), Segment::new(8, 4)];
        let mut r = SegmentReader::new(Cursor::new(data), segs).unwrap();
        assert_eq!(r.len(), 8);

        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"AAAACCCC");
    }

    #[test]
    fn 段自动排序() {
        let data = b"AAAABBBBCCCC".to_vec();
        // 故意反序传入
        let segs = vec![Segment::new(8, 4), Segment::new(0, 4)];
        let mut r = SegmentReader::new(Cursor::new(data), segs).unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"AAAACCCC", "应按物理偏移排序后拼接");
    }

    #[test]
    fn 支持随机定位() {
        let data = b"AAAABBBBCCCCDDDD".to_vec();
        let segs = vec![Segment::new(0, 4), Segment::new(8, 8)]; // AAAA + CCCCDDDD
        let mut r = SegmentReader::new(Cursor::new(data), segs).unwrap();
        assert_eq!(r.len(), 12);

        r.seek(SeekFrom::Start(6)).unwrap();
        let mut b = [0u8; 4];
        r.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"CCDD");

        r.seek(SeekFrom::End(-2)).unwrap();
        r.read_exact(&mut b[..2]).unwrap();
        assert_eq!(&b[..2], b"DD");
    }

    #[test]
    fn 拒绝空段与重叠段() {
        let data = b"0123456789".to_vec();
        assert!(SegmentReader::new(Cursor::new(data.clone()), vec![]).is_err());
        assert!(SegmentReader::new(Cursor::new(data.clone()), vec![Segment::new(0, 0)]).is_err());
        assert!(
            SegmentReader::new(
                Cursor::new(data),
                vec![Segment::new(0, 6), Segment::new(4, 4)]
            )
            .is_err(),
            "重叠段必须报错而不是猜数据来源"
        );
    }

    #[test]
    fn 单段等价于窗口() {
        let data = b"0123456789".to_vec();
        let mut r = SegmentReader::new(Cursor::new(data), vec![Segment::new(3, 4)]).unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"3456");
    }
}
