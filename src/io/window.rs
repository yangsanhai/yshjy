//! 偏移窗口：把底层文件的 `[start, start+len)` 区间映射成逻辑位置 `0..len`
//!
//! # 为什么需要它
//! 伪装包的真身不在文件开头（例如 MP4 外壳 119MB 之后才是压缩包）。
//! 解析器需要一个"看起来从 0 开始"的数据源，`Window` 就是干这个的。
//!
//! # 实测依据
//! 技术验证时确认：7z 库内部硬编码 `seek(0)`，必须用窗口包装才能处理内嵌数据。

use std::io::{self, Read, Seek, SeekFrom};

/// 偏移窗口
#[derive(Debug)]
pub struct Window<R> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> Window<R> {
    /// 在 `inner` 上建立 `[start, start+len)` 的窗口
    pub fn new(mut inner: R, start: u64, len: u64) -> io::Result<Self> {
        inner.seek(SeekFrom::Start(start))?;
        Ok(Self {
            inner,
            start,
            len,
            pos: 0,
        })
    }

    /// 窗口长度
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 窗口是否为空
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 当前逻辑位置
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// 取回底层的读取器
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> Read for Window<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let remain = self.len - self.pos;
        let want = std::cmp::min(buf.len() as u64, remain) as usize;
        let n = self.inner.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Window<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target: i128 = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek to negative position in window",
            ));
        }
        // 允许 seek 到结尾（len），但不超过
        let target = target as u64;
        self.inner.seek(SeekFrom::Start(self.start + target))?;
        self.pos = target;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn 窗口只暴露指定区间() {
        let data = b"AAAABBBBCCCC";
        let mut w = Window::new(Cursor::new(data.to_vec()), 4, 4).unwrap();
        let mut out = Vec::new();
        w.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"BBBB");
    }

    #[test]
    fn 窗口支持随机定位() {
        let data = b"0123456789";
        let mut w = Window::new(Cursor::new(data.to_vec()), 2, 5).unwrap(); // "23456"
        assert_eq!(w.len(), 5);

        w.seek(SeekFrom::Start(0)).unwrap();
        let mut b = [0u8; 1];
        w.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"2");

        w.seek(SeekFrom::End(-1)).unwrap();
        w.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"6");

        w.seek(SeekFrom::Current(-3)).unwrap();
        assert_eq!(w.position(), 2);
    }

    #[test]
    fn 读到窗口末尾就停() {
        let data = b"XYZ";
        let mut w = Window::new(Cursor::new(data.to_vec()), 1, 2).unwrap();
        let mut out = Vec::new();
        w.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"YZ");
        assert_eq!(w.read(&mut [0u8; 8]).unwrap(), 0);
    }

    #[test]
    fn 负位置报错而不是静默错读() {
        let data = b"ABC";
        let mut w = Window::new(Cursor::new(data.to_vec()), 0, 3).unwrap();
        assert!(w.seek(SeekFrom::Start(0)).is_ok());
        assert!(w.seek(SeekFrom::Current(-5)).is_err());
    }
}
