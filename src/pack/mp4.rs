// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! MP4（ISO BMFF）原子解析 —— 只做打包布局 L4 需要的那一小块
//!
//! # 这个模块只回答三个问题
//! 1. 封面里的 `mdat`（媒体数据段）和 `moov`（索引段）在哪
//! 2. 所有轨的 `stco` / `co64`（chunk 偏移表）在哪、各有几条
//! 3. 把一段字节插进 `mdat` 的**两条 chunk 之间**之后，文件要怎么改才还能播
//!
//! # 为什么必须同时改两处（实测结论，不是推论）
//!
//! 见 `refs/analysis/05-压缩布局验证.md` §4 的对照实验（2026-10-16，现造现测）：
//!
//! | 做到哪一步 | `ffmpeg -v error -f null` 的结果 |
//! |---|---|
//! | 只在 mdat 里插 ZIP，什么都不改 | rc=183，打不开 |
//! | 只改 `mdat` 的 size | rc=0，但 stderr 满屏 `Decoding error`（有坏帧） |
//! | 只补 `stco` 偏移 | rc=183（走不到 moov） |
//! | 插在 chunk 内部 + 两边都改 | rc=0，但 `marker does not match f_code`（画面损坏） |
//! | **插在 chunk 边界 + 两边都改** | **rc=0 且 stderr 为空 —— 干净** |
//!
//! 最后一行就是本模块 [`插在中间`] 做的事：`mdat.size += 插入长度`、所有
//! `>= 插点` 的 `stco`/`co64` 条目 `+= 插入长度`、插点只从 chunk 起点里挑。
//!
//! ★ 由此得出一条验收铁律：**`ffmpeg rc=0` 不等于画面没坏**。上面两行都是 rc=0。
//! 自动化测试必须判「rc==0 且 stderr 为空」（`tests/l4_proto.py` 就是这么写的）。
//!
//! # 为什么不解析 `stsz` / `stsc`
//!
//! 严格判定「插点不切断任何一条 chunk」要用 `stsz`（每个 sample 字节数）+ `stsc`
//! （每 chunk 几个 sample）算出每条 chunk 的真实区间。这里选了更简单的做法：
//! **插点只从「所有轨的 chunk 起点并集」里取**。
//!
//! 依据：MP4 规范不允许同一文件里的 chunk 数据互相重叠；交错写入时一条 chunk 的
//! 终点就是下一条 chunk 的起点，所以「并集里每个点」天然落在 chunk 边界上。
//! 2026-10-31 用 `stsz`+`stsc`+`stco` 把三个真实封面的每条 chunk 区间都算出来核过
//! （`cover5.mp4` 带音轨 101 条、`cover5_fast.mp4` faststart 101 条、`cover5_vonly.mp4`
//! 单轨 1 条）：区间零重叠，参考插点跨越 0 条 chunk。少解析两张表 = 少两种坏结构。

/// 一个 ISO BMFF box 的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct 盒子 {
    /// 相对文件头的起点
    pub 偏移: usize,
    /// 总长（含头）
    pub 大小: usize,
    /// 头长：通常 8；`size == 1` 时是 16（后跟 8 字节 large size）
    pub 头长: usize,
    /// 类型（`ftyp` / `mdat` / `moov` …）
    pub 类型: [u8; 4],
}

impl 盒子 {
    /// 载荷（头之后）起点
    pub fn 载荷起点(&self) -> usize {
        self.偏移 + self.头长
    }

    /// 盒子末尾（不含）
    pub fn 末尾(&self) -> usize {
        self.偏移 + self.大小
    }
}

/// 能往下递归的容器盒（`stco` 就藏在这些里面）
const 容器: [[u8; 4]; 8] = [
    *b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"udta", *b"edts", *b"mvex",
];

/// 顺序切出 `[起点, 终点)` 里的同级盒子
///
/// 结构坏掉（长度越界 / 小于头长 / 走到一半不够 8 字节）就**停在最后一个好盒子上**，
/// 不 panic —— 封面是用户随便挑的文件，什么都可能。
fn 顺序盒子(data: &[u8], 起点: usize, 终点: usize) -> Vec<盒子> {
    let mut out = Vec::new();
    let mut p = 起点;
    let 终点 = 终点.min(data.len());
    while p + 8 <= 终点 {
        let 声明 = u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as u64;
        let 类型 = [data[p + 4], data[p + 5], data[p + 6], data[p + 7]];
        let (大小, 头长) = match 声明 {
            // 1 = 真长度在头后面的 8 字节里
            1 => {
                if p + 16 > 终点 {
                    break;
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[p + 8..p + 16]);
                (u64::from_be_bytes(b), 16usize)
            }
            // 0 = 一直到文件尾（只对最后一个盒子合法）
            0 => ((终点 - p) as u64, 8),
            n => (n, 8),
        };
        if 大小 < 头长 as u64 || 大小 > (终点 - p) as u64 {
            break;
        }
        out.push(盒子 {
            偏移: p,
            大小: 大小 as usize,
            头长,
            类型,
        });
        p += 大小 as usize;
    }
    out
}

/// 顶层盒子（从文件头开始切）
pub fn 顶层盒子(data: &[u8]) -> Vec<盒子> {
    顺序盒子(data, 0, data.len())
}

/// 一张 chunk 偏移表（`stco` 或 `co64`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk表 {
    /// 第一条目的第一个字节（= 盒子载荷 + 8：跳过 version/flags 4 字节和 entry_count 4 字节）
    pub 条目区: usize,
    /// 条目个数
    pub 数量: usize,
    /// 每条 4 字节（`stco`）或 8 字节（`co64`）
    pub 宽: usize,
}

/// 在 `moov` 里递归找所有 `stco` / `co64`
///
/// 一张表 = 一条轨（video / audio 各一张）。返回顺序就是它们在文件里的顺序。
pub fn 找chunk表(data: &[u8], moov: 盒子) -> Vec<Chunk表> {
    let mut out = Vec::new();
    递归找表(data, moov.载荷起点(), moov.末尾(), &mut out);
    out
}

fn 递归找表(data: &[u8], 起点: usize, 终点: usize, out: &mut Vec<Chunk表>) {
    for b in 顺序盒子(data, 起点, 终点) {
        if &b.类型 == b"stco" || &b.类型 == b"co64" {
            let 宽 = if &b.类型 == b"stco" { 4 } else { 8 };
            // 布局：size(4) type(4) version+flags(4) entry_count(4) entries…
            let 条目区 = b.载荷起点() + 8;
            let Some(数量) = 读偏移(data, b.载荷起点() + 4, 4) else {
                continue;
            };
            let 数量 = 数量 as usize;
            // 表声明得比盒子还长 = 坏结构，整张表跳过（宁可不插，也不写出坏文件）
            if 条目区 > b.末尾()
                || 数量.saturating_mul(宽) > b.末尾().saturating_sub(条目区)
            {
                continue;
            }
            out.push(Chunk表 { 条目区, 数量, 宽 });
        } else if 容器.iter().any(|t| t == &b.类型) {
            递归找表(data, b.载荷起点(), b.末尾(), out);
        }
    }
}

/// 做不到「插在视频中间」的原因 —— 调用方据此降级到别的布局
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)] // 中文 + 英文缩写混排，rustc 的驼峰检查只会误报
pub enum 不能插 {
    /// 封面不是 MP4（前 8 字节没有 `ftyp`）
    不是MP4,
    /// 顶层找不到 `mdat`
    没有MDAT,
    /// 顶层找不到 `moov`
    没有MOOV,
    /// `moov` 里没有 `stco` / `co64`（分片 MP4 用的是 `moof`+`trun`）
    没有chunk表,
    /// 表里有偏移，但没有一条落在 `mdat` 载荷区内
    mdat里没有chunk起点,
    /// 插入后 `stco` 的 32 位偏移会溢出
    偏移超出32位,
    /// 插入后 `mdat` 的 32 位 size 会溢出
    尺寸超出32位,
}

impl 不能插 {
    /// 给用户看的一句话（进 `warnings`，由外壳展示）
    pub fn 说明(self) -> &'static str {
        match self {
            不能插::不是MP4 => "封面不是 MP4，没法把数据插进视频中间",
            不能插::没有MDAT => "封面里找不到视频数据段（mdat），没法插在中间",
            不能插::没有MOOV => "封面里找不到索引段（moov），没法改播放位置",
            不能插::没有chunk表 => "封面里没有 chunk 偏移表（分片 MP4？），没法插在中间",
            不能插::mdat里没有chunk起点 => "视频数据段里找不到可用的插入点，没法插在中间",
            不能插::偏移超出32位 => {
                "包太大：插进去会让播放位置超出 4 GiB 上限，这种布局装不下"
            }
            不能插::尺寸超出32位 => "包太大：视频数据段装不下（超过 4 GiB），这种布局装不下",
        }
    }

    /// 同 [`不能插::说明`]，但用于 L3「放在文件头部」的降级提示
    ///
    /// 为什么不能复用上面那句：那里写的是「插在视频中间」，而 L3 根本不往 `mdat` 里插，
    /// 照抄会让用户看到一句与自己所选布局不符的话（T18-1 的教训：降级必须说出来，且说对）。
    pub fn 头部说明(self) -> &'static str {
        match self {
            // mdat 在 L3 里只是个「跟着一起后移的盒子」，没有它也能插；
            // 但真正决定能不能插的是 moov 里的偏移表，所以这条只在 moov/mdat 都缺时才走到
            不能插::不是MP4 => "封面不是 MP4，没法把数据放在文件头部",
            不能插::没有MDAT => "封面里找不到视频数据段（mdat），没法放在文件头部",
            不能插::没有MOOV => "封面里找不到索引段（moov），没法放在文件头部",
            不能插::没有chunk表 => {
                "封面里没有 chunk 偏移表（分片 MP4？），没法放在文件头部"
            }
            不能插::mdat里没有chunk起点 => "视频数据段里找不到可用的插入点，没法放在文件头部",
            不能插::偏移超出32位 => {
                "包太大：整体后移会让播放位置超出 4 GiB 上限，这种布局装不下"
            }
            不能插::尺寸超出32位 => "包太大：视频数据段装不下（超过 4 GiB），这种布局装不下",
        }
    }
}

/// 「可以往这儿插」的封面：字节已经改好（`mdat.size` + 偏移表），长度不变
///
/// 为什么不直接把插入物拼进来：ZIP 可能几个 GB，拼进内存等于把整个包再装一遍。
/// 引擎拿到这个结构后按「前段 → 流式拷贝 ZIP → 后段」的顺序往外写，内存里只会有封面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct 中间插槽 {
    /// 补齐过的封面（与原封面等长；`mdat.size` 和所有 `stco`/`co64` 条目都改好了）
    pub 封面: Vec<u8>,
    /// 插入点：往 `封面[..插点]` 和 `封面[插点..]` 之间塞东西
    pub 插点: usize,
}

/// 算好「ZIP 插在哪、封面要怎么改」，但**不拼装**
///
/// `插入长度` = 准备插进去的字节数（就是 ZIP 的大小）。封面里所有 `>= 插点` 的偏移
/// 都加上这个数、`mdat.size` 也加上这个数 —— 也就是说：**调用方必须真的插入
/// 这么多字节**，否则算出来的封面是坏的。
pub fn 准备插槽(封面: &[u8], 插入长度: u64) -> Result<中间插槽, 不能插> {
    if 封面.len() < 8 || &封面[4..8] != b"ftyp" {
        return Err(不能插::不是MP4);
    }
    let 顶层 = 顶层盒子(封面);
    let mdat = 顶层
        .iter()
        .find(|b| &b.类型 == b"mdat")
        .copied()
        .ok_or(不能插::没有MDAT)?;
    let moov = 顶层
        .iter()
        .find(|b| &b.类型 == b"moov")
        .copied()
        .ok_or(不能插::没有MOOV)?;
    let 表 = 找chunk表(封面, moov);
    if 表.is_empty() {
        return Err(不能插::没有chunk表);
    }

    // ① 候选插点：所有轨的 chunk 起点里，落在 mdat 载荷区内的那些
    let (低, 高) = (mdat.载荷起点() as u64, mdat.末尾() as u64);
    let mut 候选: Vec<u64> = Vec::new();
    for t in &表 {
        for i in 0..t.数量 {
            let v = 读偏移(封面, t.条目区 + i * t.宽, t.宽).ok_or(不能插::没有chunk表)?;
            if (低..高).contains(&v) {
                候选.push(v);
            }
        }
    }
    if 候选.is_empty() {
        return Err(不能插::mdat里没有chunk起点);
    }
    候选.sort_unstable();
    候选.dedup();
    // 取中位：与实测参考实现（`tests/make_pack_layouts.py` 的 `_chunk_points`）同一规则。
    // 取中间是为了让 ZIP 前后都有真视频数据 —— 太靠前会退化成「贴在 mdat 头上」，
    // 太靠后会贴着尾部 free 原子，隐蔽性反而和 free 原子布局差不多。
    let 插点 = 候选[候选.len() / 2] as usize;

    let 增量 = 插入长度;
    if 增量 == 0 {
        return Ok(中间插槽 {
            封面: 封面.to_vec(),
            插点,
        });
    }

    // ② stco 是 32 位绝对偏移：加上增量会不会溢出？（co64 是 64 位，不用管）
    for t in &表 {
        if t.宽 != 4 {
            continue;
        }
        for i in 0..t.数量 {
            let v = 读偏移(封面, t.条目区 + i * t.宽, 4).ok_or(不能插::没有chunk表)?;
            if v >= 插点 as u64 && v + 增量 > u32::MAX as u64 {
                return Err(不能插::偏移超出32位);
            }
        }
    }

    // ③ mdat 的 size 字段要 +增量（`size == 0` 表示「一直到文件尾」，插完仍然成立，不动它）
    let (size位置, size宽) = if mdat.头长 == 16 {
        (mdat.偏移 + 8, 8usize)
    } else {
        (mdat.偏移, 4usize)
    };
    let 原大小 = 读偏移(封面, size位置, size宽).ok_or(不能插::没有MDAT)?;
    let 新大小 = if 原大小 == 0 {
        0
    } else {
        if size宽 == 4 && 原大小 + 增量 > u32::MAX as u64 {
            return Err(不能插::尺寸超出32位);
        }
        原大小 + 增量
    };

    // ④ 封面副本上打补丁。**位置都按原封面算** —— 副本里还没插入东西，
    //    所以 stco 表自己不用挪（真正拼装时它跟着后半段一起后移，偏移量自动对）。
    let mut 补: Vec<u8> = 封面.to_vec();
    if 新大小 != 原大小 {
        写偏移(&mut 补, size位置, size宽, 新大小);
    }
    for t in &表 {
        for i in 0..t.数量 {
            let o = t.条目区 + i * t.宽;
            let v = 读偏移(&补, o, t.宽).ok_or(不能插::没有chunk表)?;
            if v >= 插点 as u64 {
                写偏移(&mut 补, o, t.宽, v + 增量);
            }
        }
    }
    Ok(中间插槽 {
        封面: 补,
        插点,
    })
}

/// L3「放在文件头部」：把 ZIP 包成 `free` 原子插在 `ftyp` 之后
///
/// 与 [`准备插槽`]（L4 插在视频中间）的三处差别：
/// - **插点固定** = `ftyp` 末尾（不挑 chunk 边界：这里根本没往 `mdat` 里插）
/// - **`mdat.size` 一个字节都不改**（`mdat` 自己跟着整块后移，长度没变）
/// - **每条 `stco`/`co64` 都要 +增量**（后面所有字节整体后移，不是只补 `>= 插点` 那部分）
///
/// 依据：`refs/analysis/05-压缩布局验证.md` §3「L3」—— 隐写者模式 B 的修正版。
/// 隐写者原版逐原子插噪声并只补 `free` 长度 → `ffmpeg rc=183`（视频报废）；本函数
/// 不碰任何视频字节，只整体后移 + 全表补偿，实测 `rc=0` 且 stderr 空。
///
/// **代价（界面必须如实说）**：ZIP 在文件头部，EOCD 距文件尾极远 → 只靠「尾部 64 KiB
/// 窗口扫描」的工具（`unzip` / Windows 资源管理器 / 手机自带解压器）全都找不到；
/// 7-Zip 会从文件头一路往前扫，照旧能解。
pub fn 准备头部插槽(封面: &[u8], 插入长度: u64) -> Result<中间插槽, 不能插> {
    if 封面.len() < 8 || &封面[4..8] != b"ftyp" {
        return Err(不能插::不是MP4);
    }
    let 顶层 = 顶层盒子(封面);
    let ftyp = 顶层
        .iter()
        .find(|b| &b.类型 == b"ftyp")
        .copied()
        .ok_or(不能插::不是MP4)?;
    let 插点 = ftyp.末尾();
    if 插点 > 封面.len() {
        return Err(不能插::不是MP4);
    }
    // 必须先有可改的偏移表 —— 没有表就没法把后移量补回去，插了就是坏视频
    let moov = 顶层
        .iter()
        .find(|b| &b.类型 == b"moov")
        .copied()
        .ok_or(不能插::没有MOOV)?;
    let 表 = 找chunk表(封面, moov);
    if 表.is_empty() {
        return Err(不能插::没有chunk表);
    }

    let 增量 = 插入长度;
    if 增量 == 0 {
        return Ok(中间插槽 {
            封面: 封面.to_vec(),
            插点,
        });
    }

    // stco 是 32 位绝对偏移：插点在头部 → 绝大多数条目都要加，溢出检查必须全表做
    for t in &表 {
        if t.宽 != 4 {
            continue;
        }
        for i in 0..t.数量 {
            let v = 读偏移(封面, t.条目区 + i * t.宽, 4).ok_or(不能插::没有chunk表)?;
            if v >= 插点 as u64 && v + 增量 > u32::MAX as u64 {
                return Err(不能插::偏移超出32位);
            }
        }
    }

    // 副本上打补丁（位置按原封面算：副本里还没插入东西，拼装时整块后移，值不用再动）
    let mut 补: Vec<u8> = 封面.to_vec();
    for t in &表 {
        for i in 0..t.数量 {
            let o = t.条目区 + i * t.宽;
            let v = 读偏移(&补, o, t.宽).ok_or(不能插::没有chunk表)?;
            if v >= 插点 as u64 {
                写偏移(&mut 补, o, t.宽, v + 增量);
            }
        }
    }
    Ok(中间插槽 { 封面: 补, 插点 })
}

/// 把 `插入` 塞进封面 MP4 的 `mdat` 中间，返回拼好的整个文件（测试与一次性场景用）
///
/// 正式打包别用这个（会把整个 ZIP 读进内存），走 [`准备插槽`] + 流式写。
pub fn 插在中间(封面: &[u8], 插入: &[u8]) -> Result<Vec<u8>, 不能插> {
    let 槽 = 准备插槽(封面, 插入.len() as u64)?;
    if 插入.is_empty() {
        return Ok(槽.封面);
    }
    let mut 产物 = Vec::with_capacity(封面.len() + 插入.len());
    产物.extend_from_slice(&槽.封面[..槽.插点]);
    产物.extend_from_slice(插入);
    产物.extend_from_slice(&槽.封面[槽.插点..]);
    Ok(产物)
}

/// 大端读一个 4 或 8 字节整数（别的宽度不认）
fn 读偏移(data: &[u8], 位置: usize, 宽: usize) -> Option<u64> {
    if 宽 != 4 && 宽 != 8 {
        return None;
    }
    let 尾 = 位置.checked_add(宽)?;
    if 尾 > data.len() {
        return None;
    }
    let mut v = 0u64;
    for b in &data[位置..尾] {
        v = (v << 8) | *b as u64;
    }
    Some(v)
}

/// 大端写一个 4 或 8 字节整数
fn 写偏移(data: &mut [u8], 位置: usize, 宽: usize, 值: u64) {
    for i in 0..宽 {
         data[位置 + i] = ((值 >> (8 * (宽 - 1 - i))) & 0xff) as u8;
    }
}

#[cfg(test)]
mod 测试 {
    use super::*;

    fn 素材(名: &str) -> Vec<u8> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/assets/pack")
            .join(名);
        std::fs::read(&p).unwrap_or_else(|e| panic!("读不到素材 {}：{e}", p.display()))
    }

    /// ftyp 头（28 字节：size(4) + 'ftyp'(4) + 主品牌(4) + 次版本(4) + 兼容品牌(12)）
    fn 造ftyp() -> Vec<u8> {
        let mut ftyp = Vec::new();
        ftyp.extend_from_slice(&28u32.to_be_bytes());
        ftyp.extend_from_slice(b"ftyp");
        ftyp.extend_from_slice(b"isom");
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        ftyp.extend_from_slice(b"isomiso2");
        ftyp.extend_from_slice(b"mp41");
        assert_eq!(ftyp.len(), 28, "ftyp 头必须正好 28 字节，否则后面全错位");
        ftyp
    }

    /// moov > trak > mdia > minf > stbl > stco/co64（只放一张表，条目值原样写进去）
    fn 造moov表(条目: &[u64], 类型: &[u8; 4], 宽: usize) -> Vec<u8> {
        let mut stco = Vec::new();
        stco.extend_from_slice(&((16 + 宽 * 条目.len()) as u32).to_be_bytes());
        stco.extend_from_slice(类型);
        stco.extend_from_slice(&[0, 0, 0, 0]); // version + flags
        stco.extend_from_slice(&(条目.len() as u32).to_be_bytes());
        for v in 条目 {
            if 宽 == 8 {
                stco.extend_from_slice(&v.to_be_bytes());
            } else {
                stco.extend_from_slice(&(*v as u32).to_be_bytes());
            }
        }
        let 包一层 = |类型: &[u8; 4], 内层: &[u8]| -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&((8 + 内层.len()) as u32).to_be_bytes());
            v.extend_from_slice(类型);
            v.extend_from_slice(内层);
            v
        };
        let stbl = 包一层(b"stbl", &stco);
        let minf = 包一层(b"minf", &stbl);
        let mdia = 包一层(b"mdia", &minf);
        let trak = 包一层(b"trak", &mdia);
        包一层(b"moov", &trak)
    }

    /// 32 位 stco 版（最常用）
    fn 造moov(stco值: &[u32]) -> Vec<u8> {
        let 条目: Vec<u64> = stco值.iter().map(|v| *v as u64).collect();
        造moov表(&条目, b"stco", 4)
    }

    /// 造一个最小 mp4：ftyp + mdat + moov（moov 在最后，最常见的 ffmpeg 默认形态）
    fn 造mp4(mdat载荷: &[u8], stco值: &[u32], mdat_size_写0: bool) -> Vec<u8> {
        let mut 全 = 造ftyp();
        let size = if mdat_size_写0 {
            0u32
        } else {
            (8 + mdat载荷.len()) as u32
        };
        全.extend_from_slice(&size.to_be_bytes());
        全.extend_from_slice(b"mdat");
        全.extend_from_slice(mdat载荷);
        全.extend_from_slice(&造moov(stco值));
        全
    }

    /// 造 faststart 形态：ftyp + moov + mdat（mdat 最后，size 写 0 = 一直到文件尾）
    fn 造faststart(mdat载荷: &[u8], stco值: &[u32]) -> (Vec<u8>, usize) {
        let mut 全 = 造ftyp();
        全.extend_from_slice(&造moov(stco值));
        let mdat偏移 = 全.len();
        全.extend_from_slice(&0u32.to_be_bytes());
        全.extend_from_slice(b"mdat");
        全.extend_from_slice(mdat载荷);
        (全, mdat偏移)
    }

    #[test]
    fn 三个真实封面都读得出结构() {
        let c = 素材("cover5.mp4");
        let 顶层 = 顶层盒子(&c);
        let 类型: Vec<&[u8; 4]> = 顶层.iter().map(|b| &b.类型).collect();
        assert_eq!(类型, vec![b"ftyp", b"free", b"mdat", b"moov"]);

        let moov = 顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        let 表 = 找chunk表(&c, moov);
        assert_eq!(表.len(), 2, "带音轨的封面应该是两条轨 = 两张 chunk 表");
        assert_eq!(表.iter().filter(|t| t.宽 == 4).count(), 2, "两张都该是 32 位 stco");
        assert!(表.iter().all(|t| t.数量 > 0));

        // faststart：moov 在 mdat 前面，也要认得出
        let f = 素材("cover5_fast.mp4");
        let 顶层f = 顶层盒子(&f);
        assert_eq!(&顶层f[1].类型, b"moov");
        let moovf = 顶层f.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        assert_eq!(找chunk表(&f, moovf).len(), 2);

        // 单轨版：一张表、只有一条 chunk（这是「退化成贴在 mdat 头上」的极端样本）
        let v = 素材("cover5_vonly.mp4");
        let 顶层v = 顶层盒子(&v);
        let moovv = 顶层v.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        let 表v = 找chunk表(&v, moovv);
        assert_eq!(表v.len(), 1);
        assert_eq!(表v[0].数量, 1);
    }

    #[test]
    fn 插进去之后所有权移都对齐了() {
        for 名 in ["cover5.mp4", "cover5_fast.mp4", "cover5_vonly.mp4"] {
            let c = 素材(名);
            let 插入 = vec![0xABu8; 4096];
            let 顶层 = 顶层盒子(&c);
            let mdat = 顶层.iter().find(|b| &b.类型 == b"mdat").copied().unwrap();
            let moov = 顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
            let 表 = 找chunk表(&c, moov);
            let 原条目: Vec<(Chunk表, Vec<u64>)> = 表
                .iter()
                .map(|t| {
                    (
                        *t,
                        (0..t.数量)
                            .map(|i| 读偏移(&c, t.条目区 + i * t.宽, t.宽).unwrap())
                            .collect(),
                    )
                })
                .collect();

            let 产物 = 插在中间(&c, &插入).unwrap();
            assert_eq!(产物.len(), c.len() + 插入.len(), "{名}：总长要正好加上插入物");

            // 顶层结构自洽：盒子首尾相接、一个不漏
            let 新顶层 = 顶层盒子(&产物);
            assert_eq!(新顶层.len(), 顶层.len(), "{名}：顶层盒子数量不该变");
            let mut 期望起 = 0usize;
            for b in &新顶层 {
                assert_eq!(b.偏移, 期望起, "{名}：盒子要首尾相接");
                期望起 = b.末尾();
            }
            assert_eq!(期望起, 产物.len(), "{名}：最后一个盒子要正好到文件尾");

            // mdat 变大，且新增的那段里就是我们的字节
            let 新mdat = 新顶层.iter().find(|b| &b.类型 == b"mdat").copied().unwrap();
            assert_eq!(
                新mdat.大小,
                mdat.大小 + 插入.len(),
                "{名}：mdat 的 size 字段要 +插入长度"
            );
            let 新moov = 新顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
            let 新表 = 找chunk表(&产物, 新moov);
            assert_eq!(新表.len(), 表.len(), "{名}：表数量不该变");

            // 逐条核对：插点之前的原值不动，之后的 +插入长度
            let 低 = mdat.载荷起点() as u64;
            let 高 = mdat.末尾() as u64;
            let mut 全体: Vec<u64> = 原条目
                .iter()
                .flat_map(|(_, v)| v.iter().copied())
                .filter(|v| (*v >= 低) && (*v < 高))
                .collect();
            全体.sort_unstable();
            全体.dedup();
            let 插点 = 全体[全体.len() / 2] as usize;
            assert!(
                产物[插点..插点 + 插入.len()].iter().all(|b| *b == 0xAB),
                "{名}：插入物要正好落在插点"
            );
            let mut 动了 = 0;
            for (t, 原) in &原条目 {
                let 新条目区: usize = if t.条目区 >= 插点 {
                    t.条目区 + 插入.len()
                } else {
                    t.条目区
                };
                for (i, 旧) in 原.iter().enumerate() {
                    let 新 = 读偏移(&产物, 新条目区 + i * t.宽, t.宽).unwrap();
                    let 期望 = if *旧 >= 插点 as u64 {
                        动了 += 1;
                        旧 + 插入.len() as u64
                    } else {
                        *旧
                    };
                    assert_eq!(新, 期望, "{名}：第 {i} 条偏移补错了");
                }
            }
            assert!(动了 > 0, "{名}：总得有些条目被推后");

            // 插入的字节必须严格在 mdat 的载荷区内（不能跑到盒子头上）
            assert!(插点 as u64 >= 低 && (插点 as u64) < 高);
        }
    }

    #[test]
    fn 不是mp4就明说做不到() {
        assert_eq!(插在中间(b"hello world", b"x"), Err(不能插::不是MP4));
        assert_eq!(插在中间(b"", b"x"), Err(不能插::不是MP4));
        // PNG 头（前 8 字节没有 ftyp）
        assert_eq!(
            插在中间(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a], b"x"),
            Err(不能插::不是MP4)
        );
    }

    #[test]
    fn 没有mdat或没有stco都拒绝而不是瞎插() {
        // 只有 ftyp
        let mut 只有ftyp = Vec::new();
        只有ftyp.extend_from_slice(&28u32.to_be_bytes());
        只有ftyp.extend_from_slice(b"ftyp");
        只有ftyp.extend_from_slice(b"isom");
        只有ftyp.extend_from_slice(&0u32.to_be_bytes());
        只有ftyp.extend_from_slice(b"isomiso2");
        assert_eq!(插在中间(&只有ftyp, b"x"), Err(不能插::没有MDAT));
    }

    #[test]
    fn 小样本能插且结构自洽() {
        // mdat 载荷 100 字节，三条 chunk 起点（相对文件头）
        let 载荷 = vec![0u8; 100];
        let 低 = 36u32; // ftyp 28 + mdat 头 8
        let c = 造mp4(&载荷, &[低, 低 + 30, 低 + 60], false);
        let 插入 = vec![7u8; 40];
        let 产物 = 插在中间(&c, &插入).unwrap();
        assert_eq!(产物.len(), c.len() + 40);
        let 顶层 = 顶层盒子(&产物);
        let mdat = 顶层.iter().find(|b| &b.类型 == b"mdat").copied().unwrap();
        assert_eq!(mdat.大小, 108 + 40, "mdat size 要加 40");
        // 插点取中位 = 第二条 chunk 起点 = 低+30
        let 插点 = (低 + 30) as usize;
        assert!(产物[插点..插点 + 40].iter().all(|b| *b == 7));
    }

    #[test]
    fn mdat_size为零也照样能插() {
        // size==0 表示「一直到文件尾」，合法形态只有一种：mdat 是最后一个盒子。
        // 所以这里造 faststart（ftyp + moov + mdat），stco 指向 mdat 载荷。
        let 载荷 = vec![0u8; 64];
        let mdat偏移 = 28 + 造moov(&[0]).len();
        let 载荷起点 = (mdat偏移 + 8) as u32;
        let (c, 真偏移) = 造faststart(&载荷, &[载荷起点]);
        assert_eq!(真偏移, mdat偏移);

        let 产物 = 插在中间(&c, &[9u8; 16]).unwrap();
        assert_eq!(产物.len(), c.len() + 16);
        // size 字段保持 0（插完仍然「到文件尾」，改了反而把后面的数据算进 mdat）
        assert_eq!(&产物[mdat偏移..mdat偏移 + 4], &0u32.to_be_bytes());
        assert_eq!(&产物[0..4], &28u32.to_be_bytes(), "ftyp 不该动");

        // 只有一条 chunk：插点就是它的起点，插完要 +16
        let 顶层 = 顶层盒子(&产物);
        let moov盒 = 顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        let 表 = 找chunk表(&产物, moov盒);
        assert_eq!(表.len(), 1);
        assert_eq!(
            读偏移(&产物, 表[0].条目区, 4).unwrap(),
            载荷起点 as u64 + 16
        );
    }

    #[test]
    fn 尺寸溢出要拒绝而不是写出坏包() {
        let 载荷 = vec![0u8; 64];
        let 低 = 36u64;
        // 用 co64（64 位偏移）造样本：偏移那边永远不会溢出，才能单独验「尺寸」这条
        let mut c = 造ftyp();
        c.extend_from_slice(&(8 + 载荷.len() as u32).to_be_bytes());
        c.extend_from_slice(b"mdat");
        c.extend_from_slice(&载荷);
        c.extend_from_slice(&造moov表(&[低, 低 + 32], b"co64", 8));

        // 不真的分配 4 GiB —— 直接把「要插多少字节」按 u64 报上去，
        // 让检查在**还没开始写**的时候就拦住（这也是 准备插槽 单独存在的原因之一）
        assert_eq!(准备插槽(&c, u32::MAX as u64), Err(不能插::尺寸超出32位));
    }

    #[test]
    fn 偏移溢出要拒绝() {
        let 载荷 = vec![0u8; 64];
        let 低 = 36u32;
        // 第二条条目故意填到接近 u32 上限 —— 它是「坏结构」，但检查必须兜住
        let c = 造mp4(&载荷, &[低, u32::MAX - 4], false);
        assert_eq!(
            插在中间(&c, &[1u8; 32]),
            Err(不能插::偏移超出32位)
        );
    }

    #[test]
    fn 坏结构也不恐慌() {
        // 截断在各处的封面：只能返回错误，绝不能 panic
        let c = 素材("cover5.mp4");
        for 截 in [0usize, 4, 8, 28, 36, 100, c.len() / 2] {
            let 半 = &c[..截.min(c.len())];
            let _ = 插在中间(半, b"xyz");
        }
        // 长度字段瞎写的假文件
        let 乱 = b"\x00\x00\x00\x08ftyp\xff\xff\xff\xffmdat";
        let _ = 插在中间(乱, b"xyz");
    }

    // ── L3「放在文件头部」───────────────────────────────────────────────
    // 这几条盯的是 L3 与 L4 的**三处差别**：插点固定在 ftyp 末尾、mdat.size 不动、
    // 全表偏移都 +增量。漏一条就会打出「能解但视频坏」或「视频好但包错位」的包。

    /// 造一个合法的 `free` 原子（size + 'free' + 载荷）—— L3 正式路径插的就是这个
    fn free原子(载荷: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&((8 + 载荷.len()) as u32).to_be_bytes());
        v.extend_from_slice(b"free");
        v.extend_from_slice(载荷);
        v
    }

    /// 按插槽把插入物拼起来（L3 的正式路径是流式写，测试里直接拼）
    fn 拼头部(封面: &[u8], 插入: &[u8]) -> Vec<u8> {
        let 槽 = 准备头部插槽(封面, 插入.len() as u64).unwrap();
        let mut 产物 = Vec::with_capacity(封面.len() + 插入.len());
        产物.extend_from_slice(&槽.封面[..槽.插点]);
        产物.extend_from_slice(插入);
        产物.extend_from_slice(&槽.封面[槽.插点..]);
        产物
    }

    #[test]
    fn l3_插点就是ftyp末尾且mdat一个字节不动() {
        let 载荷 = vec![0u8; 100];
        let 低 = 36u32;
        let c = 造mp4(&载荷, &[低, 低 + 30, 低 + 60], false);
        let ftyp末尾 = 28usize;
        let 插入 = free原子(&[0xCDu8; 32]); // 总长 40

        let 槽 = 准备头部插槽(&c, 插入.len() as u64).unwrap();
        assert_eq!(槽.插点, ftyp末尾, "L3 的插点必须正好是 ftyp 的末尾");

        let 产物 = 拼头部(&c, &插入);
        assert_eq!(产物.len(), c.len() + 40);
        // 插进 ftyp 末尾的是「8 字节 free 头 + 32 字节载荷」
        assert_eq!(&产物[ftyp末尾..ftyp末尾 + 4], &40u32.to_be_bytes(), "free 原子头要带对的长度");
        assert_eq!(&产物[ftyp末尾 + 4..ftyp末尾 + 8], b"free");
        assert!(
            产物[ftyp末尾 + 8..ftyp末尾 + 40].iter().all(|b| *b == 0xCD),
            "载荷要正好落在插点之后"
        );
        assert_eq!(&产物[0..4], &28u32.to_be_bytes(), "ftyp 自己不该被改");

        // ★ mdat.size 必须原封不动（L4 才改它）
        let 顶层 = 顶层盒子(&产物);
        let mdat = 顶层.iter().find(|b| &b.类型 == b"mdat").copied().unwrap();
        assert_eq!(mdat.大小, 108, "L3 不往 mdat 里插，size 不能变");
        // 顶层盒子首尾相接
        let mut 起 = 0usize;
        for b in &顶层 {
            assert_eq!(b.偏移, 起, "盒子要首尾相接");
            起 = b.末尾();
        }
        assert_eq!(起, 产物.len());
    }

    #[test]
    fn l3_每一条偏移都要加上插入长度() {
        let 载荷 = vec![0u8; 128];
        let 低 = 36u32;
        let 原条目 = [低, 低 + 40, 低 + 80];
        let c = 造mp4(&载荷, &原条目, false);
        let 插入 = free原子(&[0u8; 56]); // 总长 64
        let 产物 = 拼头部(&c, &插入);

        let 顶层 = 顶层盒子(&产物);
        let moov = 顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        let 表 = 找chunk表(&产物, moov);
        assert_eq!(表.len(), 1);
        for (i, 旧) in 原条目.iter().enumerate() {
            let 新 = 读偏移(&产物, 表[0].条目区 + i * 4, 4).unwrap();
            assert_eq!(新, *旧 as u64 + 64, "第 {i} 条必须 +插入长度（全表都要补）");
        }
    }

    #[test]
    fn l3_faststart形态也能对() {
        // ftyp + moov + mdat：moov 在插点之后，整块后移，表里的绝对偏移仍要 +增量
        let 载荷 = vec![0u8; 64];
        let mdat偏移 = 28 + 造moov(&[0]).len();
        let 载荷起点 = (mdat偏移 + 8) as u32;
        let (c, _) = 造faststart(&载荷, &[载荷起点]);
        let 插入 = free原子(&[0u8; 16]); // 总长 24
        let 产物 = 拼头部(&c, &插入);
        assert_eq!(产物.len(), c.len() + 24);

        let 顶层 = 顶层盒子(&产物);
        let moov = 顶层.iter().find(|b| &b.类型 == b"moov").copied().unwrap();
        let 表 = 找chunk表(&产物, moov);
        assert_eq!(读偏移(&产物, 表[0].条目区, 4).unwrap(), 载荷起点 as u64 + 24);
        // mdat size 写的是 0（一直到文件尾）—— 插在前面仍然成立，不能改成具体值
        let mdat = 顶层.iter().find(|b| &b.类型 == b"mdat").copied().unwrap();
        assert_eq!(读偏移(&产物, mdat.偏移, 4).unwrap(), 0);
    }

    #[test]
    fn l3_没有moov或没有表就拒绝() {
        // 只有 ftyp + free（没有 moov）→ 没法补偏移，必须拒
        let mut 只有ftyp = 造ftyp();
        只有ftyp.extend_from_slice(&8u32.to_be_bytes());
        只有ftyp.extend_from_slice(b"free");
        assert_eq!(准备头部插槽(&只有ftyp, 16), Err(不能插::没有MOOV));

        // 有 moov 但没有 stco/co64（分片 MP4 的形态）→ 同样拒
        let mut 无表 = 造ftyp();
        无表.extend_from_slice(&8u32.to_be_bytes());
        无表.extend_from_slice(b"moov");
        assert_eq!(准备头部插槽(&无表, 16), Err(不能插::没有chunk表));

        // 不是 MP4
        assert_eq!(准备头部插槽(b"hello world", 16), Err(不能插::不是MP4));
    }

    #[test]
    fn l3_偏移溢出要拒绝() {
        let 载荷 = vec![0u8; 64];
        let c = 造mp4(&载荷, &[36, u32::MAX - 4], false);
        assert_eq!(准备头部插槽(&c, 32), Err(不能插::偏移超出32位));
    }

    #[test]
    fn l3_坏结构也不恐慌() {
        let c = 素材("cover5.mp4");
        for 截 in [0usize, 4, 8, 28, 36, 100, c.len() / 2] {
            let _ = 准备头部插槽(&c[..截.min(c.len())], 128);
        }
        let _ = 准备头部插槽(b"\x00\x00\x00\x08ftyp\xff\xff\xff\xffmdat", 128);
    }

    #[test]
    fn l3_真实封面插完结构自洽() {
        for 名 in ["cover5.mp4", "cover5_fast.mp4", "cover5_vonly.mp4"] {
            let c = 素材(名);
            let 产物 = 拼头部(&c, &free原子(&[0x5Au8; 89])); // 总长 97
            // 顶层盒子首尾相接、最后一个正好到文件尾（长度字段被我们改过的话这里会炸）
            let 顶层 = 顶层盒子(&产物);
            let mut 起 = 0usize;
            for b in &顶层 {
                assert_eq!(b.偏移, 起, "{名}：盒子要首尾相接");
                起 = b.末尾();
            }
            assert_eq!(起, 产物.len(), "{名}：最后一个盒子要正好到文件尾");
            // mdat 的 size 一个都没变
            let 原 = 顶层盒子(&c);
            let 原mdat = 原.iter().find(|b| &b.类型 == b"mdat").unwrap();
            let 新mdat = 顶层.iter().find(|b| &b.类型 == b"mdat").unwrap();
            assert_eq!(新mdat.大小, 原mdat.大小, "{名}：mdat size 不该变");
        }
    }
}
