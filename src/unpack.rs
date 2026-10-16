//! 统一解压器接口 —— 「加一个格式只改一处」的地基
//!
//! # 为什么要有这一层
//!
//! v0.8 之前每种格式都是一组**平行函数**，谁都不认识谁：
//!
//! ```text
//! extract_zip_auto_ctx()  extract_7z_volumes_ctx()  extract_rar_file_ctx()
//! extract_tar_ctx()       decompress_stream_ctx()
//! ```
//!
//! 后果有三个（加格式的代价全在这里）：
//! 1. 加一个格式要改**多处**：识别 / 预览 / 解压 / 批量编排 / 参数结构体各改一遍；
//! 2. 格式能力（能不能加密、能不能分卷、能不能只挑一部分）**散在实现里**，
//!    界面要显示「支持加密 🔒」得去读代码；
//! 3. 将来的 GUI / 手机端只能按格式写 `match`，没法泛化调用。
//!
//! 这一层把「格式」收成一个 trait，加格式 = **写一个 `impl InArchive` + 在
//! [`HANDLERS`] 里加一行**，其余地方不用动。
//!
//! # 参考依据（`refs/tech` 台账，逐条实测后取舍）
//!
//! | 出处 | 参考项目怎么做 | 我们怎么落 |
//! |---|---|---|
//! | `7zip-工程层.md` 1.13 | `IInArchive` / `IOutArchive` **读写分离**，只读格式只实现读接口，`CreateOutArchive` 可为 NULL | `trait InArchive`（本轮）+ 将来的 `trait OutArchive`；`Caps::can_create` 表达"能不能反向打包" |
//! | `7zip-工程层.md` 1.14 | 可选能力用 `QueryInterface` 探测，**主接口永不膨胀** | 不抄 QI（Rust 里向下转型易错），改成 [`Caps`] **数据表**：主接口保持窄，能力是 UI 能直接读来画表的数据 |
//! | `7zip-工程层.md` 1.16 | 分卷由 handler **反向索取**（`IArchiveOpenVolumeCallback`），opener 不必懂各格式的卷名规则 | 本轮仍由编排层把卷拼成一条流（`MultiFile`）再交给 handler（接口只认一条流）；反向索取留作将来处理"卷名不规则"的场合，理由写在 [`Source::volumes`] |
//! | `7zip-工程层.md` 7.7 | `CArchiveExtractCallback`（3000+ 行）**双壳共享全部落盘逻辑**，最容易写错的路径安全/覆盖策略只写一份 | 落盘共性将来收进 `OutputSink`（本轮先不动，见「下一步」） |
//! | `xadmaster.md` ①-01 | **handle 是唯一 I/O 抽象**，子流可廉价分叉 | 我们就是 [`Source::reader`]（`&mut dyn ReadSeek`）；偏移由 `io::Window` 消化、多卷由 `MultiFile` 消化 |
//! | `xadmaster.md` ②-01 | 静态**有序注册表，顺序即优先级**（"贪心的、弱的放最后"写成显式注释） | [`HANDLERS`] 显式排序 + 注释分组，新人不会乱插 |
//! | `peazip.md` 3.9 | 每后端一个解压命令构造器（`compose_un7z_cl` / `unzstd` / …），共性是重复的 | 结构映射为**每格式一个 `impl`**，共性下沉（四份参数结构体已合并为 [`UnpackOptions`]） |
//! | `zip2.md` 5.15 | 反面教材：定位算法与解压器**都不可从外部调用**，对外只有一个大类型 | 我们恰恰要暴露：定位（我们的强项）与解码（库的强项）走同一个 trait，谁都调得到 |
//!
//! # 下一步（本层还没做完的部分）
//!
//! - `list()`（预览）还没进 trait：`preview.rs` 现在按「文件路径 + Target」工作，
//!   要改成按「一条流」工作才能归位（下一批）
//! - 实现体目前**委托**给既有函数（`extract_zip_auto_ctx` 等），行为零变化；
//!   后续批次再把落盘逻辑内聚到 `OutputSink`，实现体才会真正搬进 handler
//! - `pack/`（反向打包）侧将来对应 `trait OutArchive`

use crate::ctx::UnpackCtx;
use crate::error::{Error, ErrorCode, Result};
use crate::extract::{ExtractOptions, ExtractReport, Overwrite};
use crate::extract7z::Extract7zOptions;
use crate::extract_rar::ExtractRarOptions;
use crate::extract_stream::ExtractStreamOptions;
use crate::layout::ArchiveFormat;
use crate::preview::Preview;
use crate::safety::Limits;
use crate::select::Selection;
use std::io::{Read, Seek};
use std::path::PathBuf;

/// 单流格式解出文件时的兜底名字
///
/// ⚠️ 这是引擎里**最后几个硬编码中文之一**（`job.rs:351` / `job.rs:543` /
/// `preview.rs:328` 有同样的兜底）。归位语言包是 HANDOFF「零碎」清单里的一条，
/// 等文案统一收口时四处一起改 —— 现在先集中到这里，别再散第 5 处。
pub const 兜底输出名: &str = "解压结果";

/// **可读 + 可定位**的数据源
///
/// 这是整个接口层唯一的 I/O 抽象（`xadmaster.md` ①-01：handle 是唯一 I/O 抽象）。
/// 它是一个 blanket impl，所以任何 `Read + Seek` 都能直接传进来：
///
/// - 普通文件 `std::fs::File`
/// - [`crate::io::Window`]：把「文件里从偏移 N 开始的一段」看成独立文件（伪装包的关键）
/// - [`crate::extract7z::MultiFile`]：把多个分卷虚接成一条连续流
/// - [`crate::io::SegmentReader`]：把被切开分插的数据段虚接起来
///
/// 这样 handler 永远只面对「一条流」，不必知道伪装、偏移、分卷的存在。
pub trait ReadSeek: Read + Seek {}

impl<T: Read + Seek + ?Sized> ReadSeek for T {}

/// 数据源的**形状**（不是格式，是"这东西长什么样"）
///
/// 注册表按 `(格式, 形状)` 查 handler，这样一个格式可以只声明它真正支持的样子：
/// 比如 RAR 只做单文件、7z 只做分卷流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// 普通归档（可能藏在别的文件里，但对我们就是"一条流"）
    Plain,
    /// 多卷（编排层已把各卷拼成一条流）
    Volumes,
    /// 单流压缩：解出一个文件（gzip / xz / zstd / bzip2 / lz4 / brotli）
    Single,
    /// tar 家族：里面是目录树（tar / tar.gz / tar.xz / …）
    Tar,
}

/// 格式能力声明
///
/// 抄的是 7-Zip「能力靠探测、主接口不膨胀」的**结果**，换了个更简单的实现方式：
/// 与其让调用方 `QueryInterface` 试探，不如把能力做成**可读数据** ——
/// 界面要显示「这个包支持加密」直接读 [`Caps::encrypted`]，不用试。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// 支持用密码解开加密条目
    pub encrypted: bool,
    /// 可能连**文件名**都加密（7z `-mhe=on`）—— 无密码时连清单都列不出来
    pub encrypted_names: bool,
    /// 支持分卷
    pub split_volumes: bool,
    /// 支持只解一部分（`--select`）
    pub selective: bool,
    /// 解压时能还原原文件的修改时间
    pub restores_mtime: bool,
    /// 能反向打包成这种格式（对应 7-Zip 的 `out_handler: Option<fn()>`）
    pub can_create: bool,
}

/// 一次解压的数据源
///
/// 编排层负责把「伪装偏移 / 多卷 / 分插段」都消化成一条流，然后填写这个结构。
/// 它同时携带几个 handler **确实需要**的补充信息，而不是让每个 handler 自己去猜。
pub struct Source<'a> {
    /// 数据流（唯一 I/O 抽象）
    pub reader: &'a mut dyn ReadSeek,
    /// 归档在这条流里的起点（用 `Window` 归零过就是 0）
    ///
    /// 保留它是为了不动既有实现：`extract_zip_at` 系列收「流 + base + len」
    /// 而不是「Window」，两种写法等价（源码里打补丁后重试就是 base=0 的用法）。
    pub base: u64,
    /// 这条流里可用的字节数（`None` = 用到流尾）
    pub len: Option<u64>,
    /// 分卷列表（只有 [`Shape::Volumes`] 用得上）
    ///
    /// 为什么不让 handler 自己去找卷（7-Zip 的 `IArchiveOpenVolumeCallback` 做法）：
    /// 卷名规则是**识别层**的知识（`.z01` 链、7z 的 `.001`），而识别层已经知道卷列表了，
    /// 再让每个 handler 学一遍等于把同一份知识复制 N 份。将来遇到"卷名不规则"再引入反向索取。
    pub volumes: &'a [PathBuf],
    /// 已知的压缩后总大小 —— **解压炸弹防护的分母**
    ///
    /// 这个字段是踩过两次坑换来的（见 HANDOFF「同一个坑别踩两次」）：
    /// 分母随手给 1 会让 20 MB 的正常文件被判成"压缩比 5025 倍"。
    /// 所以它**必须**由编排层从真实元数据填，不允许 handler 自己编。
    pub packed_total: u64,
}

impl<'a> Source<'a> {
    /// 以一条流建源（其余字段默认：base 0、无长度上限、无路径、非分卷、压缩大小 0）
    pub fn new(reader: &'a mut dyn ReadSeek) -> Self {
        Self {
            reader,
            base: 0,
            len: None,
            volumes: &[],
            packed_total: 0,
        }
    }

    /// 带上偏移与长度（伪装包场景）
    pub fn with_offset(mut self, base: u64, len: u64) -> Self {
        self.base = base;
        self.len = Some(len);
        self
    }

    /// 带上是哪些分卷
    pub fn with_volumes(mut self, volumes: &'a [PathBuf]) -> Self {
        self.volumes = volumes;
        self
    }

    /// 带上压缩后总大小（炸弹防护的分母）
    pub fn with_packed_total(mut self, packed_total: u64) -> Self {
        self.packed_total = packed_total;
        self
    }

    /// 长度兜底链：**显式给的长度 → 压缩后总大小 → 调用方给的兜底值**
    ///
    /// ⚠️ 它**不**会去问流自己要长度。字段 `len` 为 `None` 的契约是「用到流尾」；
    /// 要按这个语义拿长度，得自己 `seek(End)` 量一次（`ZipUnpacker::list` 就是这么做的）——
    /// 曾经把"未知"当成 0 传给窗口，得到一个长度 0 的窗口，一个字节都读不出来。
    pub fn len_or(&self, fallback: u64) -> u64 {
        match self.len {
            Some(n) => n,
            None => {
                if self.packed_total > 0 {
                    self.packed_total
                } else {
                    fallback
                }
            }
        }
    }
}

/// 统一解压参数
///
/// 合并了原来四份各写一遍的结构体（`ExtractOptions` / `Extract7zOptions` /
/// `ExtractRarOptions` / `ExtractStreamOptions`）—— 它们的字段**本来就是同一组**
/// （dest / password / overwrite / selection / limits），只有单流格式多两个
/// （输出名、压缩大小提示）。四份并一份，是 `peazip.md` 3.9 那条教训的落地。
#[derive(Debug, Clone)]
pub struct UnpackOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 密码（`None` = 不试密码，只解未加密的条目）
    pub password: Option<String>,
    /// 同名文件怎么办
    pub overwrite: Overwrite,
    /// 挑哪些条目
    pub selection: Selection,
    /// 安全限值
    pub limits: Limits,
    /// 单流格式解出来的文件名（`None` = handler 自己从源路径推）
    pub out_name: Option<String>,
    /// 单流：已知的压缩后大小（炸弹防护分母；0 = 用源的 `packed_total`）
    pub packed_hint: u64,
    /// **这一层用的压缩算法**：裸 tar 是 `None`，`tar.gz` 是 `Some(Gzip)`，
    /// 纯单流（`x.xz`）就是它本身
    pub outer: Option<ArchiveFormat>,
}

impl UnpackOptions {
    /// 以输出目录构造默认参数
    pub fn new<P: Into<PathBuf>>(dest: P) -> Self {
        Self {
            dest: dest.into(),
            password: None,
            overwrite: Overwrite::default(),
            selection: Selection::all(),
            limits: Limits::default(),
            out_name: None,
            packed_hint: 0,
            outer: None,
        }
    }

    /// 统一参数 → ZIP 侧既有结构体
    pub fn to_zip(&self) -> ExtractOptions {
        let mut o = ExtractOptions::new(&self.dest);
        o.password = self.password.clone();
        o.overwrite = self.overwrite;
        o.selection = self.selection.clone();
        o.limits = self.limits.clone();
        o
    }

    /// 统一参数 → 7z 侧既有结构体
    pub fn to_7z(&self) -> Extract7zOptions {
        let mut o = Extract7zOptions::new(&self.dest);
        o.password = self.password.clone();
        o.overwrite = self.overwrite;
        o.selection = self.selection.clone();
        o.limits = self.limits.clone();
        o
    }

    /// 统一参数 → RAR 侧既有结构体
    pub fn to_rar(&self) -> ExtractRarOptions {
        let mut o = ExtractRarOptions::new(&self.dest);
        o.password = self.password.clone();
        o.overwrite = self.overwrite;
        o.selection = self.selection.clone();
        o.limits = self.limits.clone();
        o
    }

    /// 统一参数 → 单流侧既有结构体
    pub fn to_stream(&self) -> ExtractStreamOptions {
        let mut o = ExtractStreamOptions::new(&self.dest);
        o.overwrite = self.overwrite;
        o.selection = self.selection.clone();
        o.limits = self.limits.clone();
        o.out_name = self.out_name.clone();
        o.packed_hint = self.packed_hint;
        o
    }
}

/// 统一解压器（读侧）
///
/// 一个格式一个 `impl`。只读格式（如 RAR）也实现这个 trait —— 写能力由
/// [`Caps::can_create`] 表达，对应 7-Zip「`CreateOutArchive` 可以为 NULL」。
pub trait InArchive: Sync {
    /// 我负责**哪些**格式
    ///
    /// 为什么是复数：同一份实现常常覆盖多档格式 —— RAR 4 与 RAR 5 的解压实现是同一份，
    /// 六种单流压缩（gz/xz/zst/bz2/lz4/br）也是同一份。写成单值会逼注册表里塞一堆
    /// 零大小空壳类型，而它们的方法体一模一样。**第一个元素是主格式**（日志/显示用）。
    fn formats(&self) -> &'static [ArchiveFormat];

    /// 我能吃哪些形状的数据源
    fn shapes(&self) -> &'static [Shape];

    /// 我的能力（界面直接读这个画表）
    fn caps(&self) -> Caps;

    /// 解压
    ///
    /// 前提：`src.reader` 已经是**一条流**（偏移由 `Window` 归零、多卷由编排层拼好），
    /// handler 不负责找起点 —— "找起点"是 `scan` / `layout` 的活，也是我们比市面工具强的地方。
    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport>;

    /// 列内容（不解压）—— `--list` 与将来 GUI 的预览面板用
    ///
    /// 参数与 [`InArchive::unpack`] 保持一致（都收 [`UnpackOptions`]，密码在
    /// `opts.password` 里），编排层构造一次参数就能两处用。
    ///
    /// 默认实现**显式报「这个格式还没实现列清单」**，而不是返回空清单：
    /// 空清单会被上层和用户理解成"包里什么都没有"，那是最糟的误导。
    fn list(
        &self,
        _src: &mut Source<'_>,
        _opts: &UnpackOptions,
        _ctx: &UnpackCtx,
    ) -> Result<Preview> {
        Err(Error::new(ErrorCode::NotArchive)
            .with("reason", "list_not_implemented")
            .with("format", self.formats()[0].as_str())
            .context("这个格式还没有实现「只列内容」"))
    }
}

// ─────────────────────────── 各格式的实现 ───────────────────────────
//
// 实现体目前**委托**给既有函数（行为与 v0.8 逐字一致）；
// 后续批次把落盘逻辑内聚到 OutputSink 后，实现体才会真正搬进来。

/// ZIP / ZIP64（含 `.z01` 分卷）
///
/// 定位是我们自己的强项（`layout::zip::locate`，尾部窗口），解码交给 `zip` crate；
/// 索引坏了还能在内存里打补丁重试 —— 这条"自动修复"链路藏在 `extract_zip_auto_ctx` 里，
/// 所以这里调的是 `_auto_` 版本而不是 `extract_zip_at_ctx`。
pub struct ZipUnpacker;

impl InArchive for ZipUnpacker {
    fn formats(&self) -> &'static [ArchiveFormat] {
        &[ArchiveFormat::Zip]
    }

    fn shapes(&self) -> &'static [Shape] {
        // 分卷（.z01 链）与普通 ZIP 走同一条路：编排层已把卷拼成一条流
        &[Shape::Plain, Shape::Volumes]
    }

    fn caps(&self) -> Caps {
        Caps {
            encrypted: true,        // ZipCrypto + AES-256
            encrypted_names: false, // ZIP 的文件名不加密
            split_volumes: true,
            selective: true,
            restores_mtime: true, // v0.8 接的：ZIP 存 DOS 时间
            can_create: true,     // 反向打包主力
        }
    }

    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport> {
        let eopts = opts.to_zip();
        let len = src.len_or(0);
        // 重借用出 `&mut dyn ReadSeek`：先给 reader 一个具体类型，
        // 再把它的 `&mut` 交给泛型函数 —— 泛型参数不能是 unsized 的 `dyn ReadSeek`
        let mut reader: &mut dyn ReadSeek = &mut *src.reader;
        let (mut rep, repair_note) =
            crate::extract::extract_zip_auto_ctx(&mut reader, src.base, len, &eopts, ctx)?;
        // 修过索引这件事要**结构化**地报上去（不是塞在日志里）：
        // 批量汇总据此把任务标成「索引有错但救回来了」
        rep.repaired = repair_note;
        Ok(rep)
    }

    fn list(
        &self,
        src: &mut Source<'_>,
        _opts: &UnpackOptions,
        _ctx: &UnpackCtx,
    ) -> Result<Preview> {
        // ZIP 的文件名**不加密**，所以无密码也能列出清单（这也是它能当"预览主力"的原因）。
        // `_pw` / `_ctx` 暂时用不上，写成 `_` 是为了让 clippy 的未使用检查闭嘴，
        // 同时明确"这个参数在 ZIP 上确实没有语义"。
        //
        // 长度：显式给了就用；没给就按契约「用到流尾」——**量一次流自己**。
        // ⚠️ 别把"未知"当 0：那样窗口长度是 0，一个字节都读不出来，
        //    症状是「明明是好包却报不是压缩包」（写这个 handler 时真踩了一次，
        //    靠新加的测试逮住的）。
        let len = match src.len.or(if src.packed_total > 0 {
            Some(src.packed_total)
        } else {
            None
        }) {
            Some(n) => n,
            None => {
                let end = src.reader.seek(std::io::SeekFrom::End(0)).map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("取流长度失败: {e}"))
                })?;
                end.saturating_sub(src.base)
            }
        };
        let window = crate::io::Window::new(&mut *src.reader, src.base, len)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("window: {e}")))?;
        crate::preview::preview_zip_reader(window)
    }
}

/// 7-Zip（含 `.001` 分卷、AES-256）
pub struct SevenZUnpacker;

impl InArchive for SevenZUnpacker {
    fn formats(&self) -> &'static [ArchiveFormat] {
        &[ArchiveFormat::SevenZ]
    }

    fn shapes(&self) -> &'static [Shape] {
        // 7z 只走分卷这一种形状（单卷也是 MultiFile 里唯一的一卷）
        &[Shape::Volumes]
    }

    fn caps(&self) -> Caps {
        Caps {
            encrypted: true,       // AES-256
            encrypted_names: true, // 7z -mhe=on 可能连文件名都加密
            split_volumes: true,
            selective: true,
            // 诚实结论：`sevenz-rust2` 的高层 API 拿不到每文件的 header，
            // 读不到也就还原不了（详见 CHANGELOG v0.8 的说明）
            restores_mtime: false,
            can_create: false,
        }
    }

    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport> {
        let eopts = opts.to_7z();
        // 数据源只有「一条流」：多卷由编排层拼好（`MultiFile`）再喂进来
        let reader: &mut dyn ReadSeek = &mut *src.reader;
        crate::extract7z::extract_7z_stream_ctx(reader, src.packed_total, &eopts, ctx)
    }

    fn list(&self, src: &mut Source<'_>, opts: &UnpackOptions, _ctx: &UnpackCtx) -> Result<Preview> {
        // 7z 的头可能是加密的（`-mhe=on` 连文件名都加密），所以列清单也可能要密码 ——
        // 密码由编排层放进 `opts.password`（没有就报「需要密码」，不假装能列）
        let reader: &mut dyn ReadSeek = &mut *src.reader;
        crate::preview::preview_7z(reader, opts.password.as_deref())
    }
}

/// RAR（1.3 ~ 7；只解不压）
pub struct RarUnpacker;

impl InArchive for RarUnpacker {
    fn formats(&self) -> &'static [ArchiveFormat] {
        // 格式判定分成 Rar4 / Rar5 两档（魔数不同），但解压实现是同一份
        &[ArchiveFormat::Rar4, ArchiveFormat::Rar5]
    }

    fn shapes(&self) -> &'static [Shape] {
        &[Shape::Plain]
    }

    fn caps(&self) -> Caps {
        Caps {
            encrypted: true,
            encrypted_names: false,
            split_volumes: true,
            selective: true,
            // 诚实结论：走 `rars` 高层 API 拿不到 file_time（详见 extract_rar.rs 模块注释）
            restores_mtime: false,
            // RAR 算法专有，我们只做读取
            can_create: false,
        }
    }

    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport> {
        let eopts = opts.to_rar();
        // `rars` 只提供全量内存读接口，所以 `extract_rar_stream_ctx` 内部会把流读进内存
        // （那里先做 256 MiB 上限检查再决定要不要拒绝，见 extract_rar::MAX_SIZE）
        let mut reader: &mut dyn ReadSeek = &mut *src.reader;
        crate::extract_rar::extract_rar_stream_ctx(&mut reader, &eopts, ctx)
    }

    fn list(
        &self,
        src: &mut Source<'_>,
        _opts: &UnpackOptions,
        _ctx: &UnpackCtx,
    ) -> Result<Preview> {
        // `rars` 也只支持全量内存读：预览函数内部会把流读进来（同样受 256 MiB 上限
        // 保护，超了就如实报「无法预览」，不硬撑）
        let mut reader: &mut dyn ReadSeek = &mut *src.reader;
        crate::preview::preview_rar(&mut reader)
    }
}

/// 单流压缩：解出**一个文件**（gzip / xz / zstd / bzip2 / lz4 / brotli）
pub struct SingleStreamUnpacker;

impl InArchive for SingleStreamUnpacker {
    fn formats(&self) -> &'static [ArchiveFormat] {
        &[
            ArchiveFormat::Gzip,
            ArchiveFormat::Xz,
            ArchiveFormat::Zstd,
            ArchiveFormat::Bzip2,
            ArchiveFormat::Lz4,
            ArchiveFormat::Brotli,
        ]
    }

    fn shapes(&self) -> &'static [Shape] {
        &[Shape::Single]
    }

    fn caps(&self) -> Caps {
        Caps {
            encrypted: false,
            encrypted_names: false,
            split_volumes: false,
            // 单流只有一个产物，没有"挑条目"这回事
            selective: false,
            restores_mtime: false,
            can_create: false,
        }
    }

    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport> {
        // 具体是哪种单流由编排层给出（Brotli 没魔数、只能按扩展名认，所以它自己推不出来）
        let fmt = opts.outer.unwrap_or(ArchiveFormat::Gzip);
        let out_name = opts
            .out_name
            .clone()
            .unwrap_or_else(|| 兜底输出名.to_string());
        let out_path = opts.dest.join(&out_name);
        // 重借用出 `&mut dyn ReadSeek`：`Source` 里存的就是一个可变引用，
        // 不能 move 出来，重借用一次；泛型参数又要求 Sized，所以必须有这一层
        let reader: &mut dyn ReadSeek = &mut *src.reader;
        let n = crate::extract_stream::decompress_stream_ctx(
            fmt,
            reader,
            &out_path,
            &opts.limits,
            opts.packed_hint,
            ctx,
        )?;
        // 单流只有一个产物 —— 包装成统一报告，上层不必为它写特例
        let mut rep = ExtractReport {
            bytes_written: n,
            ..Default::default()
        };
        rep.files.push(crate::extract::ExtractedFile {
            name: out_name,
            out_path,
            size: n,
            encrypted: false,
        });
        Ok(rep)
    }

    fn list(
        &self,
        _src: &mut Source<'_>,
        opts: &UnpackOptions,
        _ctx: &UnpackCtx,
    ) -> Result<Preview> {
        // 单流的清单**不需要读数据**：只有一个产物，名字由编排层算好（它知道原始
        // 文件名）—— 这也正是 `Source` 不再带 `path` 之后，名字信息的正确来源。
        let fmt = opts.outer.unwrap_or(ArchiveFormat::Gzip);
        let name = opts
            .out_name
            .clone()
            .unwrap_or_else(|| 兜底输出名.to_string());
        crate::preview::preview_single_stream(fmt, opts.packed_hint, &name)
    }
}

/// tar 家族：里面是**目录树**（tar / tar.gz / tar.xz / tar.zst / …）
pub struct TarStreamUnpacker;

impl InArchive for TarStreamUnpacker {
    fn formats(&self) -> &'static [ArchiveFormat] {
        &[ArchiveFormat::Tar]
    }

    fn shapes(&self) -> &'static [Shape] {
        &[Shape::Tar]
    }

    fn caps(&self) -> Caps {
        Caps {
            encrypted: false,
            encrypted_names: false,
            split_volumes: false,
            selective: true,
            restores_mtime: true, // tar 头里有 mtime，v0.8 接的
            can_create: false,
        }
    }

    fn unpack(
        &self,
        src: &mut Source<'_>,
        opts: &UnpackOptions,
        ctx: &UnpackCtx,
    ) -> Result<ExtractReport> {
        let eopts = opts.to_stream();
        let outer = opts.outer;
        // 重借用出 `&mut dyn ReadSeek`：`Source` 里存的就是一个可变引用，
        // 不能 move 出来，重借用一次；泛型参数又要求 Sized，所以必须有这一层
        let reader: &mut dyn ReadSeek = &mut *src.reader;
        crate::extract_stream::extract_tar_ctx(reader, outer, &eopts, ctx)
    }

    fn list(
        &self,
        _src: &mut Source<'_>,
        _opts: &UnpackOptions,
        _ctx: &UnpackCtx,
    ) -> Result<Preview> {
        // tar 是**顺序格式**：列清单得把整条流扫一遍。现在如实报「信息不完整」
        // （`complete: false`，外壳会显示"无法预览"），而不是给一个空清单 ——
        // 空清单会被当成"包里什么都没有"，那是最糟的误导。
        // 将来要做的话：边扫边收集，扫到哪报到哪（`complete: false` 已经预留了）。
        Ok(Preview {
            format: "tar",
            complete: false,
            ..Default::default()
        })
    }
}

// ─────────────────────────── 注册表 ───────────────────────────
//
// 顺序即优先级（`xadmaster.md` ②-01：把「贪心的、弱的放最后」写成显式注释）。
// 我们的顺序按「判定确定性从高到低」排：魔数明确的在前，靠结构/扩展名判定的在后。
// 加格式就三步：① 写一个 `impl InArchive` ② 在下面加一行 ③ 补一条能力表测试。

/// 所有已实现的解压器（静态、有序、可被 `find` 查询）
pub static HANDLERS: &[&dyn InArchive] = &[
    // ① 魔数明确、命中率高
    &ZipUnpacker,
    &SevenZUnpacker,
    &RarUnpacker,
    // ② 单流压缩（一个产物）
    &SingleStreamUnpacker,
    // ③ tar 家族（要看 512 字节块结构，比魔数弱一档，所以排最后）
    &TarStreamUnpacker,
];

/// 按「格式 + 形状」找解压器
///
/// 找不到 = 这个格式的这一种形状我们还不支持（是能力边界，不是错误）。
pub fn find(fmt: ArchiveFormat, shape: Shape) -> Option<&'static dyn InArchive> {
    HANDLERS
        .iter()
        .copied()
        .find(|h| h.formats().contains(&fmt) && h.shapes().contains(&shape))
}

/// 按「格式 + 形状」找解压器，找不到就报错（附上格式名与形状，便于排查）
pub fn find_or_err(fmt: ArchiveFormat, shape: Shape) -> Result<&'static dyn InArchive> {
    find(fmt, shape).ok_or_else(|| {
        Error::new(ErrorCode::NotArchive)
            .with("format", fmt.as_str())
            .with("shape", format!("{shape:?}"))
            .context("这个格式暂时不支持这种解压方式")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 格式矩阵上的每一档都有实现认领() {
        // 防「新加格式忘了注册」：HANDOFF 的格式矩阵里每一档都要查得到
        let 应该有 = [
            (ArchiveFormat::Zip, Shape::Plain),
            (ArchiveFormat::Zip, Shape::Volumes),
            (ArchiveFormat::SevenZ, Shape::Volumes),
            (ArchiveFormat::Rar4, Shape::Plain),
            (ArchiveFormat::Rar5, Shape::Plain), // RAR 4/5 共用一份实现
            (ArchiveFormat::Tar, Shape::Tar),
            (ArchiveFormat::Gzip, Shape::Single),
            (ArchiveFormat::Xz, Shape::Single),
            (ArchiveFormat::Zstd, Shape::Single),
            (ArchiveFormat::Bzip2, Shape::Single),
            (ArchiveFormat::Lz4, Shape::Single),
            (ArchiveFormat::Brotli, Shape::Single),
        ];
        for (fmt, shape) in 应该有 {
            assert!(
                find(fmt, shape).is_some(),
                "格式 {fmt:?} / 形状 {shape:?} 没有 handler"
            );
        }
    }

    #[test]
    fn 不支持的组合明确返回空() {
        // tar 不该被当成单流（否则会把一个 tar 当成一个文件解出来）
        assert!(find(ArchiveFormat::Tar, Shape::Single).is_none());
        // 单流格式不该被当成普通归档
        assert!(find(ArchiveFormat::Brotli, Shape::Plain).is_none());
        // 未知格式没有任何 handler
        assert!(find(ArchiveFormat::Unknown, Shape::Plain).is_none());
        assert!(find_or_err(ArchiveFormat::Unknown, Shape::Plain).is_err());
    }

    #[test]
    fn 能力表与实际实现一致() {
        // 这几条是 CHANGELOG 里写死的"诚实结论"，能力表不能吹牛
        let 七z = find(ArchiveFormat::SevenZ, Shape::Volumes).unwrap();
        assert!(!七z.caps().restores_mtime, "7z 还原不了时间戳，别写 true");
        assert!(七z.caps().encrypted_names, "7z -mhe=on 会加密文件名");

        let rar = find(ArchiveFormat::Rar5, Shape::Plain).unwrap();
        assert!(!rar.caps().restores_mtime, "RAR 还原不了时间戳，别写 true");
        assert!(!rar.caps().can_create, "RAR 只解不压");

        let zip = find(ArchiveFormat::Zip, Shape::Plain).unwrap();
        assert!(zip.caps().restores_mtime, "ZIP 已接时间戳还原（v0.8）");
        assert!(zip.caps().can_create, "ZIP 是反向打包主力");

        // 单流没有"挑条目"这回事
        let gz = find(ArchiveFormat::Gzip, Shape::Single).unwrap();
        assert!(!gz.caps().selective);
    }

    #[test]
    fn 同一个格式加形状只能有一个实现认领() {
        // 重复注册会让"按顺序取第一个"变成隐式行为，这里直接拦住
        for h in HANDLERS {
            for fmt in h.formats() {
                for shape in h.shapes() {
                    let 认领数 = HANDLERS
                        .iter()
                        .filter(|x| x.formats().contains(fmt) && x.shapes().contains(shape))
                        .count();
                    assert_eq!(
                        认领数, 1,
                        "格式 {fmt:?} / 形状 {shape:?} 被 {认领数} 个实现认领了"
                    );
                }
            }
        }
    }

    #[test]
    fn 统一参数能转成四份既有结构体且字段不丢() {
        let mut o = UnpackOptions::new("/tmp/出");
        o.password = Some("解压码".into());
        o.selection = Selection::only(vec![1, 3]);
        o.out_name = Some("结果.bin".into());
        o.packed_hint = 4096;

        let z = o.to_zip();
        assert_eq!(z.dest, PathBuf::from("/tmp/出"));
        assert_eq!(z.password.as_deref(), Some("解压码"));
        assert!(!z.selection.is_all());

        let s = o.to_stream();
        assert_eq!(s.out_name.as_deref(), Some("结果.bin"));
        assert_eq!(s.packed_hint, 4096);

        // 7z / RAR 两份也不能把密码和选择漏掉
        assert_eq!(o.to_7z().password.as_deref(), Some("解压码"));
        assert_eq!(o.to_rar().password.as_deref(), Some("解压码"));
        assert!(!o.to_rar().selection.is_all());
    }

    #[test]
    fn 源的默认值与回退规则() {
        // 注意用 Cursor：`&[u8]` 只有 Read 没有 Seek，而接口收的是「可定位流」
        let 数据 = [1u8, 2, 3, 4];
        let mut 流 = std::io::Cursor::new(&数据[..]);
        let s = Source::new(&mut 流);
        assert_eq!(s.base, 0);
        assert_eq!(s.len, None);
        assert_eq!(s.packed_total, 0);
        // 长度未知时：没有压缩大小就退回给定兜底值
        assert_eq!(s.len_or(999), 999);

        let s = Source::new(&mut 流).with_packed_total(1234);
        assert_eq!(s.len_or(999), 1234, "有压缩大小时优先用它");

        let s = Source::new(&mut 流).with_offset(100, 50);
        assert_eq!(s.len_or(999), 50, "显式给的长度优先级最高");
    }

    #[test]
    fn 兜底输出名与既有代码一致() {
        // 这个常量的值必须与 job.rs / preview.rs 里的硬编码一致，
        // 否则统一接口接上去会改变文件名（回归会炸）
        assert_eq!(兜底输出名, "解压结果");
    }

    #[test]
    fn zip_的列清单走统一接口() {
        // 造一个内存 ZIP（和 extract.rs 的测试同样的手法）
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        w.start_file("a.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, b"hello").unwrap();
        w.start_file("中文名.bin", opt).unwrap();
        std::io::Write::write_all(&mut w, b"xy").unwrap();
        let data = w.finish().unwrap().into_inner();

        let mut cur = std::io::Cursor::new(&data[..]);
        let mut src = Source::new(&mut cur);
        let handler = find(ArchiveFormat::Zip, Shape::Plain).unwrap();
        let pv = handler
            .list(&mut src, &UnpackOptions::new("."), &UnpackCtx::quiet(0))
            .expect("ZIP 应当能列清单");

        assert_eq!(pv.entries.len(), 2);
        assert_eq!(pv.entries[0].path, "a.txt");
        assert_eq!(pv.entries[1].path, "中文名.bin", "中文名不能变成乱码");
        assert_eq!(pv.total_size, 7);
        assert_eq!(pv.encrypted_count, 0);
        assert!(pv.complete);
        // 序号必须 1-based（界面显示的号 = `--select` 用的号）
        assert_eq!(pv.entries[0].index, 1);
    }

    /// 只声明格式、**故意不实现 `list`** 的假解压器 —— 专门用来测 trait 的默认实现
    struct 假解压器;

    impl InArchive for 假解压器 {
        fn formats(&self) -> &'static [ArchiveFormat] {
            &[ArchiveFormat::Unknown]
        }
        fn shapes(&self) -> &'static [Shape] {
            &[Shape::Plain]
        }
        fn caps(&self) -> Caps {
            Caps::default()
        }
        fn unpack(
            &self,
            _s: &mut Source<'_>,
            _o: &UnpackOptions,
            _c: &UnpackCtx,
        ) -> Result<ExtractReport> {
            unreachable!("这个假解压器只用来测 list 的默认实现")
        }
    }

    #[test]
    fn 没实现列清单的格式要明确报错而不是给空清单() {
        // 将来加格式时，如果只写了 unpack 没写 list，默认实现必须**明确报错**，
        // 绝不能返回空清单 —— 那会被上层和用户理解成"包里什么都没有"。
        let mut cur = std::io::Cursor::new(&b"x"[..]);
        let mut src = Source::new(&mut cur);
        let err = 假解压器
            .list(&mut src, &UnpackOptions::new("."), &UnpackCtx::quiet(0))
            .expect_err("没实现 list 的格式必须报错");
        assert_eq!(err.param("reason"), Some("list_not_implemented"));
        assert!(err.param("format").is_some(), "报错要说清是哪个格式");
    }
}
