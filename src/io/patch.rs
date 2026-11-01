//! 补丁窗口 —— 读取时「就地改字节」，**绝不改动磁盘上的文件**
//!
//! # 用途
//! 有些坏包（尤其是伪装包）本身数据是好的，只是**头部记的几个偏移字段写错了**，
//! 导致所有读包工具都找不到中央目录。这时最省事的办法不是重写解压器，
//! 而是在**内存里把错的那几个字节改成正确值**，然后交给成熟的解码器 ——
//! 这正是 zip2 源码里最值得借鉴的一招（改 8 字节即白嫖它的全部解压能力）。
//!
//! 我们在此基础上更进一层：偏移到底该改成多少，由 [`crate::repair`] 分析后决定。
//!
//! # 与 [`super::Window`] 的区别
//! - `Window`：把 `[base, base+len)` 这段当成一个独立文件
//! - `PatchWindow`：同上，**外加**读取时替换若干处字节

use std::io::{self, Read, Seek, SeekFrom};

/// 一处补丁：从 `at`（相对归档起点的偏移）开始替换成 `bytes`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// 相对归档起点的偏移
    pub at: u64,
    /// 替换成的字节
    pub bytes: Vec<u8>,
}

impl Patch {
    /// 构造一处补丁
    pub fn new(at: u64, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            at,
            bytes: bytes.into(),
        }
    }
    /// 覆盖的字节范围 `[at, at + len)`
    fn end(&self) -> u64 {
        self.at + self.bytes.len() as u64
    }
}

/// 带补丁的偏移窗口
pub struct PatchWindow<R> {
    inner: R,
    /// 归档起点在原始流中的绝对偏移
    base: u64,
    /// 窗口长度
    len: u64,
    /// 当前逻辑位置（相对 base）
    pos: u64,
    /// 补丁表（按 `at` 排序）
    patches: Vec<Patch>,
}

impl<R: Read + Seek> PatchWindow<R> {
    /// 在 `[base, base+len)` 这段上开一个带补丁的窗口
    pub fn new(mut inner: R, base: u64, len: u64, mut patches: Vec<Patch>) -> io::Result<Self> {
        patches.sort_by_key(|p| p.at);
        inner.seek(SeekFrom::Start(base))?;
        Ok(Self {
            inner,
            base,
            len,
            pos: 0,
            patches,
        })
    }

    /// 窗口长度
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<R: Read + Seek> Read for PatchWindow<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let want = buf.len().min((self.len - self.pos) as usize);
        self.inner.seek(SeekFrom::Start(self.base + self.pos))?;
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Ok(0);
        }
        let read_start = self.pos;
        let read_end = self.pos + n as u64;

        // 把落在本次读取范围内的补丁贴上去
        for p in &self.patches {
            if p.end() <= read_start || p.at >= read_end {
                continue;
            }
            for (i, b) in p.bytes.iter().enumerate() {
                let abs = p.at + i as u64;
                if abs >= read_start && abs < read_end {
                    buf[(abs - read_start) as usize] = *b;
                }
            }
        }

        self.pos = read_end;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for PatchWindow<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target: i128 = match from {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(p) => self.len as i128 + p as i128,
            SeekFrom::Current(p) => self.pos as i128 + p as i128,
        };
        if target < 0 || target > self.len as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PatchWindow: seek 越界",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn data() -> Vec<u8> {
        let mut v = vec![0xAAu8; 100]; // 前导垃圾
        v.extend_from_slice(b"HELLO WORLD THIS IS A TEST PAYLOAD"); // 27 字节，从 100 开始
        v.extend(std::iter::repeat_n(0xBBu8, 50));
        v
    }

    #[test]
    fn 不打补丁时与原文一致() {
        let d = data();
        let len = d.len() as u64 - 100;
        let mut w = PatchWindow::new(Cursor::new(d.clone()), 100, len, vec![]).unwrap();
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut w, &mut out).unwrap();
        assert_eq!(&out[..], &d[100..]);
    }

    #[test]
    fn 补丁生效且不动原数据() {
        let d = data();
        let len = d.len() as u64 - 100;
        // 把 "HELLO" 改成 "HELLX"
        let patch = Patch::new(0, b"HELLX".to_vec());
        let mut w = PatchWindow::new(Cursor::new(d.clone()), 100, len, vec![patch]).unwrap();
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut w, &mut out).unwrap();
        assert_eq!(&out[..5], b"HELLX");
        assert_eq!(
            &out[5..34],
            b" WORLD THIS IS A TEST PAYLOAD",
            "补丁后正文要对"
        );
        // 原始数据没被改
        assert_eq!(&d[100..105], b"HELLO");
    }

    #[test]
    fn 分块读取也能贴上跨块补丁() {
        let d = data();
        let len = d.len() as u64 - 100;
        // 跨在第 4..8 字节
        let patch = Patch::new(4, b"YYYY".to_vec());
        let mut w = PatchWindow::new(Cursor::new(d.clone()), 100, len, vec![patch]).unwrap();
        // 每次只读 1 字节（模拟小块读取）
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match std::io::Read::read(&mut w, &mut byte) {
                Ok(0) => break,
                Ok(_) => out.push(byte[0]),
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(
            &out[4..8],
            b"YYYY",
            "跨块补丁必须正确贴上: {:?}",
            &out[..10]
        );
    }

    #[test]
    fn seek_是相对窗口的() {
        let d = data();
        let len = d.len() as u64 - 100;
        let mut w = PatchWindow::new(Cursor::new(d.clone()), 100, len, vec![]).unwrap();
        assert_eq!(w.seek(SeekFrom::Start(6)).unwrap(), 6); // 相对 base
        let mut b = [0u8; 5];
        std::io::Read::read_exact(&mut w, &mut b).unwrap();
        assert_eq!(&b, b"WORLD");
        assert_eq!(w.seek(SeekFrom::End(0)).unwrap(), len);
    }

    #[test]
    fn 越界_seek_报错() {
        let d = data();
        let len = d.len() as u64 - 100;
        let mut w = PatchWindow::new(Cursor::new(d), 100, len, vec![]).unwrap();
        assert!(w.seek(SeekFrom::Start(len + 10)).is_err());
    }
}
