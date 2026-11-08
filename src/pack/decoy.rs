// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 干扰数据 / 假签名（第三期 B）
//!
//! 目的：让产物像业界隐写工具（隐写者 / Steganographier）做出来的那样「难啃」——
//! 扫描器看到的是一堆 `Rar!` / `PK\x03\x04` 特征码和随机字节，不知道哪个是真的。
//!
//! # 铁律：噪声只在压缩数据之外
//!
//! - **L1 尾部追加**：写在「封面之后、ZIP 起点之前」；
//! - **L2 free 原子**：写在「原子内的 pad 之后、ZIP 起点之前」。
//!
//! 绝不写进 MP4 box 本体（会破坏播放），也绝不写进压缩流内部（会破坏解压）。
//! 位置由 `pack::build_ctx_decoy` 负责，本模块只管「造字节」和「算种子」。
//!
//! # 可复现（与隐写者的「每次哈希都变」相反）
//!
//! 随机量全部来自 [`种子`]（输入路径 + 布局盐）派生的 xorshift，**不碰系统时间**：
//! 同一份输入打两次，产物逐字节一致 —— 用户要能校验自己下载/分享的东西。
//! 这也是 L2 pad 在 `--decoy` 开启时改用稳定种子的原因（否则整包哈希每次都不一样）。
//!
//! # 假签名的填充：为什么签名后面是 30 个零（实测结论，别改回纯随机）
//!
//! 2026-10-29 实测（`~/.hermes/cache/scratch/decoy_probe`，五种填充 × 20 个种子 × 2 种长度）：
//!
//! | 段内填充 | 7-Zip 列不出内容的次数 / 40 |
//! |---|---|
//! | 全零 | 0 |
//! | 签名 + 30 字节零 + 其余随机 | **0** |
//! | 签名 + 40 字节零 + 其余随机 | 0 |
//! | 签名 + 64 字节随机、其余零 | **17** |
//!
//! 也就是说「假签名后面紧跟随机字节」会让 7-Zip **随机性地**把那个候选当成真的 ZIP 起点，
//! 于是整个文件列不出内容（`unzip` / 我们自己的引擎不受影响）。30 个零正好是一个
//! 「匿名空条目」local header 的形状，7-Zip 会确定性地跳过它。
//!
//! 代价说明：`zshyj` 的验收要求「7z / unzip 都还能列内容」，所以这里选确定性的填充，
//! 不选「更像真包但会随机把用户的 7-Zip 搞坏」的纯随机。

use std::io::Write;
use std::path::PathBuf;

use crate::error::{Error, ErrorCode, Result};

/// 段数上限（再多样品就没意义了，只会让产物膨胀）
pub const 段数上限: usize = 64;
/// 单段最小字节数
pub const 单段最小: usize = 4 * 1024;
/// 单段最大字节数
pub const 单段最大: usize = 1024 * 1024;
/// 噪声总量硬上限
///
/// 方案原文写的是「≤ 外壳大小的 10%」，但内置封面只有 1688 字节 —— 10% 是 168 字节，
/// 连一段（最小 4 KiB）都放不下，功能等于不可用。实测（`decoy_probe`）噪声长度对
/// `file` 的判定、7-Zip / unzip 的列目录能力都**没有影响**（1 MiB 也全通过），
/// 所以这里改成绝对上限 1 MiB：够糊扫描器，又不会把产物的体积搞成以噪声为主。
pub const 总量上限: u64 = 1024 * 1024;

/// 假特征码候选集（与 `refs/tech/sevenz-rust+steganographier.md` 里隐写者撒的一致）
const 假签名: [&[u8]; 5] = [
    b"Rar!",
    b"PK\x03\x04",
    b"\x1f\x8b\x08",
    b"BZh",
    b"7z\xbc\xaf\x27\x1c",
];

/// 签名后面必须跟的零字节数（见模块头实测表）
const 签名后零长: usize = 30;

/// 干扰数据选项（`--decoy` / `--decoy-size` / `--decoy-signatures`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoyOptions {
    /// 撒几段（0 = 不打）
    pub count: usize,
    /// 每段字节数
    pub size: usize,
    /// 每段里夹一枚假特征码
    pub signatures: bool,
}

impl Default for DecoyOptions {
    fn default() -> Self {
        Self {
            count: 0,
            size: 16 * 1024,
            signatures: false,
        }
    }
}

impl DecoyOptions {
    /// 完全不打干扰（默认）
    pub fn none() -> Self {
        Self::default()
    }

    /// 是否真的会写噪声
    pub fn is_on(&self) -> bool {
        self.count > 0 && self.size > 0
    }

    /// 噪声总字节数（写进 `PackReport`，也是进度分母的一部分）
    pub fn total(&self) -> u64 {
        if self.is_on() {
            self.count as u64 * self.size as u64
        } else {
            0
        }
    }

    /// 校验参数范围 —— 越界**明确报错**，不静默截断（D20 教训）
    pub fn validate(&self) -> Result<()> {
        if self.count > 段数上限 {
            return Err(Error::new(ErrorCode::InvalidArgument).context(format!(
                "--decoy 最多 {} 段（收到 {}）",
                段数上限, self.count
            )));
        }
        if self.count > 0 && !(单段最小..=单段最大).contains(&self.size) {
            return Err(Error::new(ErrorCode::InvalidArgument).context(format!(
                "--decoy-size 只能是 4K–1M（收到 {} 字节）",
                self.size
            )));
        }
        if self.total() > 总量上限 {
            return Err(Error::new(ErrorCode::InvalidArgument).context(format!(
                "干扰数据一共 {} 字节，超过上限 {} 字节（{} 段 × {} 字节）",
                self.total(),
                总量上限,
                self.count,
                self.size
            )));
        }
        Ok(())
    }
}

/// 从输入路径算一个**稳定**种子（同一份输入 → 同一个种子 → 同一份产物）
///
/// FNV-1a 64。**不用 `std::hash::DefaultHasher`** —— 它的输出不保证跨版本稳定，
/// 且 `refs/analysis/05` 踩过 Python `hash()` 受 `PYTHONHASHSEED` 影响的同款坑。
pub fn 种子(输入: &[PathBuf], 盐: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for p in 输入 {
        for b in p.to_string_lossy().as_bytes() {
            mix(*b);
        }
        // 路径之间插分隔符，避免 ["ab","c"] 与 ["a","bc"] 撞种子
        mix(0xff);
    }
    for b in 盐.to_le_bytes() {
        mix(b);
    }
    // 兜底：xorshift 的 0 状态是退化的
    if h == 0 { 0x9E37_79B9_7F4A_7C15 } else { h }
}

/// 造一段干扰数据
///
/// `seed` 由 [`种子`] 派生（含段号），`size` 已由调用方校验过范围。
/// 返回的 Vec 最大 1 MiB —— 方案 §1.3 的资源红线是「不许整包读进内存」，
/// 单段一 MiB 的临时缓冲不违反它。
pub fn 造一段(seed: u64, size: usize, signatures: bool) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    let mut x = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
    for b in buf.iter_mut() {
        // xorshift64：够用、零依赖、跨平台一致
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x >> 24) as u8;
    }
    if signatures && size >= 假签名[1].len() + 签名后零长 + 16 {
        let sig = 假签名[(seed >> 11) as usize % 假签名.len()];
        // 放在段中部、8 字节对齐：既不是段首（free 原子/pad 的规则管不到这里），
        // 也不会紧贴 ZIP 起点
        let mut pos = (size / 2) & !7usize;
        if pos + sig.len() + 签名后零长 > size {
            pos = size - sig.len() - 签名后零长;
        }
        buf[pos..pos + sig.len()].copy_from_slice(sig);
        for b in buf[pos + sig.len()..pos + sig.len() + 签名后零长].iter_mut() {
            *b = 0;
        }
    }
    buf
}

/// 把 `count` 段噪声全部写出，返回写入的字节数（= `opts.total()`）
///
/// 逐段生成、逐段写 —— 不会把全部噪声攒在内存里。
pub fn 写噪声<W: Write>(out: &mut W, opts: &DecoyOptions, base_seed: u64) -> Result<u64> {
    if !opts.is_on() {
        return Ok(0);
    }
    opts.validate()?;
    let mut 已写 = 0u64;
    for i in 0..opts.count {
        // 每段独立种子：同一包两次打包逐字节一致，但段与段之间不重复
        let s = 种子(&[], base_seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let 段 = 造一段(s, opts.size, opts.signatures);
        out.write_all(&段)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("写干扰数据: {e}")))?;
        已写 += 段.len() as u64;
    }
    Ok(已写)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认不打() {
        let o = DecoyOptions::default();
        assert!(!o.is_on());
        assert_eq!(o.total(), 0);
        assert!(o.validate().is_ok());
        assert_eq!(o.size, 16 * 1024, "默认每段 16K（方案 §B 决策 1）");
        assert!(!o.signatures, "默认不撒假签名");
    }

    #[test]
    fn 越界明确报错() {
        // 段数超限
        let o = DecoyOptions { count: 65, size: 4096, signatures: false };
        assert!(o.validate().is_err());
        // 段太小 / 太大
        for size in [1024usize, 2 * 1024 * 1024] {
            let o = DecoyOptions { count: 1, size, signatures: false };
            assert!(o.validate().is_err(), "size={size} 应被拒");
        }
        // 总量超 1 MiB
        let o = DecoyOptions { count: 64, size: 32 * 1024, signatures: false };
        assert!(o.validate().is_err(), "64×32K=2MiB 应被拒");
        // 边界值都放行
        let ok = DecoyOptions { count: 64, size: 16 * 1024, signatures: true };
        assert!(ok.validate().is_ok(), "64×16K=1MiB 正好在上限上");
    }

    #[test]
    fn 种子稳定且与段号无关地复用() {
        let a = vec![PathBuf::from("/tmp/包 1.mp4")];
        let b = vec![PathBuf::from("/tmp/包 2.mp4")];
        assert_eq!(种子(&a, 7), 种子(&a, 7), "同一输入必须同一个种子");
        assert_ne!(种子(&a, 7), 种子(&b, 7), "不同输入要不同种子");
        assert_ne!(种子(&a, 7), 种子(&a, 8), "盐不同要不同种子");
        // 拼接撞种子：["ab","c"] 与 ["a","bc"] 不许相等
        let x = vec![PathBuf::from("ab"), PathBuf::from("c")];
        let y = vec![PathBuf::from("a"), PathBuf::from("bc")];
        assert_ne!(种子(&x, 0), 种子(&y, 0));
    }

    #[test]
    fn 同一段两次生成逐字节一致() {
        let s = 种子(&[PathBuf::from("/tmp/x.mp4")], 1);
        assert_eq!(造一段(s, 16384, true), 造一段(s, 16384, true));
        // 换种子就该不一样（否则等于没随机）
        assert_ne!(造一段(s, 16384, true), 造一段(s ^ 1, 16384, true));
    }

    #[test]
    fn 假签名确实在里面且后面是零() {
        let s = 种子(&[PathBuf::from("/tmp/y.mp4")], 3);
        let seg = 造一段(s, 16 * 1024, true);
        let sig = 假签名[(s >> 11) as usize % 假签名.len()];
        let pos = seg
            .windows(sig.len())
            .position(|w| w == sig)
            .expect("开了 --decoy-signatures 就必须能找到假签名");
        // 签名后 30 字节必须是零（实测结论：写随机会把 7-Zip 随机性骗坏）
        assert!(
            seg[pos + sig.len()..pos + sig.len() + 签名后零长].iter().all(|b| *b == 0),
            "假签名后必须跟 30 个零字节"
        );
        // 段里不能整段为零（那就不是「干扰数据」了）
        assert!(seg.iter().any(|b| *b != 0));
    }

    #[test]
    fn 关掉签名就一个特征码都没有() {
        let s = 种子(&[PathBuf::from("/tmp/z.mp4")], 5);
        let seg = 造一段(s, 16 * 1024, false);
        for sig in 假签名 {
            // 极小概率随机撞上，允许命中但要求少于 1 处以外的情况可忽略；
            // 这里用固定种子，实测为 0 命中，钉死行为
            assert!(
                seg.windows(sig.len()).filter(|w| *w == sig).count() == 0,
                "--decoy-signatures off 时不该出现 {:?}",
                sig
            );
        }
    }

    #[test]
    fn 写噪声总字节数与段数一致() {
        let o = DecoyOptions { count: 3, size: 4096, signatures: true };
        let mut out: Vec<u8> = Vec::new();
        let n = 写噪声(&mut out, &o, 12345).expect("写噪声");
        assert_eq!(n, 3 * 4096);
        assert_eq!(out.len(), 3 * 4096);
        // 段与段不同（否则一眼看出是重复块）
        assert_ne!(&out[..4096], &out[4096..8192]);
    }

    #[test]
    fn 不打时一个字节都不写() {
        let mut out: Vec<u8> = Vec::new();
        let n = 写噪声(&mut out, &DecoyOptions::none(), 1).expect("不打");
        assert_eq!(n, 0);
        assert!(out.is_empty());
    }
}
