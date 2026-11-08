// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 块缓存：给「不能随便 seek」的源套上 `Read + Seek`（第三期 C）
//!
//! # 它解决什么
//!
//! 解压器（zip 尤其）是**随机访问**的：先跳到文件尾读 EOCD，再跳回目录区，
//! 再逐个跳到条目数据。普通文件随便跳；手机上的 `content://` 句柄有的只能顺序读。
//!
//! 这个适配器把「顺序流」包装成「可 seek 的文件」：
//!
//! - 按 [`默认块大小`]（32 KiB）分块，最近用过的块留在内存里（LRU，
//!   上限 [`默认上限`] 8 MiB）；
//! - 往前跳（超出已读位置）：把中间的字节读掉，只缓存需要的块；
//! - **往回跳且那块已经被 LRU 丢掉**：明确返回 `Unsupported` 错误，
//!   由调用方按 [`crate::seek::SeekAbility::CopyFirst`] 复制一份再来 ——
//!   **绝不静默返回 EOF**（那会让解压少解几个文件还不报错，是最坏的失败方式）。
//!
//! # 与全项目的关系
//!
//! 和 [`crate::io::PartFile`] 的 sink 模式同一个思路：把平台差异收在 IO 层，
//! **解码逻辑一行不改**。桌面永远走 [`crate::seek::SeekAbility::Full`]，用不到它。

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};

/// 默认块大小（32 KiB：够一次填满一个解压缓冲，又不至于让 LRU 块数太多）
pub const 默认块大小: usize = 32 * 1024;
/// 默认缓存上限（8 MiB）
pub const 默认上限: usize = 8 * 1024 * 1024;

/// 缓存里的一块
struct Block {
    /// 块号（`偏移 / block_size`）
    id: u64,
    /// 块数据（最后一块可能短于 `block_size`）
    data: Vec<u8>,
    /// 最近使用序号（越大越新）
    last: u64,
}

/// 把顺序流包装成可 seek 的读适配器
pub struct BlockCache<R> {
    inner: R,
    block_size: usize,
    max_bytes: usize,
    /// 最近使用在**尾部**
    blocks: VecDeque<Block>,
    /// 下一个要从 `inner` 顺序读出来的块号（底层流只能往前，不能退）
    next_block: u64,
    /// 逻辑读位置
    pos: u64,
    /// 总长度（`None` = 不知道，此时不支持 `SeekFrom::End`）
    len: Option<u64>,
    /// 逻辑时钟（LRU 用）
    tick: u64,
    /// 已缓存字节数（避免每块求和）
    cached_bytes: usize,
    /// 被淘汰的块数（单测断「LRU 真的丢块」用）
    dropped: u64,
    /// 底层流是否已经读到结尾
    eof: bool,
}

impl<R: Read> BlockCache<R> {
    /// 用默认块大小与上限
    pub fn new(inner: R) -> Self {
        Self::with_capacity(inner, 默认块大小, 默认上限)
    }

    /// 自定义块大小与缓存上限（上限小于一块时按一块算，否则永远存不住）
    pub fn with_capacity(inner: R, block_size: usize, max_bytes: usize) -> Self {
        let block_size = block_size.max(1);
        let max_bytes = max_bytes.max(block_size);
        Self {
            inner,
            block_size,
            max_bytes,
            blocks: VecDeque::new(),
            next_block: 0,
            pos: 0,
            len: None,
            tick: 0,
            cached_bytes: 0,
            dropped: 0,
            eof: false,
        }
    }

    /// 告诉它总长度 —— 之后 `SeekFrom::End` 才能用
    pub fn with_len(mut self, len: u64) -> Self {
        self.len = Some(len);
        self
    }

    /// 已经被丢掉（淘汰）的块数
    pub fn dropped_blocks(&self) -> u64 {
        self.dropped
    }

    /// 当前缓存着几块
    pub fn cached_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// 取回底层流（解压完可以从里面再拿走，例如换档重试）
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// 找块（命中时刷新 LRU 次序）
    fn get_mut(&mut self, id: u64) -> Option<&mut Block> {
        self.tick += 1;
        let tick = self.tick;
        let idx = self.blocks.iter().position(|b| b.id == id)?;
        let b = self.blocks.remove(idx)?;
        let mut b = b;
        b.last = tick;
        self.blocks.push_back(b);
        self.blocks.back_mut()
    }

    fn has(&self, id: u64) -> bool {
        self.blocks.iter().any(|b| b.id == id)
    }

    /// 放一块进缓存并做 LRU 淘汰
    fn insert(&mut self, id: u64, data: Vec<u8>) {
        self.tick += 1;
        let last = self.tick;
        self.cached_bytes += data.len();
        self.blocks.push_back(Block { id, data, last });
        while self.cached_bytes > self.max_bytes && self.blocks.len() > 1 {
            if let Some(old) = self.blocks.pop_front() {
                self.cached_bytes -= old.data.len();
                self.dropped += 1;
            }
        }
    }

    /// 顺序推进底层流直到「块 `id` 已经被读出来」
    fn fill_upto(&mut self, id: u64) -> io::Result<()> {
        while self.next_block <= id && !self.eof {
            let mut buf = vec![0u8; self.block_size];
            let mut got = 0usize;
            // 一次 read 可能短读，凑满一块再算（否则最后一块会被切碎）
            while got < self.block_size {
                match self.inner.read(&mut buf[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if got == 0 {
                self.eof = true;
                break;
            }
            buf.truncate(got);
            if got < self.block_size {
                self.eof = true;
            }
            self.insert(self.next_block, buf);
            self.next_block += 1;
        }
        Ok(())
    }
}

impl<R: Read> Read for BlockCache<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.len.is_some_and(|len| self.pos >= len) {
            return Ok(0);
        }
        let bs = self.block_size as u64;
        let id = self.pos / bs;
        let off = (self.pos % bs) as usize;

        if !self.has(id) {
            if id < self.next_block {
                // 这块已经被 LRU 丢掉，而底层流不能回退 —— 明确报错，不装成 EOF
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!(
                        "这一段已经被块缓存丢弃（上限 {} 字节）—— 这个位置不能跳着读，需要先复制一份本地副本",
                        self.max_bytes
                    ),
                ));
            }
            self.fill_upto(id)?;
        }
        let Some(b) = self.get_mut(id) else {
            return Ok(0); // 真的到结尾了
        };
        if off >= b.data.len() {
            return Ok(0);
        }
        let n = out.len().min(b.data.len() - off);
        out[..n].copy_from_slice(&b.data[off..off + n]);
        if let Some(len) = self.len {
            let 剩余 = (len - self.pos) as usize;
            if n > 剩余 {
                self.pos += 剩余 as u64;
                return Ok(剩余);
            }
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read> Seek for BlockCache<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let base = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => match self.len {
                Some(l) => l as i128 + d as i128,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "不知道总长度，不能从尾部定位（用 with_len 告诉它）",
                    ));
                }
            },
        };
        if base < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek 到了负数位置",
            ));
        }
        self.pos = base as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一段可重复的数据
    fn 数据(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    /// 用「整读进 Vec 再切」当标准答案
    fn 标准答案(data: &[u8], pos: usize, n: usize) -> Vec<u8> {
        let end = (pos + n).min(data.len());
        data[pos.min(data.len())..end].to_vec()
    }

    #[test]
    fn 随机_seek_read_与整读逐字节一致() {
        let data = 数据(300 * 1024 + 17); // 跨多块、且最后一块是短的
        let mut c = BlockCache::with_capacity(io::Cursor::new(data.clone()), 4096, 1 << 20);
        // 固定的伪随机序列（可复现，不用 rand 依赖）
        let mut s: u64 = 12345;
        for _ in 0..300 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let pos = (s >> 33) as usize % data.len();
            let n = ((s >> 11) as usize % 5000) + 1;
            c.seek(SeekFrom::Start(pos as u64)).unwrap();
            let mut buf = vec![0u8; n];
            let mut got = 0;
            while got < n {
                match c.read(&mut buf[got..]) {
                    Ok(0) => break,
                    Ok(k) => got += k,
                    Err(e) => panic!("read 失败: {e}"),
                }
            }
            assert_eq!(
                &buf[..got],
                标准答案(&data, pos, n).as_slice(),
                "pos={pos} n={n} 处读出来的字节不一致"
            );
        }
    }

    #[test]
    fn 超过上限真的丢块() {
        let data = 数据(12 * 1024 * 1024); // 12 MiB > 8 MiB 上限
        let mut c = BlockCache::with_capacity(io::Cursor::new(data), 默认块大小, 默认上限);
        // 顺序读到底
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0usize;
        loop {
            match c.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(total, 12 * 1024 * 1024);
        assert!(c.dropped_blocks() > 0, "12 MiB 走过 8 MiB 缓存，必须淘汰过块");
        assert!(c.cached_blocks() * 默认块大小 <= 默认上限 + 默认块大小);
    }

    #[test]
    fn 回退到已淘汰的块要报错而不是假结束() {
        let data = 数据(12 * 1024 * 1024);
        let mut c = BlockCache::with_capacity(io::Cursor::new(data), 默认块大小, 默认上限);
        let mut buf = vec![0u8; 1 << 20];
        while let Ok(n) = c.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        // 回到 0 号块：早被淘汰了
        c.seek(SeekFrom::Start(0)).unwrap();
        let mut one = [0u8; 16];
        let e = c.read(&mut one).expect_err("必须报错，不许返回 Ok(0)");
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        assert!(e.to_string().contains("复制"), "错误里要告诉用户下一步：{e}");
    }

    #[test]
    fn 不知道长度时不能从尾部定位() {
        let data = 数据(1024);
        let mut c = BlockCache::with_capacity(io::Cursor::new(data.clone()), 256, 4096);
        assert!(c.seek(SeekFrom::End(0)).is_err());
        // 告诉它长度后就能
        let mut c = c.with_len(data.len() as u64);
        assert_eq!(c.seek(SeekFrom::End(0)).unwrap(), data.len() as u64);
        // 末尾之后读是 EOF
        let mut b = [0u8; 8];
        assert_eq!(c.read(&mut b).unwrap(), 0);
    }

    #[test]
    fn with_len_会截断末尾多余字节() {
        // 底层流比声明长度长（上面加了个外层装饰），读的时候必须按声明长度截断
        let data = 数据(1000);
        let mut c = BlockCache::with_capacity(io::Cursor::new(data), 128, 4096).with_len(500);
        let mut buf = vec![0u8; 4096];
        let mut got = 0;
        loop {
            match c.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(got, 500, "声明长度之后不许读多");
    }

    #[test]
    fn 默认参数是_32k_与_8m() {
        assert_eq!(默认块大小, 32 * 1024);
        assert_eq!(默认上限, 8 * 1024 * 1024);
        let c = BlockCache::new(io::Cursor::new(数据(10)));
        assert_eq!(c.block_size, 默认块大小);
        assert_eq!(c.max_bytes, 默认上限);
    }

    #[test]
    fn 上限比一块还小时按一块算() {
        let c = BlockCache::with_capacity(io::Cursor::new(数据(10)), 4096, 100);
        assert_eq!(c.max_bytes, 4096, "上限小于一块会永远存不住东西，必须抬到一块");
    }
}
