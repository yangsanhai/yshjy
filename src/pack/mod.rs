//! 伪装打包 —— 把压缩包藏进视频 / 图片里
//!
//! 与「解压」方向相反。布局由数据表驱动，实测淘汰的布局（L5 分散多段 / L6 前后加垃圾）不提供。
//!
//! # 布局选择（依据调研实测的兼容性矩阵）
//! | 布局 | 说明 | 实测结论 |
//! |---|---|---|
//! | [`PackLayout::TailAppend`] | 视频后面直接接 ZIP | 视频能播；8 种工具全通 |
//! | [`PackLayout::FreeAtom`] | 藏进尾部 `free` 原子 | **推荐**：视频能播 + 8 种工具全通 |
//!
//! 「分散多段」和「前后加垃圾」两种布局**故意不提供** —— 实测 7-Zip 只解出 1/4，
//! 且 ffmpeg 直接报错播不了，属于"看起来更隐蔽、实际把用户坑了"。
//!
//! # 关键实现细节（对齐生成方 Steganographier 的做法，但修掉它的 bug）
//! - `free` 原子：`size(4) + b"free"(4) + payload`；
//!   payload 超过 4 GiB 时写 `size = 1` 再跟 8 字节 large-size（MP4 规范的 64 位形式）
//! - 原子开头塞 **64–256 字节随机 pad**：既保证原子内不以 `PK` 开头（更隐蔽），
//!   又让每次产物哈希不同
//! - 产物**必须回读自检**（见 [`self_check`]），杜绝"造出一个打不开的包"

use crate::error::{Error, ErrorCode, Result};
use crate::pack::cover::BuiltinCover;
use crate::pack::decoy::DecoyOptions;
// 写侧加密目前还挂在 zip 的 unstable 门后（读侧已稳定）
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use zip::unstable::write::FileOptionsExt as _;

pub mod cover;
pub mod decoy;

/// 伪装布局
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackLayout {
    /// 视频后面直接接 ZIP（最朴素，兼容性最好）
    TailAppend,
    /// 藏进尾部 `free` 原子（推荐）
    #[default]
    FreeAtom,
}

/// 打包用的归档格式（T17-4）
///
/// - `Zip`：默认。**密码 / 分卷 / 自解压 / 伪装布局只有 ZIP 能做。**
/// - `TarGz` / `TarZst`：只给普通压缩用（备份场景）。
///
/// # 为什么 tar 系不能进伪装模式（2026-10-31 探针实测，不是推论）
///
/// 手工拼「`mini.mp4` 封面 + tar / tar.gz / tar.zst」后跑 `yshjy --list`：
/// 三种全部报 `NOT_ARCHIVE`（定位器只找 ZIP 的 EOCD `PK\x05\x06`，认不出
/// gzip `1f 8b` / zstd `28 b5 2f fd` / tar 的 `ustar` 头）。
/// 也就是说「打出来我们自己也解不开」—— 与其做完再解释，不如协议层直接拒绝
/// （用户能理解：伪装是给网盘看的，tar.gz 本来就是个正经归档）。
/// 解压侧**单文件** tar / tar.gz / tar.zst 是好的（`--list` 与真解压都实测过）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackFormat {
    /// 标准 ZIP（默认）
    #[default]
    Zip,
    /// tar + gzip
    TarGz,
    /// tar + zstd
    TarZst,
}

impl PackFormat {
    /// 从协议 / 命令行 / 界面传来的字符串名解析（大小写不敏感，收几种常见写法）
    pub fn 解析(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "tar.gz" | "targz" | "tgz" | "tar_gz" => Some(Self::TarGz),
            "tar.zst" | "tarzst" | "tzst" | "tar_zst" => Some(Self::TarZst),
            _ => None,
        }
    }

    /// 对外名字（协议 / 报告里用这个）
    pub fn 名字(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
        }
    }

    /// tar 系：没有密码、没有分卷、不能自解压、不能伪装
    pub fn 是tar(self) -> bool {
        !matches!(self, Self::Zip)
    }

    /// 中间产物后缀（临时文件名要跟着格式走，别让 .zip 里装着 tar）
    fn 临时后缀(self) -> &'static str {
        self.名字()
    }
}

/// 打包参数
#[derive(Debug, Clone)]
pub struct PackOptions {
    /// 要打进去的文件 / 目录
    pub inputs: Vec<PathBuf>,
    /// 封面数据（`None` = 用内置素材）
    pub cover: Option<Vec<u8>>,
    /// 输出文件扩展名 —— 决定用哪个内置素材
    pub cover_ext: String,
    /// 密码（`None` = 不加密）
    pub password: Option<String>,
    /// 布局
    pub layout: PackLayout,
    /// 随机 pad 范围（仅 FreeAtom 用）
    pub pad_range: (usize, usize),
    /// 压缩级别 0-9（0 = 只存不压）
    pub level: u32,
    /// 归档格式（`Zip` 默认；tar 系只给普通压缩用，见 [`PackFormat`]）
    pub format: PackFormat,
    /// 防炸链：给条目时间戳加随机偏移（T18-3 / CLI `--tweak-time` / 协议 `pack.tweak_time`）
    ///
    /// 网盘按内容哈希判重，同一份东西反复上传会秒传 / 炸链。时间戳是 ZIP/tar 头里的
    /// 字段，改了它整包字节就变 —— 哈希也变，网盘当成新文件。**默认关**（DESIGN §14.2）。
    pub tweak_time: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            cover: None,
            cover_ext: "mp4".into(),
            password: None,
            layout: PackLayout::default(),
            pad_range: (64, 256),
            level: 6,
            format: PackFormat::Zip,
            tweak_time: false,
        }
    }
}

/// 打包结果
#[derive(Debug, Clone)]
pub struct PackReport {
    /// 产物总大小
    pub total_size: u64,
    /// 其中 ZIP 部分大小
    pub zip_size: u64,
    /// 封面部分大小
    pub cover_size: u64,
    /// 打进去的条目数
    pub entries: usize,
    /// 用的什么布局
    pub layout: PackLayout,
    /// 干扰数据字节数（`--decoy` 没开时是 0）
    pub noise_size: u64,
    /// ZIP 数据在产物里的起点偏移（自检用它核对「噪声确实在压缩数据之外」）
    pub zip_offset: u64,
    /// 自检发现的问题（空 = 全好）
    pub warnings: Vec<String>,
    /// `--keep` 时保留的中间 ZIP（默认已删除，这里是 None）
    pub intermediate: Option<PathBuf>,
}

impl PackReport {
    /// 是否洁癖通过（自检零问题）
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }

    // 注：这里**故意没有** `summary()`。
    //
    // 早先它直接拼中文句子（"写出 N 字节（封面 A + 内容 B），共 C 个条目…"），
    // 违反了本项目的分层约定（见 src/i18n.rs 模块头：引擎只给结构化数据，
    // 句子归外壳）。结果是：英文/日文界面下面这句永远中文，翻译不了。
    // v0.9 第 10 批把它挪到外壳（main::do_pack）查语言包渲染 —— 别再加回来。
}

/// `--keep`：保留打包的中间 ZIP（默认删掉）
///
/// 放全局开关而不是 [`PackOptions`] 的字段：那个结构体测试里有十几处逐字段构造，
/// 加字段会把测试一起改坏（同 [`build_plain`] 的注释）。
static 保留中间产物: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 设定是否保留中间产物（`--keep`）
pub fn 设定保留中间产物(b: bool) {
    保留中间产物.store(b, std::sync::atomic::Ordering::Relaxed);
}

/// 是否保留中间产物
pub fn 保留中间产物已开启() -> bool {
    保留中间产物.load(std::sync::atomic::Ordering::Relaxed)
}

/// 统计输入的总字节与文件数（打包进度的分母）
///
/// 目录要递归。拿不到 stat 的条目直接跳过 —— 进度分母少算一点，不该让打包失败。
pub fn 输入统计(inputs: &[PathBuf]) -> (u64, u64) {
    fn 走(p: &Path, 字节: &mut u64, 文件: &mut u64) {
        let Ok(m) = fs::metadata(p) else {
            return;
        };
        if m.is_dir() {
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    走(&e.path(), 字节, 文件);
                }
            }
        } else {
            *字节 = 字节.saturating_add(m.len());
            *文件 += 1;
        }
    }
    let (mut b, mut f) = (0u64, 0u64);
    for i in inputs {
        走(i, &mut b, &mut f);
    }
    (b, f.max(1))
}

/// 打包进度事件（`PACK_PROGRESS` 的数据源）
///
/// # 为什么不是简单的 `FnMut(u64, &str)`
///
/// 伪装包和自解压都是**两段式**的：先把内容压进临时 ZIP，再把那个 ZIP 拼到
/// 外壳 / 存根后面。两段的「分母」不是一个东西 —— 压缩阶段的分母是「输入文件总字节」，
/// 拼装阶段的分母是「中间 ZIP 的大小」。
///
/// 只给 `(已写, 名字)` 的话，外壳没法知道「压缩完了、接下来总量变了」，
/// 进度条要么停在某个百分比、要么冲过 100%。所以多一个 [`PackTick::TotalHint`]：
/// 外壳收到它就把分母放大，进度条才走得平滑。
pub enum PackTick {
    /// 又处理了 n 字节
    ///
    /// `Some(名字)` = 压缩阶段正在压的条目；`None` = 拼装输出阶段（没有「当前文件」可言）
    Wrote(u64, Option<String>),
    /// 压缩阶段结束，接下来还要往输出写这么多字节
    TotalHint(u64),
}

/// 把临时 ZIP 流式拼到输出，并按块报进度
///
/// 以前这里是 `std::io::copy`（一次拷完、零进度）：2 GB 的包在机械盘上要几十秒，
/// 界面进度条从压缩阶段结束后就一动不动，用户看起来就是「卡死了」。
fn 拷贝带进度(源: &Path, 出: &mut impl Write, 进度: &mut dyn FnMut(PackTick)) -> Result<u64> {
    use std::io::Read as _;
    let mut f = fs::File::open(源)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("open tmp: {e}")))?;
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut 总 = 0u64;
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read tmp: {e}")))?;
        if n == 0 {
            break;
        }
        出.write_all(&buf[..n])
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("copy zip: {e}")))?;
        总 += n as u64;
        进度(PackTick::Wrote(n as u64, None));
    }
    Ok(总)
}

/// **把输入打成伪装包**
///
/// 数据流：`封面 → [free 原子头] → 随机 pad → ZIP 数据`
///
/// 为了不被大文件撑爆内存，ZIP 先落在临时文件，最后流式拼接。
pub fn build<W: Write>(opts: &PackOptions, out: &mut W) -> Result<PackReport> {
    build_ctx(opts, out, &mut |_| {})
}

/// 同 [`build`]，但逐块报进度（`PACK_PROGRESS` 的数据源）
pub fn build_ctx<W: Write>(
    opts: &PackOptions,
    out: &mut W,
    进度: &mut dyn FnMut(PackTick),
) -> Result<PackReport> {
    build_ctx_decoy(opts, &DecoyOptions::none(), out, 进度)
}

/// 同 [`build_ctx`]，但可以顺便在压缩数据之外撒干扰数据（第三期 B）
///
/// 为什么要单开一个函数、而不是给 [`PackOptions`] 加字段：那个结构体在测试里
/// 有十几处**逐字段**构造，加字段会把它们全改一遍（同 [`build_plain`] 的注释）。
pub fn build_ctx_decoy<W: Write>(
    opts: &PackOptions,
    decoy: &DecoyOptions,
    out: &mut W,
    进度: &mut dyn FnMut(PackTick),
) -> Result<PackReport> {
    if opts.inputs.is_empty() {
        return Err(Error::new(ErrorCode::Internal).with("reason", "没有要打包的文件"));
    }
    // 防御：伪装 / 自解压路径只认 ZIP。正常调用方（api / CLI）已在参数校验里拦过，
    // 这里再守一道 —— 免得以后有人绕开校验直接调库，打出一个「自己都解不开」的包。
    if opts.format.是tar() {
        return Err(Error::new(ErrorCode::Internal)
            .with("reason", "伪装打包只支持 zip：tar 系没有可靠的定位方式，解压侧认不出"));
    }
    // 参数越界要在**动磁盘之前**就报错（别打到一半才发现 --decoy 填错）
    decoy.validate()?;

    // ① 封面：用户给的，或内置素材
    let cover: Vec<u8> = match &opts.cover {
        Some(c) => c.clone(),
        None => BuiltinCover::for_extension(&opts.cover_ext)
            .bytes()
            .to_vec(),
    };

    // ② 内容先打成 ZIP（走临时文件，避免大包占内存）
    let tmp = std::env::temp_dir().join(format!(
        "yshjy_pack_{}_{}.zip",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let entries = write_zip(&opts.inputs, &tmp, opts, 进度)?;
    let zip_size = fs::metadata(&tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tmp meta: {e}")))?
        .len();

    // ③ 拼装
    let cover_size = cover.len() as u64;

    // 防呆：`free` 原子是 MP4 的 ISO 盒子结构，塞进图片/文本里会**直接损坏封面**
    // （实测：`ys -o 图.png` 以前就是 MP4 头 + .png 后缀，看图软件一认魔数就露馅）。
    // 这里按**封面魔数**判断，而不是看扩展名 —— 调用方传错 cover_ext 也拦得住。
    let 封面是mp4 = cover.len() > 8 && &cover[4..8] == b"ftyp";
    let mut layout = opts.layout;
    let mut warnings: Vec<String> = Vec::new();
    if layout == PackLayout::FreeAtom && !封面是mp4 {
        layout = PackLayout::TailAppend;
        warnings.push("封面不是 MP4，已自动改用「尾部追加」布局（free 原子只适合 MP4）".into());
    }

    out.write_all(&cover)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write cover: {e}")))?;

    // 干扰数据的种子：只由输入路径派生（**不看系统时间**），
    // 这样同一份输入打两次，产物逐字节一致 —— 用户能校验自己分享的东西。
    let 噪声种子 = decoy::种子(&opts.inputs, 0x00DE_C0DE);
    let 噪声总量 = decoy.total();

    // 压缩完了：告诉外壳「接下来还要写多少字节」，进度条才不会停在压缩阶段的百分比
    进度(PackTick::TotalHint(zip_size + 噪声总量));

    let mut atom_overhead = 0u64;
    let noise_size;
    let mut pad_len = 0u64;
    match layout {
        PackLayout::TailAppend => {
            // L1：噪声写在「封面之后、ZIP 起点之前」
            noise_size = 写噪声带进度(out, decoy, 噪声种子, 进度)?;
            拷贝带进度(&tmp, out, 进度)?;
        }
        PackLayout::FreeAtom => {
            // `--decoy` 开启时 pad 也走稳定种子 —— 否则 pad 每次不同，整包哈希就变了，
            // 「同一份输入两次打包逐字节一致」这条验收过不去。
            let pad = if decoy.is_on() {
                make_pad_seeded(opts.pad_range, 噪声种子 ^ 0x5EED)
            } else {
                make_pad(opts.pad_range)
            };
            pad_len = pad.len() as u64;
            // L2：噪声写在「free 原子内 pad 之后、ZIP 起点之前」（原子结构不变）
            let payload_len = pad_len + 噪声总量 + zip_size;
            atom_overhead = write_free_atom_header(out, payload_len)?;
            out.write_all(&pad)
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write pad: {e}")))?;

            noise_size = 写噪声带进度(out, decoy, 噪声种子, 进度)?;
            拷贝带进度(&tmp, out, 进度)?;
        }
    }
    // ZIP 起点：自检要拿它核对「噪声确实全在压缩数据之外」
    let zip_offset = cover_size + atom_overhead + pad_len + noise_size;

    if 保留中间产物已开启() {
        // `--keep`：留着中间 ZIP（外壳会把路径打出来）
    } else {
        let _ = fs::remove_file(&tmp);
    }

    Ok(PackReport {
        total_size: cover_size + zip_size + atom_overhead + pad_len + noise_size,
        zip_size,
        cover_size,
        entries,
        // 报告**实际用的**布局（可能被上面降级过），不是 opts 里那个
        layout,
        noise_size,
        zip_offset,
        warnings,
        intermediate: 保留中间产物已开启().then(|| tmp.clone()),
    })
}

/// 普通压缩：只产出一个标准 ZIP，不写封面、不藏任何东西（GUI-PLAN §5.2②）
///
/// 与 [`build`] 共用 `write_zip`，差别只有「不写封面」这一步。
///
/// 为什么单独开一个函数而不是给 [`PackOptions`] 加 `plain: bool`：
/// 那个结构体在测试里有十几处**逐字段**构造，加一个字段就得把所有测试一起改，
/// 而且改漏一处就是编译错误（HANDOFF「给广泛构造的结构体加字段」那条教训）。
pub fn build_plain<W: Write>(opts: &PackOptions, out: &mut W) -> Result<PackReport> {
    build_plain_ctx(opts, out, &mut |_| {})
}

/// 同 [`build_plain`]，但把「已写多少」逐块报出来（`PACK_PROGRESS` 的数据源）
pub fn build_plain_ctx<W: Write>(
    opts: &PackOptions,
    out: &mut W,
    进度: &mut dyn FnMut(PackTick),
) -> Result<PackReport> {
    if opts.inputs.is_empty() {
        return Err(Error::new(ErrorCode::Internal).with("reason", "没有要打包的文件"));
    }

    let tmp = std::env::temp_dir().join(format!(
        // 后缀跟着格式走：tar 系别叫 .zip（`--keep` 时用户会拿着个 .zip 名去 tar -tzf）
        "yshjy_plain_{}_{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        opts.format.临时后缀()
    ));
    let entries = write_archive(&opts.inputs, &tmp, opts, 进度)?;
    let zip_size = fs::metadata(&tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tmp meta: {e}")))?
        .len();

    // 拼装阶段（这里以前也没有进度：2 GB 的普通 ZIP 同样会卡住不动）
    进度(PackTick::TotalHint(zip_size));
    拷贝带进度(&tmp, out, 进度)?;
    if 保留中间产物已开启() {
        // `--keep`：留着中间 ZIP
    } else {
        let _ = fs::remove_file(&tmp);
    }

    Ok(PackReport {
        total_size: zip_size,
        zip_size,
        cover_size: 0,
        entries,
        // 普通压缩没有「布局」概念；字段保留给协议兼容，对外由 `mode` 区分
        layout: PackLayout::TailAppend,
        // 普通压缩（标准 ZIP）不支持干扰数据：没有外壳可藏，加了只是往 ZIP 前面
        // 塞垃圾，会把别人的 unzip 搞出 warning —— CLI/协议层会明确拒绝这个组合。
        noise_size: 0,
        zip_offset: 0,
        warnings: Vec::new(),
        intermediate: 保留中间产物已开启().then(|| tmp.clone()),
    })
}

/// 造随机 pad
///
/// 用系统时间做种子（避免为这点小事拖进 `rand` 依赖，也符合"依赖最小"原则）。
/// 保证首两字节不是 `PK`，让 `free` 原子内部不以 ZIP 签名开头。
fn make_pad(range: (usize, usize)) -> Vec<u8> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(7);
    make_pad_seeded(range, seed)
}

/// 同 [`make_pad`]，但随机量由调用方给种子
///
/// `--decoy` 开启时必须走这条：pad 若还用系统时间，产物每次都不一样，
/// 「同一份输入两次打包逐字节一致」就过不去（见 `pack::decoy` 模块头）。
fn make_pad_seeded((lo, hi): (usize, usize), seed: u64) -> Vec<u8> {
    let span = hi.saturating_sub(lo).max(1);
    let seed = if seed == 0 { 7 } else { seed };
    let pad_len = lo + (seed as usize % span);
    let mut pad = vec![0u8; pad_len];
    let mut x = (seed as u32) | 1;
    for b in pad.iter_mut() {
        // xorshift32：够用且零依赖
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = (x & 0xFF) as u8;
    }
    if pad.len() >= 2 && pad[0] == b'P' && pad[1] == b'K' {
        pad[0] = 0;
    }
    pad
}

/// 写干扰数据并顺手报进度（`Wrote` 的字节也进分母，见调用处的 `TotalHint`）
///
/// 不带 `--decoy` 时 [`decoy::写噪声`] 直接返回 0 —— 一个字节都不写、也不报事件。
fn 写噪声带进度<W: Write>(
    out: &mut W,
    decoy: &DecoyOptions,
    种子: u64,
    进度: &mut dyn FnMut(PackTick),
) -> Result<u64> {
    let n = crate::pack::decoy::写噪声(out, decoy, 种子)?;
    if n > 0 {
        进度(PackTick::Wrote(n, None));
    }
    Ok(n)
}

/// 写 `free` 原子头，返回写入的字节数
///
/// MP4 规范：`size(4, 大端) + type(4)`；`size` 为 1 时表示后面跟 8 字节真实长度。
fn write_free_atom_header<W: Write>(out: &mut W, payload_len: u64) -> Result<u64> {
    const LARGE_THRESHOLD: u64 = u32::MAX as u64 - 16;
    if payload_len > LARGE_THRESHOLD {
        // 64 位形式：size=1 + 'free' + 8 字节真实大小
        out.write_all(&1u32.to_be_bytes())
            .and_then(|_| out.write_all(b"free"))
            .and_then(|_| out.write_all(&(payload_len + 16).to_be_bytes()))
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("free atom (64): {e}")))?;
        Ok(16)
    } else {
        let size = (payload_len + 8) as u32;
        out.write_all(&size.to_be_bytes())
            .and_then(|_| out.write_all(b"free"))
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("free atom: {e}")))?;
        Ok(8)
    }
}

/// 按 [`PackOptions::format`] 选写入器（T17-4）
///
/// 只有**普通压缩**走这里 —— 伪装 / 自解压永远 ZIP（理由见 [`PackFormat`]
/// 的类型注释：封面 + tar 系的包定位不出来，探针实测过）。
fn write_archive(
    inputs: &[PathBuf],
    tmp: &Path,
    opts: &PackOptions,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    match opts.format {
        PackFormat::Zip => write_zip(inputs, tmp, opts, 进度),
        PackFormat::TarGz | PackFormat::TarZst => write_tar(inputs, tmp, opts, 进度),
    }
}

/// 把输入打成 tar（外面套 gzip 或 zstd）
///
/// 与 [`write_zip`] 的三处有意为之的不同：
/// 1. **mtime 不用手填**：`append_path_with_name` 自己读文件的修改时间
///    （ZIP 那边得手工填，因为不填会变成 1980-01-01）；
/// 2. **目录要显式加条目**：tar 没有「隐式路径」，空目录不加就丢了；
/// 3. **名字用相对顶层名**：`/home/…/照片/a.jpg` → `照片/a.jpg`，
///    与 ZIP 侧「保留顶层目录名」一致。
fn write_tar(
    inputs: &[PathBuf],
    tmp: &Path,
    opts: &PackOptions,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    use std::io::Write as _;
    let file = fs::File::create(tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("create tmp: {e}")))?;
    let buf = std::io::BufWriter::new(file);
    // 防炸链（T18-3）：与 ZIP 侧同一个约定 —— 开着才抖，关着一律用文件真实 mtime
    let 抖种子 = opts.tweak_time.then(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED)
    });

    // 两个分支只差「外面套什么」，里面都是 tar::Builder
    let count;
    match opts.format {
        PackFormat::TarGz => {
            let enc = flate2::write::GzEncoder::new(buf, flate2::Compression::new(opts.level));
            let mut tb = tar::Builder::new(enc);
            count = 加tar条目(&mut tb, inputs, 抖种子, 进度)?;
            let enc = tb
                .into_inner()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tar 收尾: {e}")))?;
            let mut f = enc
                .finish()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("gzip 收尾: {e}")))?;
            f.flush()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("gzip flush: {e}")))?;
        }
        PackFormat::TarZst => {
            // zstd 的「0 级」是「用库默认值」，不是「不压缩」—— level 0 要往上抬到 1，
            // 否则界面选「仅存储」的 zstd 包反而被压了一遍，语义对不上。
            let 级 = opts.level.max(1) as i32;
            let enc = zstd::stream::write::Encoder::new(buf, 级)
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zstd 初始化: {e}")))?;
            let mut tb = tar::Builder::new(enc);
            count = 加tar条目(&mut tb, inputs, 抖种子, 进度)?;
            let enc = tb
                .into_inner()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tar 收尾: {e}")))?;
            let mut f = enc
                .finish()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zstd 收尾: {e}")))?;
            f.flush()
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zstd flush: {e}")))?;
        }
        PackFormat::Zip => unreachable!("write_tar 只处理 tar 系"),
    }
    Ok(count)
}

/// 按 tar 的规则把顶层输入逐个加进去（目录递归）
fn 加tar条目<W: Write>(
    tb: &mut tar::Builder<W>,
    inputs: &[PathBuf],
    抖种子: Option<u64>,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    let mut count = 0usize;
    for input in inputs {
        let meta = fs::metadata(input)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("stat {input:?}: {e}")))?;
        let base: PathBuf = input
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("item"));
        if meta.is_dir() {
            count += tar走目录(tb, input, &base, 抖种子, 进度)?;
        } else {
            tar加文件(tb, input, &base, &meta, 抖种子)?;
            count += 1;
            进度(PackTick::Wrote(meta.len(), Some(base.to_string_lossy().to_string())));
        }
    }
    Ok(count)
}

/// 往 tar 里加一个文件（T18-3：开了防炸链就手动造 header 改 mtime）
///
/// 为什么不能一直用 `append_path_with_name`：它自己读文件的 mtime，
/// 没法改 —— 而「防炸链」要改的正是这个。所以开着时手动造 header：
/// `set_metadata` 把权限/uid 等照抄文件，`set_mtime` 换成抖过的值，再 `set_cksum`。
fn tar加文件<W: Write>(
    tb: &mut tar::Builder<W>,
    src: &Path,
    arc: &Path,
    meta: &fs::Metadata,
    抖种子: Option<u64>,
) -> Result<()> {
    let Some(种子) = 抖种子 else {
        // 没开防炸链：老行为，tar crate 按文件真实时间写
        return tb.append_path_with_name(src, arc).map_err(|e| {
            Error::new(ErrorCode::Internal).context(format!("tar 加入 {src:?}: {e}"))
        });
    };
    let mut header = tar::Header::new_gnu();
    header.set_metadata(meta);
    if let Some(t) = 抖动时间(meta.modified().ok(), 种子, &arc.to_string_lossy()) {
        if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
            header.set_mtime(d.as_secs());
        }
    }
    header.set_cksum();
    let mut f = fs::File::open(src).map_err(|e| {
        Error::new(ErrorCode::Internal).context(format!("open {src:?}: {e}"))
    })?;
    tb.append_data(&mut header, arc, &mut f).map_err(|e| {
        Error::new(ErrorCode::Internal).context(format!("tar 加入 {src:?}: {e}"))
    })?;
    Ok(())
}

/// tar 的目录递归
///
/// `read_dir` 的顺序由文件系统决定，两次可能不一样 —— 这里**排序**，
/// 否则「同一份输入打两次逐字节一致」那条验收会时绿时红。
fn tar走目录<W: Write>(
    tb: &mut tar::Builder<W>,
    dir: &Path,
    prefix: &Path,
    抖种子: Option<u64>,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    tb.append_dir(prefix, dir)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tar 目录 {dir:?}: {e}")))?;
    let rd = fs::read_dir(dir)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read_dir {dir:?}: {e}")))?;
    let mut 条目: Vec<_> = rd
        .map(|e| e.map_err(|e| Error::new(ErrorCode::Internal).context(format!("entry: {e}"))))
        .collect::<Result<Vec<_>>>()?;
    条目.sort_by_key(|e| e.file_name());

    let mut n = 0usize;
    for ent in 条目 {
        let p = ent.path();
        let arc = prefix.join(ent.file_name());
        let meta = ent
            .metadata()
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("meta: {e}")))?;
        if meta.is_dir() {
            n += tar走目录(tb, &p, &arc, 抖种子, 进度)?;
        } else {
            tar加文件(tb, &p, &arc, &meta, 抖种子)?;
            n += 1;
            进度(PackTick::Wrote(meta.len(), Some(arc.to_string_lossy().to_string())));
        }
    }
    Ok(n)
}

/// 把输入路径（文件或目录，可递归）打成 ZIP
fn write_zip(
    inputs: &[PathBuf],
    tmp: &Path,
    opts: &PackOptions,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    let file = fs::File::create(tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("create tmp: {e}")))?;
    let mut zw = zip::ZipWriter::new(file);

    let method = if opts.level == 0 {
        zip::CompressionMethod::Stored
    } else {
        zip::CompressionMethod::Deflated
    };
    // 每个条目要带**自己的**修改时间，所以选项按文件就地构造、不共用
    // （共用会让所有条目都变成 ZIP 格式起点 1980-01-01 —— 实测踩到过）
    let pw = opts.password.as_deref();

    // 防炸链（T18-3）：种子每次打包都换（系统时间）—— 这种功能的目的就是每次都不同
    let 抖种子 = opts.tweak_time.then(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED)
    });

    let mut count = 0usize;
    for input in inputs {
        let meta = fs::metadata(input)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("stat {input:?}: {e}")))?;
        if meta.is_dir() {
            // 保留顶层目录名（对齐生成方的 arcname = <dir>/<rel>）
            let base_name = input
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "dir".into());
            count += add_dir(&mut zw, input, &base_name, method, opts.level as i64, pw, 抖种子, 进度)?;
        } else {
            let name = input
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            let 原时间 = meta.modified().ok();
            let 时间 = match 抖种子 {
                Some(s) => 抖动时间(原时间, s, &name),
                None => 原时间,
            };
            let fopt = 条目选项(method, opts.level as i64, 时间, pw)?;
            zw.start_file(&name, fopt)
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zip start: {e}")))?;
            let mut f = fs::File::open(input).map_err(|e| {
                Error::new(ErrorCode::Internal).context(format!("open {input:?}: {e}"))
            })?;
            // 逐块写 + 逐块报：单文件几 GB 时界面也有动静（每 4 MiB 报一次）
            let mut buf = vec![0u8; 4 * 1024 * 1024];
            loop {
                use std::io::Read as _;
                use std::io::Write as _;
                let n = f.read(&mut buf).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("read {input:?}: {e}"))
                })?;
                if n == 0 {
                    break;
                }
                zw.write_all(&buf[..n]).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("zip write: {e}"))
                })?;
                进度(PackTick::Wrote(n as u64, Some(name.clone())));
            }
            count += 1;
        }
    }

    zw.finish()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zip finish: {e}")))?;
    Ok(count)
}

/// 给一个条目构造 ZIP 选项（带上它**自己的**修改时间）
///
/// 为什么要带时间：不写的话 ZIP 里存的就是格式起点 1980-01-01，
/// 解出来所有文件都会变成那个假时间（实测踩到：解出的文件全是 1980-01-01 08:00）。
/// **也不能共用一个 options** —— 那样所有条目只能有一个时间。
fn 条目选项<'a>(
    method: zip::CompressionMethod,
    level: i64,
    mtime: Option<std::time::SystemTime>,
    pw: Option<&'a str>,
) -> Result<zip::write::FileOptions<'a, 'a, ()>> {
    let mut o = zip::write::SimpleFileOptions::default().compression_method(method);
    // 「仅存储」（level=0）不能带压缩级别：zip crate 会直接拒
    // `Unsupported compression level`（实测踩到 —— level=0 打包整个失败）。
    if method != zip::CompressionMethod::Stored {
        o = o.compression_level(Some(level));
    }
    if let Some(dt) = mtime.and_then(系统时间转_zip_时间) {
        o = o.last_modified_time(dt);
    }
    match pw {
        Some(p) => o
            .with_deprecated_encryption(p.as_bytes())
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("加密选项无效: {e}"))),
        None => Ok(o),
    }
}

/// 系统时间 → ZIP 的「年月日时分秒」
///
/// ZIP 的 DOS 时间戳只能表示 1980–2107 年；超范围（或早于 1970）返回 `None`，
/// 让它保持默认 —— 硬塞上去会得到一个荒唐的日期。
fn 系统时间转_zip_时间(t: std::time::SystemTime) -> Option<zip::DateTime> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let (y, m, d, hh, mm, ss) = crate::restore::unix_to_civil_本地(secs);
    if !(1980..=2107).contains(&y) {
        return None;
    }
    zip::DateTime::from_date_and_time(y as u16, m as u8, d as u8, hh as u8, mm as u8, ss as u8).ok()
}

/// 「防炸链」：按「条目路径 + 本次种子」派生一个时间戳（T18-3）
///
/// 网盘按内容哈希判重 —— 同一份东西反复上传会秒传 / 炸链。时间戳是 ZIP / tar 头里的
/// 字段，改了它整包字节就变、哈希也变，网盘当成新文件。
///
/// 三个刻意的选择：
/// 1. **不是「全改成现在」**：几十个条目时间一模一样反而一眼假，看着就像程序造的。
///    这里落在 [5 年前, 现在] 之间，像一份正常的旧备份；
/// 2. **按「路径 + 种子」派生，不调全局随机**：同一包里不同文件不会撞成同一秒，
///    而且同一次打包里同一个文件的时间是稳定的（否则自检/复算对不上）；
/// 3. **每次打包换种子**（系统时间）—— 防炸链的目的就是每次不一样。所以开着它时
///    「同一份输入两次打包逐字节一致」**不再成立**；没开时那条约定照旧（别混）。
///
/// 永远不出 1970 / 1980：区间下限是「现在 - 5 年」，且秒数被 `max(0)` 兜住。
fn 抖动时间(原: Option<std::time::SystemTime>, 种子: u64, 路径: &str) -> Option<std::time::SystemTime> {
    use std::time::{Duration, UNIX_EPOCH};
    let _ = 原; // 开着抖动时一律用派生值，原来的 mtime 不参与（它正是要被打乱的东西）
    let 现在 = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let 最早 = (现在 - 5 * 365 * 86_400).max(0);
    // FNV 式混入路径，再 xorshift 打散 —— 不引 rand（依赖最小原则）
    let mut x = 种子 ^ 0x9E37_79B9_7F4A_7C15;
    for b in 路径.as_bytes() {
        x = x.wrapping_mul(0x100_0000_01b3).wrapping_add(*b as u64);
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let 跨度 = (现在 - 最早).max(1) as u64;
    let 秒 = 最早 + (x % 跨度) as i64;
    Some(UNIX_EPOCH + Duration::from_secs(秒.max(0) as u64))
}

fn add_dir<W: Write + Seek>(
    zw: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
    method: zip::CompressionMethod,
    level: i64,
    pw: Option<&str>,
    抖种子: Option<u64>,
    进度: &mut dyn FnMut(PackTick),
) -> Result<usize> {
    let mut n = 0usize;
    let rd = fs::read_dir(dir)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read_dir {dir:?}: {e}")))?;
    for ent in rd {
        let ent =
            ent.map_err(|e| Error::new(ErrorCode::Internal).context(format!("entry: {e}")))?;
        let p = ent.path();
        let name = ent.file_name().to_string_lossy().to_string();
        let arc = format!("{prefix}/{name}");
        let meta = ent
            .metadata()
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("meta: {e}")))?;
        if meta.is_dir() {
            n += add_dir(zw, &p, &arc, method, level, pw, 抖种子, 进度)?;
        } else {
            let 原时间 = meta.modified().ok();
            let 时间 = match 抖种子 {
                Some(s) => 抖动时间(原时间, s, &arc),
                None => 原时间,
            };
            let fopt = 条目选项(method, level, 时间, pw)?;
            zw.start_file(&arc, fopt)
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("zip start: {e}")))?;
            let mut f = fs::File::open(&p)
                .map_err(|e| Error::new(ErrorCode::Internal).context(format!("open {p:?}: {e}")))?;
            let mut buf = vec![0u8; 4 * 1024 * 1024];
            loop {
                use std::io::Read as _;
                use std::io::Write as _;
                let k = f.read(&mut buf).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("read {p:?}: {e}"))
                })?;
                if k == 0 {
                    break;
                }
                zw.write_all(&buf[..k]).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("zip write: {e}"))
                })?;
                进度(PackTick::Wrote(k as u64, Some(arc.clone())));
            }
            n += 1;
        }
    }
    Ok(n)
}

/// **产物自检**：把刚写出的伪装包读回来，确认能定位、能列条目、条目数一致
///
/// 对齐生成方的教训（`Steganographier.py` 的输出目录快照仲裁）：
/// **不能"以为成功了"就完事**，必须回读验证。
pub fn self_check(path: &Path, expect_entries: usize) -> Result<Vec<String>> {
    self_check_at(path, expect_entries, None, 0)
}

/// 同 [`self_check`]，另外核对「ZIP 起点」与「噪声长度」（`--decoy` 的验收）
///
/// 为什么用「起点比对」而不是「再解压一遍」：
/// 噪声全在 ZIP 区间之外 ⇒ 解出来的内容必然与不加干扰时完全一致，
/// 起点比对是这条性质**更严格**的验证（字节级），代价只是一次定位。
/// 真解压一遍留在 `tests/decoy.sh` 里跑（那里有 `--test` 与产物 sha256 对比）。
pub fn self_check_at(
    path: &Path,
    expect_entries: usize,
    expect_zip_offset: Option<u64>,
    noise_size: u64,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let mut f = fs::File::open(path)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("open out: {e}")))?;
    let size = f
        .metadata()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("meta: {e}")))?
        .len();

    let layout = crate::layout::zip::locate(&mut f, size).map_err(|e| {
        Error::new(ErrorCode::NotArchive).context(format!("自检失败：产物定位不了 ZIP（{e}）"))
    })?;

    let entries = crate::layout::zip::list_entries(&mut f, &layout).map_err(|e| {
        Error::new(ErrorCode::NotArchive).context(format!("自检失败：列不出条目（{e}）"))
    })?;

    if entries.len() != expect_entries {
        warnings.push(format!(
            "自检：条目数不一致（写入 {expect_entries}，读回 {}）",
            entries.len()
        ));
    }
    if layout.base == 0 && layout.entries_total > 0 && expect_zip_offset.is_some() {
        warnings.push("自检：ZIP 起点为 0，说明封面没有真正包裹住数据".to_string());
    }
    if let Some(exp) = expect_zip_offset {
        if layout.base != exp {
            warnings.push(format!(
                "自检：ZIP 起点应为 {exp} 字节，实际 {}",
                layout.base
            ));
        }
        if noise_size > 0 && layout.base < noise_size {
            warnings.push(format!(
                "自检：ZIP 起点 {} 落在 {} 字节干扰数据之内 —— 噪声写进了压缩数据",
                layout.base, noise_size
            ));
        }
    }
    Ok(warnings)
}

/// 普通压缩（tar 系）的自检：把产物读回来数条目（T17-4）
///
/// ZIP 那套靠 EOCD 定位产物，tar 系没有 EOCD —— 但 tar 的头本身就是索引，
/// 直接用 tar crate 从头读到尾数条目就够了，比 ZIP 那套更直白。
///
/// 只数**非目录条目**：`write_tar` 的 `entries` 也只在文件上 +1
/// （目录条目是 tar 格式的要求，不该算进用户看到的「几个文件」）。
pub fn self_check_tar(
    path: &Path,
    format: PackFormat,
    expect_entries: usize,
) -> Result<Vec<String>> {
    let f = fs::File::open(path)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("open out: {e}")))?;
    let reader: Box<dyn std::io::Read> = match format {
        PackFormat::TarGz => Box::new(flate2::read::GzDecoder::new(f)),
        PackFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(f).map_err(|e| {
            Error::new(ErrorCode::Internal).context(format!("zstd 解压器建不起来: {e}"))
        })?),
        PackFormat::Zip => unreachable!("self_check_tar 只处理 tar 系"),
    };
    let mut ar = tar::Archive::new(reader);
    let mut n = 0usize;
    let 条目 = ar
        .entries()
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tar 读不起来: {e}")))?;
    for e in 条目 {
        let e = e.map_err(|e| {
            Error::new(ErrorCode::Internal).context(format!("tar 条目坏: {e}"))
        })?;
        if !e.header().entry_type().is_dir() {
            n += 1;
        }
    }
    let mut warnings = Vec::new();
    if n != expect_entries {
        warnings.push(format!("自检：条目数不一致（写入 {expect_entries}，读回 {n}）"));
    }
    Ok(warnings)
}

/// 读一个文件的前若干字节（用于验证封面是不是合法视频/图片）
pub fn sniff_head(path: &Path, n: usize) -> Result<Vec<u8>> {
    let mut f = fs::File::open(path)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("open: {e}")))?;
    let mut buf = vec![0u8; n];
    let got = f
        .read(&mut buf)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read: {e}")))?;
    buf.truncate(got);
    Ok(buf)
}

// ───────────────────────── 自解压（SFX）─────────────────────────

/// Windows 自解压存根（`scripts/build-sfx-stub.sh` 交叉编译生成）
///
/// 用 `include_bytes!` 编进主程序是**故意的**：用户生成自解压包时不需要外部程序、
/// 不需要联网，也不依赖用户机器上装着 7-Zip。
///
/// 发布前必须先跑构建脚本；没有存根就直接编译失败 —— 不允许悄悄退化成普通 ZIP。
pub const SFX_STUB: &[u8] = include_bytes!("../../assets/sfx/yshjy-sfx.exe");

/// 自解压尾标：`"YSHSFX01" + zip_offset(u64 LE) + zip_size(u64 LE)` = 24 字节
pub const SFX_MAGIC: &[u8; 8] = b"YSHSFX01";

/// Linux `.sh` 自解压存根模板（P23-5）
///
/// 占位符 **20 位定宽零填充**：替换前后长度完全一致，所以「脚本长度 = ZIP 偏移」
/// 这条自洽关系成立（先按模板长度算偏移，再把数字填进去，不用改两遍）。
/// 最后一行必须是 `exit 0` —— shell 是流式读脚本的，不退出就会把后面的 ZIP 当命令解析。
pub const SFX_SH_STUB: &str = r#"#!/bin/sh
# 杨三海自解压包 —— 由 yshjy 生成
# 用法：双击，或在终端里 sh 这个文件。会解到同目录的「同名文件夹」。
# 有解压码的包：export YSHJY_SFX_PASSWORD=你的密码 再运行（不设会提示输入）。
set -eu
SELF="$0"
case "$SELF" in
  */*) ;;
  *) SELF="./$SELF" ;;
esac
ZIP_OFFSET="00000000000000000000"
ZIP_SIZE="00000000000000000000"
# 归一化：定宽填充可能留空格，前导 0 又会被 shell 当八进制 → 先走一遍算术展开
ZIP_OFFSET=$(($ZIP_OFFSET))
ZIP_SIZE=$(($ZIP_SIZE))
BASE=$(basename "$SELF")
NAME=${BASE%.*}
TARGET="$NAME"
N=1
while [ -e "$TARGET" ]; do
  TARGET="$NAME ($N)"
  N=$((N + 1))
done
TMP="${TMPDIR:-/tmp}/yshjy-sfx-$$.zip"
trap 'rm -f "$TMP"' EXIT INT TERM
tail -c +$((ZIP_OFFSET + 1)) "$SELF" | head -c "$ZIP_SIZE" > "$TMP"
mkdir -p "$TARGET"
if command -v unzip >/dev/null 2>&1; then
  if [ -n "${YSHJY_SFX_PASSWORD:-}" ]; then
    unzip -q -o -P "$YSHJY_SFX_PASSWORD" "$TMP" -d "$TARGET"
  else
    unzip -q -o "$TMP" -d "$TARGET"
  fi
elif command -v bsdtar >/dev/null 2>&1; then
  bsdtar -xf "$TMP" -C "$TARGET"
elif command -v python3 >/dev/null 2>&1; then
  python3 -c 'import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$TMP" "$TARGET"
else
  echo "需要 unzip / bsdtar / python3 三者之一才能解开这个包" >&2
  exit 1
fi
echo "解压完成：$TARGET"
# 解完自动打开输出目录 —— 只对「真人自己跑」开，自动化场景安静收场：
#   ① YSHJY_SFX_NO_OPEN=1 显式关（CI / 脚本一律用它，最高优先）
#   ② stdout 是终端（真人在终端里敲 ./包.sh）→ 开
#   ③ stdout 被重定向/管道，但父进程不是 shell（= 在文件管理器里双击）→ 开
#   ④ 其余（`sh 包.sh > log`、批量脚本调用）→ 不开
if [ -z "${YSHJY_SFX_NO_OPEN:-}" ] && command -v xdg-open >/dev/null 2>&1; then
  sfx_open=1
  if [ ! -t 1 ]; then
    case "$(ps -o comm= -p "$PPID" 2>/dev/null)" in
      bash|dash|sh|zsh|ksh|fish) sfx_open=0 ;;
    esac
  fi
  if [ "$sfx_open" = 1 ]; then
    xdg-open "$TARGET" >/dev/null 2>&1 || true
  fi
fi
exit 0
"#;

/// **生成 Windows 自解压 exe**：`[存根][ZIP 数据][尾标 24B]`
///
/// 和伪装包不是一回事：这里不追求「能播放/能打开」，exe 自己就是程序，
/// 双击后由存根把尾部 ZIP 解到旁边的新文件夹。
///
/// 暂不支持的（明确报错，不静默降级）：解压码、分卷。
pub fn build_sfx<W: Write>(opts: &PackOptions, out: &mut W) -> Result<PackReport> {
    build_sfx_ctx(opts, out, &mut |_| {})
}

/// 同 [`build_sfx`]，但逐块报进度（`PACK_PROGRESS` 的数据源）
pub fn build_sfx_ctx<W: Write>(
    opts: &PackOptions,
    out: &mut W,
    进度: &mut dyn FnMut(PackTick),
) -> Result<PackReport> {
    if opts.inputs.is_empty() {
        return Err(Error::new(ErrorCode::Internal).with("reason", "没有要打包的文件"));
    }

    let tmp = std::env::temp_dir().join(format!(
        "yshjy_sfx_{}_{}.zip",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let entries = write_zip(&opts.inputs, &tmp, opts, 进度)?;
    let zip_size = fs::metadata(&tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tmp meta: {e}")))?
        .len();

    let 存根大小 = SFX_STUB.len() as u64;
    out.write_all(SFX_STUB)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write stub: {e}")))?;
    进度(PackTick::TotalHint(zip_size));
    拷贝带进度(&tmp, out, 进度)?;
    // 尾标：存根只认最后 24 字节，偏移是「ZIP 起点在 exe 里的绝对位置」
    out.write_all(SFX_MAGIC)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write magic: {e}")))?;
    out.write_all(&存根大小.to_le_bytes())
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write offset: {e}")))?;
    out.write_all(&zip_size.to_le_bytes())
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write size: {e}")))?;

    if 保留中间产物已开启() {
        // `--keep`：留着中间 ZIP
    } else {
        let _ = fs::remove_file(&tmp);
    }

    Ok(PackReport {
        total_size: 存根大小 + zip_size + 24,
        zip_size,
        cover_size: 存根大小,
        entries,
        layout: PackLayout::TailAppend,
        noise_size: 0,
        zip_offset: 存根大小,
        warnings: Vec::new(),
        intermediate: None,
    })
}

/// **生成 Linux 自解压 .sh**：`[shell 脚本][ZIP 数据]`
///
/// 与 Windows 版共用同一份 ZIP（`write_zip`），差别只在「外壳」是文本脚本：
/// 偏移和长度作为常量写进脚本里（定宽占位替换，前后长度一致），不需要尾标。
pub fn build_sfx_sh<W: Write>(opts: &PackOptions, out: &mut W) -> Result<PackReport> {
    build_sfx_sh_ctx(opts, out, &mut |_| {})
}

/// 同 [`build_sfx_sh`]，但逐块报进度（`PACK_PROGRESS` 的数据源）
pub fn build_sfx_sh_ctx<W: Write>(
    opts: &PackOptions,
    out: &mut W,
    进度: &mut dyn FnMut(PackTick),
) -> Result<PackReport> {
    if opts.inputs.is_empty() {
        return Err(Error::new(ErrorCode::Internal).with("reason", "没有要打包的文件"));
    }

    let tmp = std::env::temp_dir().join(format!(
        "yshjy_sfx_sh_{}_{}.zip",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let entries = write_zip(&opts.inputs, &tmp, opts, 进度)?;
    let zip_size = fs::metadata(&tmp)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("tmp meta: {e}")))?
        .len();

    // 脚本长度 = ZIP 偏移（占位符定宽，替换不改变长度）
    let 偏移 = SFX_SH_STUB.len() as u64;
    // 用**空格**右对齐填充而不是零填充：`$((000123))` 会被 shell 当八进制，
    // 一旦数字里出现 8/9 就直接算错（实测：ZIP 提取偏移错 100 字节，unzip 报找不到目录）。
    let 脚本 = SFX_SH_STUB
        .replace(
            "ZIP_OFFSET=\"00000000000000000000\"",
            &format!("ZIP_OFFSET=\"{偏移:>20}\"")[..],
        )
        .replace(
            "ZIP_SIZE=\"00000000000000000000\"",
            &format!("ZIP_SIZE=\"{zip_size:>20}\"")[..],
        );
    debug_assert_eq!(
        脚本.len(),
        SFX_SH_STUB.len(),
        "占位符必须定宽替换，否则偏移会算错"
    );

    out.write_all(脚本.as_bytes())
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("write sh stub: {e}")))?;
    进度(PackTick::TotalHint(zip_size));
    拷贝带进度(&tmp, out, 进度)?;

    if 保留中间产物已开启() {
        // `--keep`：留着中间 ZIP
    } else {
        let _ = fs::remove_file(&tmp);
    }

    Ok(PackReport {
        total_size: 偏移 + zip_size,
        zip_size,
        cover_size: 偏移,
        entries,
        layout: PackLayout::TailAppend,
        noise_size: 0,
        zip_offset: 偏移,
        warnings: Vec::new(),
        intermediate: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_pack_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// 伪装 / 自解压打包原来走的是空回调 + `io::copy`，进度条整段是死的。
    /// 这两条盯着「三件事都得有」：压缩阶段逐块报、拼装前告知新总量、拼装阶段逐块报。
    #[test]
    fn 伪装打包会逐块报进度并把拼装总量告知外壳() {
        let d = scratch("progress_disguise");
        let 大小 = 512 * 1024;
        fs::write(d.join("大文件.bin"), vec![7u8; 大小]).unwrap();
        let opts = PackOptions {
            inputs: vec![d.clone()],
            ..Default::default()
        };
        let mut 出 = Vec::new();
        let mut 记录: Vec<PackTick> = Vec::new();
        let rep = {
            let mut 报 = |t: PackTick| 记录.push(t);
            build_ctx(&opts, &mut 出, &mut 报).unwrap()
        };

        let 压缩: u64 = 记录
            .iter()
            .filter_map(|t| match t {
                PackTick::Wrote(n, Some(_)) => Some(*n),
                _ => None,
            })
            .sum();
        assert_eq!(压缩, 大小 as u64, "压缩阶段报的字节数应等于输入字节数");

        let 提示 = 记录
            .iter()
            .find_map(|t| match t {
                PackTick::TotalHint(n) => Some(*n),
                _ => None,
            })
            .expect("拼装前必须告知外壳新总量，否则进度条会卡在压缩阶段的百分比上");
        assert_eq!(提示, rep.zip_size);

        let 拼装: u64 = 记录
            .iter()
            .filter_map(|t| match t {
                PackTick::Wrote(n, None) => Some(*n),
                _ => None,
            })
            .sum();
        assert_eq!(拼装, rep.zip_size, "拼装阶段报的字节数应等于中间 ZIP 大小");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 自解压打包也报进度() {
        let d = scratch("progress_sfx");
        fs::write(d.join("a.bin"), vec![9u8; 256 * 1024]).unwrap();
        let opts = PackOptions {
            inputs: vec![d.clone()],
            ..Default::default()
        };
        let mut 出 = Vec::new();
        let mut 条数 = 0usize;
        let mut 总量提示 = None;
        {
            let mut 报 = |t: PackTick| {
                条数 += 1;
                if let PackTick::TotalHint(n) = t {
                    总量提示 = Some(n);
                }
            };
            let rep = build_sfx_sh_ctx(&opts, &mut 出, &mut 报).unwrap();
            assert!(条数 > 2, "自解压打包以前是空回调，一条进度都没有");
            assert_eq!(总量提示, Some(rep.zip_size));
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn free原子头_小尺寸用32位形式() {
        let mut out = Vec::new();
        let n = write_free_atom_header(&mut out, 100).unwrap();
        assert_eq!(n, 8);
        assert_eq!(u32::from_be_bytes([out[0], out[1], out[2], out[3]]), 108);
        assert_eq!(&out[4..8], b"free");
    }

    #[test]
    fn free原子头_超大尺寸用64位形式() {
        let mut out = Vec::new();
        let n = write_free_atom_header(&mut out, 5_000_000_000).unwrap();
        assert_eq!(n, 16);
        assert_eq!(
            u32::from_be_bytes([out[0], out[1], out[2], out[3]]),
            1,
            "size=1 表示 64 位形式"
        );
        assert_eq!(&out[4..8], b"free");
        let real = u64::from_be_bytes(out[8..16].try_into().unwrap());
        assert_eq!(real, 5_000_000_000 + 16);
    }

    #[test]
    fn 打一个伪装包能被我们自己读回() {
        let dir = scratch("basic");
        fs::write(dir.join("a.txt"), b"hello pack").unwrap();
        fs::write(dir.join("b.bin"), vec![7u8; 3000]).unwrap();

        let opts = PackOptions {
            inputs: vec![dir.join("a.txt"), dir.join("b.bin")],
            ..Default::default()
        };
        let mut out = Vec::new();
        build(&opts, &mut out).unwrap();

        // 落盘 → 自检（读回来定位 + 列条目）
        let outfile = dir.join("fake.mp4");
        fs::write(&outfile, &out).unwrap();
        let warns = self_check(&outfile, 2).unwrap();
        assert!(warns.is_empty(), "自检应通过: {warns:?}");

        // 用我们的解压引擎把它解回来
        let dest = scratch("unpack");
        let mut cur = Cursor::new(out);
        let size = cur.get_ref().len() as u64;
        let l = crate::layout::zip::locate(&mut cur, size).unwrap();
        let rep = crate::extract::extract_zip_at(
            &mut cur,
            l.base,
            l.data_len(),
            &crate::extract::ExtractOptions::new(&dest),
        )
        .unwrap();
        assert_eq!(rep.files.len(), 2, "应解回 2 个: {:?}", rep.skipped);
        assert_eq!(
            fs::read_to_string(dest.join("a.txt")).unwrap(),
            "hello pack"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 伪装包开头的封面仍是合法mp4() {
        let dir = scratch("cover");
        fs::write(dir.join("x.txt"), b"data").unwrap();
        let opts = PackOptions {
            inputs: vec![dir.join("x.txt")],
            ..Default::default()
        };
        let mut out = Vec::new();
        build(&opts, &mut out).unwrap();
        // 前 8 字节应是内置素材（MP4 头）
        assert_eq!(&out[4..8], b"ftyp", "外壳必须是合法 MP4");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 普通压缩是标准zip没有封面() {
        let dir = scratch("plain");
        fs::write(dir.join("x.txt"), b"plain mode").unwrap();
        let opts = PackOptions {
            inputs: vec![dir.join("x.txt")],
            ..Default::default()
        };
        let mut out = Vec::new();
        let rep = build_plain(&opts, &mut out).unwrap();

        // 1) 产物开头就是 ZIP 本地文件头 —— 不是「MP4 头 + 后缀」
        assert_eq!(&out[..4], b"PK\x03\x04", "普通压缩必须就是标准 ZIP");
        // 2) 没有封面区
        assert_eq!(rep.cover_size, 0);
        assert_eq!(rep.total_size, out.len() as u64);

        // 3) 标准解码库能直接读（不需要任何「定位」逻辑）
        let mut ar = zip::ZipArchive::new(std::io::Cursor::new(out)).unwrap();
        assert_eq!(ar.len(), 1);
        let mut s = String::new();
        std::io::Read::read_to_string(&mut ar.by_index(0).unwrap(), &mut s).unwrap();
        assert_eq!(s, "plain mode");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 尾部追加布局也能读回() {
        let dir = scratch("tail");
        fs::write(dir.join("y.txt"), b"tail mode").unwrap();
        let opts = PackOptions {
            inputs: vec![dir.join("y.txt")],
            layout: PackLayout::TailAppend,
            ..Default::default()
        };
        let mut out = Vec::new();
        build(&opts, &mut out).unwrap();
        let outfile = dir.join("tail.mp4");
        fs::write(&outfile, &out).unwrap();
        assert!(self_check(&outfile, 1).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 带密码打包能解回来() {
        let dir = scratch("pw");
        fs::write(dir.join("secret.txt"), "机密内容").unwrap();
        let opts = PackOptions {
            inputs: vec![dir.join("secret.txt")],
            password: Some("密码123".into()),
            ..Default::default()
        };
        let mut out = Vec::new();
        build(&opts, &mut out).unwrap();

        let dest = scratch("pw_out");
        let mut cur = Cursor::new(out);
        let size = cur.get_ref().len() as u64;
        let l = crate::layout::zip::locate(&mut cur, size).unwrap();
        let mut eopts = crate::extract::ExtractOptions::new(&dest);
        eopts.password = Some("密码123".into());
        let rep = crate::extract::extract_zip_at(&mut cur, l.base, l.data_len(), &eopts).unwrap();
        assert_eq!(rep.files.len(), 1, "带密码的包要能解回: {:?}", rep.skipped);
        assert_eq!(
            fs::read_to_string(dest.join("secret.txt")).unwrap(),
            "机密内容"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 目录输入保留顶层目录名() {
        let dir = scratch("tree");
        let sub = dir.join("我的目录");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("内.txt"), b"nested").unwrap();

        let opts = PackOptions {
            inputs: vec![sub.clone()],
            ..Default::default()
        };
        let mut out = Vec::new();
        build(&opts, &mut out).unwrap();

        let dest = scratch("tree_out");
        let mut cur = Cursor::new(out);
        let size = cur.get_ref().len() as u64;
        let l = crate::layout::zip::locate(&mut cur, size).unwrap();
        let rep = crate::extract::extract_zip_at(
            &mut cur,
            l.base,
            l.data_len(),
            &crate::extract::ExtractOptions::new(&dest),
        )
        .unwrap();
        let names: Vec<_> = rep.files.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("我的目录/内.txt")),
            "应保留顶层目录名: {names:?}"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dest);
    }

    /// 造一个只有一个文件的小输入目录
    fn 小输入(tag: &str) -> (PathBuf, PathBuf) {
        let dir = scratch(tag);
        let f = dir.join("a.txt");
        fs::write(&f, b"hello").unwrap();
        (dir, f)
    }

    #[test]
    fn png_封面必须产出真_png_而不是假图片() {
        // v0.9 修的真 bug：`ys -o 图.png` 曾产出「MP4 头 + .png 后缀」，
        // 因为外壳从没把扩展名传进来。这条测试钉死：封面格式必须跟着扩展名走。
        let (dir, f) = 小输入("cover_png");
        let opts = PackOptions {
            inputs: vec![f],
            cover_ext: "png".into(),
            ..Default::default() // 默认是 FreeAtom —— 必须被自动降级
        };
        let mut out = Vec::new();
        let rep = build(&opts, &mut out).unwrap();

        assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n", "产物必须以 PNG 魔数开头");
        assert_eq!(
            rep.layout,
            PackLayout::TailAppend,
            "PNG 封面不能走 MP4 的 free 原子布局"
        );
        assert!(!rep.warnings.is_empty(), "降级必须如实报告，不能悄悄改");
        // 而且还能被我们自己解回来
        let size = out.len() as u64;
        let mut cur = Cursor::new(out);
        let l = crate::layout::zip::locate(&mut cur, size).unwrap();
        assert_eq!(l.base as usize, rep.cover_size as usize, "ZIP 应紧跟在封面之后");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn jpg_封面必须产出真_jpeg() {
        let (dir, f) = 小输入("cover_jpg");
        let opts = PackOptions {
            inputs: vec![f],
            cover_ext: "jpg".into(),
            ..Default::default()
        };
        let mut out = Vec::new();
        let rep = build(&opts, &mut out).unwrap();
        assert_eq!(&out[..3], &[0xFF, 0xD8, 0xFF], "产物必须以 JPEG 魔数开头");
        assert_eq!(rep.layout, PackLayout::TailAppend);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mp4_封面仍然用_free_原子() {
        // 反向保险：别为了修图片把视频的默认行为也改了
        let (dir, f) = 小输入("cover_mp4");
        let opts = PackOptions {
            inputs: vec![f],
            cover_ext: "mp4".into(),
            ..Default::default()
        };
        let mut out = Vec::new();
        let rep = build(&opts, &mut out).unwrap();
        assert_eq!(&out[4..8], b"ftyp", "MP4 封面的 ftyp 要在");
        assert_eq!(rep.layout, PackLayout::FreeAtom, "MP4 仍走 free 原子");
        assert!(rep.warnings.is_empty(), "正常路径不该有降级警告");
        let _ = fs::remove_dir_all(&dir);
    }
}
