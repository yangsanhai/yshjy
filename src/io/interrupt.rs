//! 可中断读取：让"取消"能立刻生效
//!
//! # 为什么需要它（见 08-三层贯通审查 B1）
//! 解压库内部的循环不受我们控制（7z 解一个大文件可能几十秒不返回），
//! 仅在外层检查取消标志会导致"用户点了取消，界面半天没反应"。
//!
//! 解法：把检查**下沉到读取层** —— 每读满 `check_every` 字节检查一次取消标志，
//! 一旦取消就返回 `io::Error(Interrupted)`，让解压库自己因为读失败而中断。

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 默认检查间隔：1 MiB
pub const DEFAULT_CHECK_INTERVAL: u64 = 1 << 20;

/// 可中断读取器
#[derive(Debug)]
pub struct InterruptibleReader<R> {
    inner: R,
    cancel: Arc<AtomicBool>,
    check_every: u64,
    since_check: u64,
}

impl<R: Read> InterruptibleReader<R> {
    /// 用给定的取消标志包装读取器（默认每 1 MiB 检查一次）
    pub fn new(inner: R, cancel: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            cancel,
            check_every: DEFAULT_CHECK_INTERVAL,
            since_check: 0,
        }
    }

    /// 自定义检查间隔（测试用）
    pub fn with_interval(inner: R, cancel: Arc<AtomicBool>, check_every: u64) -> Self {
        Self {
            inner,
            cancel,
            check_every: check_every.max(1),
            since_check: 0,
        }
    }

    /// 是否已被请求取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 取回底层读取器
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for InterruptibleReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.since_check >= self.check_every {
            self.since_check = 0;
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "cancelled by user",
                ));
            }
        }
        let n = self.inner.read(buf)?;
        self.since_check += n as u64;
        Ok(n)
    }
}

/// 创建一个"取消令牌"（外壳持有它来请求取消，引擎持有克隆来检查）
pub fn cancel_token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn 未取消时正常读取() {
        let data = b"hello world".to_vec();
        let token = cancel_token();
        let mut r = InterruptibleReader::new(Cursor::new(data), token);
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"hello world");
        assert!(!r.is_cancelled());
    }

    #[test]
    fn 取消后读取中断() {
        let data = vec![0u8; 4096];
        let token = cancel_token();
        // 每 64 字节检查一次
        let mut r = InterruptibleReader::with_interval(Cursor::new(data), token.clone(), 64);

        // 先读一批（触发一次检查，此时未取消）
        let mut buf = [0u8; 256];
        r.read_exact(&mut buf).unwrap();

        // 请求取消
        token.store(true, Ordering::Relaxed);

        // 再读应当被中断（这是"取消能生效"的关键）
        let mut more = [0u8; 256];
        let err = loop {
            match r.read(&mut more) {
                Ok(0) => panic!("应当先报中断错误"),
                Ok(_) => continue,
                Err(e) => break e,
            }
        };
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }
}
