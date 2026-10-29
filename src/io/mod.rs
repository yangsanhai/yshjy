//! I/O 抽象层
//!
//! 三个关键包装器（见 docs/ARCHITECTURE.md §5 与 08-三层贯通审查 B1/B4）：
//!
//! | 包装器 | 作用 | 应对的场景 |
//! |--------|------|------------|
//! | [`Window`] | 把文件的一段"看成"从 0 开始的独立文件 | 数据不在文件开头（伪装包） |
//! | [`SegmentReader`] | 把多个分散的数据段"虚接"成连续流 | 数据被切开分插（E3 场景） |
//! | [`InterruptibleReader`] | 可中断读取 | 用户点取消要能立刻生效 |
//! | [`PartFile`] | 先写 `.yshjy-part`，写完原子改名 | 中途失败 / 被杀不留半截正式文件 |
//! | [`BlockCache`] | 给「只能顺序读」的源套上可 seek 的外观 | 手机存储（SAF）/ 管道流 |
//!
//! 前三个都用标准库 trait 实现，**不依赖任何平台 API**，可在 Linux/Windows/Android 通用。

pub mod atomic;
pub mod blockcache;
pub mod interrupt;
pub mod patch;
pub mod segment;
pub mod window;

pub use atomic::{PartFile, PartWriter, PART_SUFFIX};
pub use blockcache::{BlockCache, 默认上限 as 块缓存上限, 默认块大小};
pub use interrupt::InterruptibleReader;
pub use segment::SegmentReader;
pub use window::Window;

/// 数据段：底层文件中的一段区间 `[start, start + len)`
///
/// 用于描述"数据分散在多处"的情况（见 04-变态场景验证）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Segment {
    /// 段在底层文件中的起始偏移
    pub start: u64,
    /// 段的长度
    pub len: u64,
}

impl Segment {
    /// 新建数据段
    pub fn new(start: u64, len: u64) -> Self {
        Self { start, len }
    }

    /// 段的结束偏移（不含）
    pub fn end(&self) -> u64 {
        self.start + self.len
    }

    /// 该位置是否落在本段内
    pub fn contains(&self, pos: u64) -> bool {
        pos >= self.start && pos < self.end()
    }

    /// 是否为空段
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 段的区间计算正确() {
        let s = Segment::new(100, 50);
        assert_eq!(s.end(), 150);
        assert!(s.contains(100));
        assert!(s.contains(149));
        assert!(!s.contains(150));
        assert!(!s.contains(99));
    }
}
