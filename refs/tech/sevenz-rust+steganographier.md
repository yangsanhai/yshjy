# sevenz-rust(2) + SteganographierGUI 技术拆解（施工清单）

> 对象：`refs/sevenz-rust/`（**实为 sevenz-rust2 0.24.0**，Rust 8088 行 / 27 文件，Apache-2.0）
> 　　　`refs/SteganographierGUI/Steganographier.py`（Python 3847 行，CRLF，VERSION 1.3.11，git HEAD `c2c6eaa`）——**生成方**
> 验证方式：逐行读源码 + 复核既有分析（`refs/analysis/sevenz-rust.md` / `steganographier-hide.md` / `steganographier-reveal.md`），行号均已对源码核对。
> `决定` 图例：✅已采用（已进我方代码）｜🔄待采用（确认有价值，排队）｜⚠️已评估-不采用（附原因）｜❓未判定
> 我方现状锚点：`src/scan.rs`（签名扫描/7z 头校验）、`src/layout/zip.rs`（EOCD 反推）、`src/io/{window,segment,interrupt}.rs`、`src/rescue.rs`、`src/safety.rs`、`src/hardware.rs`、`src/pack/cover.rs`。当前依赖仅 `flate2` + `thiserror`。

> **两条硬结论先记住**：
> 1. **sevenz-rust 不负责定位/分卷/偏移**——`Archive::read` 硬编码 `seek(Start(0))`（`src/reader.rs:198-199`），分卷、非 0 偏移、损坏头恢复、内存上限全要我方自己做；它只当「给我一条从 0 开始的 7z 字节流 + 密码，我吐文件」的内核（🔁 建议按 crate 引入）。
> 2. **Steganographier 是生成方**，我们要支持它**全部**伪装布局：`mp4`（尾部追加）、`mp4(zarchiver)`（ftyp 后插 free + 改 stco/co64 + 噪声）、`mkv`（附件）。它的 **MP4 原子操作/偏移修正/反指纹/密码** 侧是重点（④ 节拆得最细）。

---

## ① 读取（定位、缓冲、分块、并行、内存、偏移、分卷、可中断）

| # | 技术点 | 出处(文件:行) | 做法 | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| R-1 | 非 0 偏移读取：库硬编码 `seek(Start(0))`，必须外包一层"偏移窗口" | sevenz-rust `src/reader.rs:197-199` | 自己实现 `Read+Seek` 包装器，把 `[base, …)` 重新标定为 0 起点再喂给库。 | 内嵌 7z / 伪装壳可直接读，O(1) 内存 | 写错 base（±1）→ 报 `BadSignature`，需靠签名+RCI 复核 | ✅已采用（`src/io/window.rs`） |
| R-2 | 分卷=裸字节切分：两项目都没有真正的分卷实现，须自行拼成单流 | 隐写者 `Steganographier.py:2058-2340`（`hide_file` 无任何分卷分支）｜sevenz-rust 全库无 volume（只认单流 `reader.rs:197`） | `.7z.001/.002…` 按卷号顺序虚接成一条 `Read+Seek` 流；7z 卷无卷头，直接拼即合法。 | 分卷支持≈实现一个多文件拼流，无需解析卷结构 | 缺卷要提前用长度自洽判定，别等库报偏移越界 | ✅已采用（`src/io/segment.rs`） |
| R-3 | 8 MiB 分块流式读（绝不整包入内存） | 隐写者 `Steganographier.py:1699-1704`（`read_in_chunks`，8MB） | 大文件一律分块读+分块写，块间不保留整份数据。 | 常量内存，8GB+ 包也不炸 | 块边界要处理跨块签名（留重叠） | ✅已采用（`src/scan.rs` 分块扫描，`ScanOptions.chunk_size`） |
| R-4 | `BoundedReader`：把子读取限制在声明的 `pack_size` 内 | sevenz-rust `src/reader.rs:25-49` | 解码器上游包一层带剩余量的读取器，读满即停。 | 防解码器越界读到相邻数据 | 仅对声明 size 的块有效 | 🔄待采用 |
| R-5 | `SharedBoundedReader`：多解码器共享一个源、每次读前重定位 | sevenz-rust `src/reader.rs:54-101` | `Rc<RefCell<&mut R>>`+区间游标，read 前 `seek(cur)`。 | 多路 coder（BCJ2）共享源；顺序读免重复 seek | `RefCell` 运行时借用、非线程安全 | 🔄待采用 |
| R-6 | 线程数默认 `available_parallelism()`、可显式设并 clamp 1..=256 | sevenz-rust `src/reader.rs:1289-1291, 1328-1330` | 默认吃满核，暴露 setter 让上层按机器档位改。 | 多核直接受益 | 手机/小机默认吃满核会卡，必须显式设 | ✅已采用（`src/hardware.rs` 探测核心数+档位） |
| R-7 | NextHeader 一次性整块读入内存（库做法；大包需自设上限） | sevenz-rust `src/reader.rs:372-373` | 按声明的 `next_header_size` `vec![0; n]` 后 `read_exact`。 | 头小、逻辑简单 | 声明值极大→OOM（库已加 `:350-359` 边界校验兜底）；我方应按上限流式 | 🔄待采用（配合 J-19 上限） |
| R-8 | 进度回调=已处理字节/总字节（上界估算 `8+256`） | 隐写者 `Steganographier.py:2105, 2119-2120` | 复制过程按字节累加回调，总量取上界防止进度回退。 | UI 进度条平滑 | 估算偏差会小幅跳变 | 🔄待采用 |
| R-9 | 写盘用大缓冲（32 MB 分块 + 8 MB 进度阈值） | 隐写者 `Steganographier.py:2839, 2903` | 解压写文件用 32MB 块、进度每 8MB 刷一次。 | 少 syscall、进度不过密 | 32MB 缓冲对小文件浪费 | 🔄待采用 |
| R-10 | 临时/中间文件放输入目录**之外**，用后即删（finally 容错清理） | 隐写者 `Steganographier.py:521-536, 2332-2337` | 临时 zip 名 `<basename>_hidden_<N>.zip` 放父目录；删除失败只记日志不掩盖真错。 | 避免自包含膨胀 / 目录被污染 | 跨卷/权限受限时可能创建失败 | 🔄待采用 |
| R-11 | 可中断读取（长扫描/长解压要能取消） | 隐写者 `Steganographier.py:455-465`（`cancel_processing`）｜sevenz-rust 无 | I/O 抽象里注入取消标志，循环内检查。 | 大文件可随时停 | 加一处检查点开销 | ✅已采用（`src/io/interrupt.rs`） |

---

## ② 判断（签名、结构校验、假签名、一致性、嗅探、编码、CRC）

| # | 技术点 | 出处(文件:行) | 做法 | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| J-1 | 7z `StartHeaderCRC` 强校验：`CRC32(bytes[12..32])==u32@8` | sevenz-rust `src/reader.rs:242-244` | 命中签名后读 32 字节头，校验 bytes[12..32] 的 CRC32 等于 bytes[8..12]。 | 单道 CRC 即 2⁻³² 假阳性，扫描可放心 | 头部被改（bytes8..32）即失败→需恢复路径 | ✅已采用（`src/scan.rs:428-435` `verify_7z_header`） |
| J-2 | 7z 无「总长度」字段，但可精确推导 `总长=32+NextHeaderOffset+NextHeaderSize` | sevenz-rust `src/reader.rs:366-369` | 由头内两个 REAL_UINT64 算出 blob 精确 `[start,end)`。 | 免猜长度，只读真身段 | 需 `checked_add` 防溢出 | ✅已采用（`src/scan.rs:436-441`） |
| J-3 | `next_header_size > reader_len` → 截断/缺卷信号 | sevenz-rust `src/reader.rs:350-359` | 拼流长度不够时提前报「缺卷」，而非等库报 `next header offset out of range`。 | 免费完整性信号、报错更清楚 | 依赖头字段可靠 | ✅已采用（`src/scan.rs` 长度自洽判定） |
| J-4 | 版本 `major` 必须为 0（否则 `UnsupportedVersion`） | sevenz-rust `src/reader.rs:210-215` | 命中后顺带校验 `hdr[6]==0`。 | 再降误报 | 几乎不增加成本 | 🔄待采用（`verify_7z_header` 目前未查版本字节） |
| J-5 | 第二道 `NextHeaderCRC` 校验（读 NextHeader 后算 CRC32） | sevenz-rust `src/reader.rs:374-376` | 拿到 NextHeader 内容后校 CRC==u32@28。 | 双 CRC 叠加≈2⁻⁶⁴ | 要真读一次头（I/O 成本） | 🔄待采用 |
| J-6 | 头有效性探针：`bytes[12..32]` 全 0 才判「头无效」 | sevenz-rust `src/reader.rs:219-227` | 用「20 字节全 0」当损坏判据触发恢复分支。 | 实现简单 | 判据脆弱：只清部分字节不进这条路，易误判 | ⚠️已评估-不采用（判据不可靠，改走多候选扫描） |
| J-7 | 损坏头恢复 `try_to_locale_end_header`：末尾 1MiB 反向找 `0x17/0x01` | sevenz-rust `src/reader.rs:299-341` | 从尾部反向逐字节找头 NID，反推 offset/size 试解析。 | 思路正确（StartHeader 坏仍可救） | **库内实现是坏的**：`L330-331` 用 `?` 而非 `continue`，第一个假候选即中止；实测 12/12 全废 | ⚠️已评估-不采用（库版不可用）→ 见 F-1 自行实现 |
| J-8 | 假签名识别：绝不「见签名即认定」 | 隐写者 `Steganographier.py:2301-2323`（故意撒 7 种假特征码 + 随机字节） | 每个签名命中都要过结构校验（头 CRC / EOCD 可解析）才算数。 | 直接抵抗作者的「哈希随机化」干扰设计 | 校验成本略增 | ✅已采用（`src/scan.rs` 置信度 Low/Medium/High + 结构校验） |
| J-9 | 短签名（3 字节）误报极高 → 加合理性校验 | 隐写者假签名池含 `GZIP`/`BZIP2` 短码 `Steganographier.py:2307-2308` | gzip 第 4 字节保留位须 0、bzip2 第 4 字节须 `1..9`。 | 实测把上千次误报压下去 | 只覆盖已知格式 | ✅已采用（`src/scan.rs:44-53` `short_sig_plausible`） |
| J-10 | MP4 `free` 原子 payload 正判定应是「内含可解析 EOCD」而非「首字节=PK」 | 隐写者 `Steganographier.py:3074`（只认 `startswith(b'PK\x03\x04')`，脆弱） | 在 payload 内部搜 EOCD/`PK\x03\x04`，能解析出中央目录即认定。 | 不漏「64–256B 随机 pad + ZIP」的普通模式 | 要多做一次 EOCD 扫描 | ⚠️已评估-不采用（不照抄其脆弱判据）→ 改写为 J-11 |
| J-11 | 把整份伪装文件当 ZIP 打开：尾部 EOCD → 中心目录 → 反推 base | 隐写者 `Steganographier.py:2658, 2833`（`_try_mp4_direct_extraction`/`_extract_with_zipfile`） | `base = eocd_abs - cd_size - cd_offset`，所有 LFH 偏移整体平移 base。 | 覆盖 mp4 普通模式 + 「外壳本就是 zip」 | 必须自己算，`zip` crate 不会自动修正 | ✅已采用（`src/layout/zip.rs` 偏移反推） |
| J-12 | EOCD 必须落在「末尾 64KB+22」窗口内（否则认不出） | 隐写者 `Steganographier.py:2833`（依赖 CPython `_EndRecData`） | 从尾部倒着找 `PK\x05\x06`，支持带注释（慢路径）。 | 兼容必然带 comment 的包 | 窗口外（free 内嵌）必须另走路径 | ✅已采用（`src/layout/zip.rs` 尾部窗口，默认 1 MiB） |
| J-13 | ZIP64：`PK\x06\x06` EOCD + `PK\x06\x07` 定位器 + `0xFFFFFFFF` 哨兵 | 隐写者 `Steganographier.py:2833`（依赖 `_EndRecData64`） | 识别定位器、读 ZIP64 EOCD 的 cd_size/offset。 | 大包/大目录不漏 | 兼容性细节多 | ✅已采用（`src/layout/zip.rs:26-27,154-214`） |
| J-14 | `free` 原子选取门槛（`size>1024` 太宽 / `size<=8 continue` 错位） | 隐写者 `Steganographier.py:3374`（>1024）、`3065-3067`（≤8 `continue` 未重定位） | 只按门槛取第一个 `free` 会误选播放器对齐用的填充原子。 | —— | 选错即失败且不回头；`continue` 会错位解析 | ⚠️已评估-不采用（两个门槛都有坑，改为全量候选+结构判定） |
| J-15 | 头加密 vs 内容加密要分开判：内容加密头明文可先列表，头加密直接 `PasswordRequired` | sevenz-rust `src/reader.rs:381-423`（`kEncodedHeader` 解出含 AES 才需密码） | 先尝试列表成功 → 展示文件树再问密码；头加密 → 先要密码。 | UI 体验好、少一次白等 | 需区分两种形态 | 🔄待采用 |
| J-16 | 「密码错」判定不能只看错误类型（三种都算错） | sevenz-rust `src/error.rs:46-53`（`PasswordRequired`/`MaybeBadPassword`/`ChecksumVerificationFailed`） | 把三类都当「候选密码错」处理，继续下一个候选。 | 避免把错密码当「文件损坏」 | 会多试几个候选 | 🔄待采用 |
| J-17 | 内存/字典上限判定：LZMA2 字典 → `MaxMemLimited` | sevenz-rust `src/decoder.rs:106-114, 255-271` | 扫描阶段先解析 coder props 的字典大小（`(2\|(b&1))<<(b/2+11)`），超大直接拒。 | 拦恶意 LZMA2 字典 OOM | 只覆盖 LZMA2 | 🔄待采用 |
| J-18 | header 元素 count/size 上限 `bounded_count`（防「小文件声明巨量」） | sevenz-rust `src/reader.rs:292, 521, 836, 872, 1035` | 任何头内计数值不得超过头缓冲字节数。 | 免费的 OOM 防护，不误杀正常包 | 无 | 🔄待采用 |
| J-19 | 预分配上限 `MAX_PREALLOC_BYTES=4 MiB`（按声明 size 预分配有上限） | sevenz-rust `src/reader.rs:20-23, 1706-1707, 1740` | 预分配按 `min(size,4MiB)`，真实大小随读增长。 | 防小包逼出大分配 | 大文件会多次 realloc | 🔄待采用 |
| J-20 | 噪声块形态特征：`00 00 00 08 'mdat'` 可作指纹 | 隐写者 `Steganographier.py:2422-2425, 2323` | 每段噪声后跟 8 字节空 mdat，出现在文件中段/末尾。 | 识别 zarchiver 脏链的辅助特征 | 合法 MP4 也可能有空 mdat | 🔄待采用 |
| J-21 | 原子链非法 size（`<8`/越界/随机 ASCII）→ 弃 atom 走签名扫描 | 隐写者 `Steganographier.py:2282-2285, 3134-3136` | 遇荒谬 size 不再按 atom 前进，切换「签名扫描 + 结构自校验」。 | 脏 box 链也能兜住 | 需有兜底路径 | ✅已采用（`src/scan.rs` 全文件签名扫描兜底） |
| J-22 | 加密 ZIP 判定：ZipCrypto vs AES（extra `0x9901`、AE-2、strength 3） | 隐写者 `Steganographier.py:1962-1965, 2925` | 读中心目录 extra 判断是否 AES，决定用哪套解压器。 | 加密包直接选对引擎 | 非 AES 也要能解 | 🔄待采用 |

---

## ③ 解压（解码器、密码、分卷拼接、流式、嵌套、目录重建、选择性提取）

| # | 技术点 | 出处(文件:行) | 做法 | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| D-1 | 7z AES-256-CBC + SHA-256 KDF 解码 | sevenz-rust `src/encryption/aes.rs:131-182` | 解析 props（salt/iv/numCyclesPower）→ KDF → CBC 解密。 | 与 7-Zip 标准一致 | 需引入 crate 或自实现 | 🔄待采用（建议按 crate 引入 `sevenz-rust2`） |
| D-2 | 密码 = 字符→**UTF-16LE**（`Password::from(&str)`） | sevenz-rust `src/encryption/password.rs:53-61` | 密码字符串按 UTF-16LE 编字节再喂 KDF。 | 与 7-Zip 存储约定一致 | **没有 `From<String>`**，须 `s.as_str()` | 🔄待采用 |
| D-3 | raw-key 模式（`num_cycles_power==0x3F`）：key=salt‖password 截断 | sevenz-rust `src/encryption/aes.rs:161-169` | 特殊 power 值时跳过 KDF，直接拼 key。 | 兼容非标准包 | 少见，易踩 | 🔄待采用 |
| D-4 | KDF 工作因子上限 `MAX_AES_CYCLES_POWER=24` | sevenz-rust `src/encryption/aes.rs:184-188` | 超过即拒，防 2^n 轮 SHA-256 的 CPU DoS。 | 防空转 | 正常包 power=19 不受影响 | 🔄待采用 |
| D-5 | KDF 结果缓存（按 `sha256(power‖saltlen‖salt‖pw)` 指纹单槽） | sevenz-rust `src/encryption/aes.rs:207-230` | 同 salt+power 解多卷/多内层包时复用密钥。 | 多包提速明显 | 单槽缓存换密码即失效；建议我方自建 HashMap | 🔄待采用（自建多槽） |
| D-6 | coder method-id → 解码器映射（COPY/LZMA/LZMA2/PPMd/BZIP2/DEFLATE/ZSTD/BROTLI/LZ4/BCJ/DELTA/AES） | sevenz-rust `src/decoder.rs:92-208`；method-id 表 `src/archive.rs:311-357` | 建方法表 + 按 feature 挂解码器。 | 全家桶一次到位 | 部分需开 feature | 🔄待采用 |
| D-7 | **BCJ2 四路输入**解码（多输入 coder 图） | sevenz-rust `src/reader.rs:1423-1515`（`build_decode_stack2`） | 主/分流 + 两个旁路，按 2 位选择器组装。 | 支持官方 BCJ2 包（实测通过） | 仅解码有，编码无 | 🔄待采用 |
| D-8 | 分卷拼接后直接解（无卷头，等价单流） | 隐写者无分卷 `Steganographier.py:2058` ｜ sevenz-rust 单流 `src/reader.rs:197` | 虚接段 → 交给解压内核。 | 分卷≈拼流 | 缺卷须先判 | ✅已采用（`src/io/segment.rs`） |
| D-9 | 逐 block 解码；非 solid 可多线程并行 | sevenz-rust `src/reader.rs:1794-1897`（`BlockDecoder`） | 每个 block 一个解码器，非 solid 一文件一 block 可并发。 | 非 solid 并行快 | solid 必须从头解 | 🔄待采用 |
| D-10 | 选择性提取单文件（`read_file`，solid 要先解前面所有） | sevenz-rust `src/reader.rs:1730-1745` | 先列表→定位目标 block→只解需要的块。 | 大包只取一个文件很省 | solid 包便宜不了 | 🔄待采用 |
| D-11 | CRC32 校验（块级/子流级/文件级），失败报 `ChecksumVerificationFailed` | sevenz-rust `src/reader.rs:113-149, 1880-1882` | 边读边算 CRC，读满即比对。 | 免费完整性校验 | 失败会中断该流 | 🔄待采用 |
| D-12 | 流式写盘 `io::copy`（不 `read_to_end` 到内存） | sevenz-rust `src/util/decompress.rs:219-234` | 大文件一律流式 copy 到 BufWriter。 | 常量内存 | 需处理时间戳设置 | ✅已采用（我方 I/O 层流式） |
| D-13 | 成员名清洗 `sanitize_path`（控制字符/非法字符/保留名/255 截断） | 隐写者 `Steganographier.py:539-584` | 去 Unicode 类别 C、替换 `<>:"\|?*`、处理 CON/PRN/COM1、截断 255。 | 跨平台落地安全 | 见 D-26 的穿越隐患 | ✅已采用（`src/safety.rs`） |
| D-14 | Zip-Slip 防护 `safe_join` | sevenz-rust `src/util/decompress.rs:153-172` | 拒绝 `..`/根/盘符分量，`\` 当分隔符。 | 防 CWE-22 | 无 | ✅已采用（`src/safety.rs`） |
| D-15 | 密码预验证（解压前读第 1 个非目录成员 1 KB） | 隐写者 `Steganographier.py:2851-2861, 2943-2953` | 通过预读判密码，失败即换下一候选、不落半成品。 | 快、不留垃圾 | 仅覆盖第一个成员 | 🔄待采用 |
| D-16 | 密码候选序列：用户密码 → PW.txt → 空密码兜底 | 隐写者 `Steganographier.py:2514-2519` | 组有序去重列表，空密码永远兜底。 | 覆盖绝大多数场景 | 用户密码若也在本里会试两次 | 🔄待采用 |
| D-17 | 密码本格式：`utf-8-sig` + TAB 取首列 | 隐写者 `Steganographier.py:2450-2456` | 一行一密码，`split('\t')[0]`（兼容「解TMD压」格式）。 | 兼容既有密码本 | —— | 🔄待采用 |
| D-18 | ⚠️ 密码本 `encode('ascii','ignore')` 会**静默吞掉中文密码** | 隐写者 `Steganographier.py:2458` | 写端不削、读端削→中文密码走密码本必失配。 | —— | 中文密码无效，隐蔽偏差 | ⚠️已评估-不采用（我们保留原样、不做 ASCII 过滤） |
| D-19 | 密码统一 `encode('utf-8')` | 隐写者 `Steganographier.py:1965, 2669` | 与本工具写入端一致。 | 自产包必中 | 与中文 Windows 的 WinRAR/7-Zip 包（GBK）不一致 | 🔄待采用（补 GBK/CP936 变体） |
| D-20 | 双引擎重试：先 `zipfile`（快，ZipCrypto）再 `pyzipper`（AES） | 隐写者 `Steganographier.py:2673-2698` | 标准包走标准库、AES 包走 pyzipper，逐密码各试一遍。 | 覆盖两种加密 | 多一倍尝试 | 🔄待采用（我方：`zip` crate 无原生 AES，需 `aes`/`winzip-aes` 或复用内核） |
| D-21 | MKV 附件提取：`mkvinfo` 找附件名 → `mkvextract` 抽 #1 → ZIP 再解一层 | 隐写者 `Steganographier.py:3392-3462` | 附件索引硬编码 `1:`，抽出的 `.zip` 再解压。 | 支持 mkv 模式 | 依赖外部 mkvtoolnix；索引/名解析脆弱 | 🔄待采用（但**不调外部程序**：自解析 EBML Attachments） |
| D-22 | 7-Zip 两步解压（外层无密码、内层带密码，`-t#`） | 隐写者 `Steganographier.py:2707-2761` | 子进程 7z.exe，先当压缩包打开再解内层。 | 省事、覆盖广 | **依赖外部程序，违背一体化原则**；且「装不装 7z」会改变解一层还是两层 | ⚠️已评估-不采用（我们一体化，绝不调外部程序） |
| D-23 | 目录项按结尾 `/` 判定 | 隐写者 `Steganographier.py:1794, 2874` | 名以 `/` 结尾即建目录。 | 简单 | 需防「假目录」 | 🔄待采用 |
| D-24 | 同名自动改名 `name (1).ext` | 隐写者 `Steganographier.py:1733-1749` | 冲突时递增编号，批次内用集合占位防撞号。 | 不覆盖既有文件 | 需处理并发 | 🔄待采用 |
| D-25 | ⚠️ `sanitize_path` 的 `strip('. ')` 会把 `..` 清成空→产生绝对路径 `//x`（Zip Slip 隐患） | 隐写者 `Steganographier.py:568-574` | 分段 `strip('. ')` 后 join，`..` 段被清空。 | —— | 有写到目标目录之外的隐患 | ⚠️已评估-不采用（我方显式拒绝绝对路径/`..`） |
| D-26 | 时间戳恢复（c/m/a） | sevenz-rust `src/util/decompress.rs:226-233` | 写盘后 `set_times` 恢复访问/修改/创建时间。 | 保留元数据 | 平台差异 | 🔄待采用 |

---

## ④ 压缩 / 伪装生成（**生成方重点**：MP4 原子、偏移修正、分卷、密码、反指纹）

> Steganographier 是我们必须支持的「生成方」，它的写侧布局全部要能读；写侧能力我们也按需复刻（尤其反指纹与哈希随机化）。

| # | 技术点 | 出处(文件:行) | 做法 | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| C-1 | 普通模式 `mp4`：外壳逐字节复制 + `[free头8B][随机pad 64–256B][ZIP]` 追加 | 隐写者 `Steganographier.py:2093-2154` | 8MB 分块复制外壳→写 free 头→写随机 pad→写 ZIP 原文；文件以 ZIP 的 EOCD 结束。 | 外壳 100% 不改→必定可播；改后缀即解压 | 依赖读端「尾部 EOCD 偏移修正」 | 🔄待采用（**读取侧必须支持**） |
| C-2 | `free` 原子头：`size=8+payload`；`total>0xFFFFFFFF` 时 `size=1`+u64 large-size | 隐写者 `Steganographier.py:2427-2438` | 标准 8B 头 vs 16B large-size 头，只在一处算清。 | 规范正确、>4GB 也能装 | 读端两种都要认 | 🔄待采用 |
| C-3 | 随机 pad 的两重目的：原子内不以 `PK` 开头 + 产物哈希随机化 | 隐写者 `Steganographier.py:2137-2142` | `os.urandom(randint(64,256))` 垫在 ZIP 前。 | 反指纹、每次哈希不同 | 读端不能只看首字节（见 J-10） | 🔄待采用 |
| C-4 | ZArchiver 模式：`free` 插在 `ftyp` 之后，并同步改写 `stco/co64` | 隐写者 `Steganographier.py:2201-2299` | 写 ftyp→写 free(ZIP)→逐个顶层原子改写偏移后写出。 | 数据在文件头部，ZArchiver 可从头扫 | 偏移修正不全（见 C-7/C-9）→产物可能不可播 | 🔄待采用（**读取侧必须支持**） |
| C-5 | `stco`(32 位) / `co64`(64 位) 表项整体平移 `+= delta` | 隐写者 `Steganographier.py:3200-3244` | 读 `entry_count`，逐条 `>I`/`>Q` 加偏移，长度不足则原样返回。 | chunk 偏移表正确 | 越界/长度不符要防 | 🔄待采用 |
| C-6 | 容器原子递归：`moov/trak/mdia/minf/stbl/udta` | 隐写者 `Steganographier.py:3254-3294` | 容器内从 pos=8 遍历子 box（含 size==0/1）递归调用 C-5。 | 覆盖标准 MP4 树 | 不认未知容器 | 🔄待采用 |
| C-7 | ⚠️ 噪声块被**误缩进进 `while` 循环体**：每个顶层原子后都插一份 | 隐写者 `Steganographier.py:2301-2323`（缩进 32 空格=循环体层级） | 本应循环后写一次，实际逐原子写。 | —— | `mdat` 被推到 stco 记录不到的位置；实测非 faststart→`moov atom not found`、faststart→`header damaged`；**产物不可播** | ⚠️已评估-不采用（照抄现状）→ 按注释意图改到文件末尾（见 C-8） |
| C-8 | 噪声块构成：假特征码×2 + 5–10KB urandom×2 + 空 `mdat` | 隐写者 `Steganographier.py:2302-2323` | 每份噪声 ≈10–20KB + 8B，池含 RAR4/RAR5/7Z/ZIP/GZIP/BZIP2/XZ。 | 哈希随机化/反去重（作者本意） | 会撑大文件（本例外壳放大 3 倍） | 🔄待采用（若做哈希随机化：写到**文件末尾**、并把噪声长度并入 stco 补偿） |
| C-9 | 偏移修正覆盖缺口：不处理 `tfhd.base_data_offset`/`tfdt`(fMP4)、`sidx`、`saio/saiz`/`mvex` | 隐写者 `Steganographier.py:3198-3257` | 只认 `stco/co64`。 | 简单 | **分片 MP4/CMAF 会静默生成坏文件** | ⚠️已评估-不采用（照抄）→ 对 fMP4 直接拒绝或补全 |
| C-10 | MKV 模式：`mkvmerge` 挂 2 个附件（ZIP + 8KB 垃圾） | 隐写者 `Steganographier.py:2158-2198` | 子进程 mkvmerge，附件 2 是垃圾以降低可疑度。 | ZIP 以附件形式藏 | **依赖外部 mkvmerge，违背一体化** | ⚠️已评估-不采用（我方自写 EBML 包装） |
| C-11 | 条目顺序 `random.shuffle` | 隐写者 `Steganographier.py:1978, 2007` | 打乱写入顺序。 | 反指纹/反去重 | 无 | 🔄待采用 |
| C-12 | 每条目随机 mtime（1980–2099 合法时间元组） | 隐写者 `Steganographier.py:2019-2036` | 生成随机 `date_time` 塞进 `ZipInfo`。 | 反指纹 | 非法日期要重摇 | 🔄待采用 |
| C-13 | 随机压缩级别 1–9（无密码时） | 隐写者 `Steganographier.py:2015` | `randint(1,9)` 传给逐条 write。 | 反指纹 | 无 | 🔄待采用 |
| C-14 | EOCD 注释固定写 SHA-256 + 时间戳 + timehash | 隐写者 `Steganographier.py:1938, 1993, 2051` | 目录内容哈希 + 可读时间 + 时间哈希拼进 `zf.comment`。 | 自带校验/溯源 | 使读端必走 EOCD 慢路径（带注释） | 🔄待采用 |
| C-15 | 加密 = AES-256 / AE-2 / 强制 `STORED`（不压缩）+ `flag_bits\|=0x1` | 隐写者 `Steganographier.py:1962-1985`（`pyzipper.WZ_AES` + `ZIP_STORED`） | pyzipper AESZipFile，压缩方式锁死 STORED（注释里 DEFLATED 被注释掉）。 | 与 Windows 端互通 | 加密产物体积≈明文；`zip` crate 无原生 AES | 🔄待采用 |
| C-16 | 中文名自动置 UTF-8 标志位 `0x800`（原始名照写，不 sanitize） | 隐写者 `Steganographier.py:2036`（`ZipInfo`） | 写侧不 sanitize，靠 `zipfile` 自动加标志位。 | 中文文件名可还原 | 读端需按标志位解名 | 🔄待采用 |
| C-17 | 打包排除输出/临时 zip 自身 + `normpath` 防自包含膨胀 | 隐写者 `Steganographier.py:1943-1960, 1895, 521-536` | `collect_zip_entries` 跳过 archive 自身；输入先 `normpath`。 | 防「一边读一边写自己」爆炸 | 尾部分隔符输入必须归一化 | 🔄待采用（我方打包侧硬约束） |
| C-18 | 目录输入保留顶层目录名（arcname = `<dir>/<rel>`） | 隐写者 `Steganographier.py:1948-1956` | 单文件取 basename，目录保留顶层结构。 | 解出后结构正确 | 无 | 🔄待采用 |
| C-19 | 输出命名规则：`_hidden_<N>` / `_hidden_z<N>` / `_hidden_<N>.mkv` | 隐写者 `Steganographier.py:2342-2420` | 三种 output_option × 三种模式拼名 + 冲突改名。 | 与源工具一致、可预测 | 规则细节多 | 🔄待采用 |
| C-20 | 内置/默认封面素材（`cover_video` 目录，4 种选择策略） | 隐写者 `Steganographier.py:1843-1889, 1837-1841` | 随机/时长/名称/下拉 4 策略挑外壳 mp4。 | 用户没素材也能开工 | 素材要合法 MP4 | ✅已采用（`src/pack/cover.rs` 内置极小素材，编译进二进制） |
| C-21 | 大小-时长合理性提醒（阈值表，仅提醒不阻断） | 隐写者 `Steganographier.py:670-687, 1178-1205` | ≤200MB/1min … >4GB/120min 逐档对比，低了就提示。 | 帮用户判断伪装是否可信 | 误导式阈值、非硬校验 | 🔄待采用 |
| C-22 | 假特征码池（8 种签名）用于哈希随机化 | 隐写者 `Steganographier.py:2302-2310` | 随机撒入伪造 RAR/7Z/ZIP/GZIP/BZIP2/XZ 签名。 | 反扫描/反去重 | 会干扰第三方扫描器（也干扰我们自己，靠 J-8/J-9 化解） | 🔄待采用（可选，写到文件末尾） |
| C-23 | mkv 输入 >2GB 硬拒绝 | 隐写者 `Steganographier.py:3475` 附近（CLI/GUI 双拦） | 超限直接报错。 | 防坏产物 | 限制用户 | 🔄待采用 |
| C-24 | 进度上界估算 `overhead = 8 + 256` | 隐写者 `Steganographier.py:2105` | 按「最多多写 264 字节」估总量。 | 进度不回退 | 引入噪声后不再准 | 🔄待采用 |

---

## ⑤ 修补（目录重建、偏移修复、数据恢复、CRC 引导、分段重连、部分损坏、救援）

| # | 技术点 | 出处(文件:行) | 做法 | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| F-1 | 7z 头损坏恢复（自行实现）：末尾反向找 `0x17/0x01` 多候选 + CRC 确认，**失败即 continue** | sevenz-rust `src/reader.rs:299-341`（库版是坏的：`L330-331` 用 `?`） | 逐候选解析 `kEncodedHeader` 的 StreamsInfo，能完整消费且文件数>0/名字合法才接受。 | 救回「StartHeader 被改」的包 | 候选多、成本高 | 🔄待采用（务必修正 `?`→`continue` 这个 bug） |
| F-2 | 7z 头丢/尾截断 = 基本报废（只剩 LZMA2 裸流，边界/名单全丢） | sevenz-rust `src/reader.rs:419-423`（无 Header 直接报错） | UI 如实说明「头丢＝无解」，不做无效抢救。 | 预期管理、不浪费用户时间 | 用户可能失望 | ❓未判定（需产品口径确认是否做裸流捞取） |
| F-3 | **假成功防护**：解压前后对输出目录做快照（路径→大小+mtime_ns）仲裁 | 隐写者 `Steganographier.py:2476-2500, 2634-2639` | 报了成功但目录没多文件/全是 0 字节→判假成功、**保留原件**。 | 唯一落地校验，防数据丢失 | 需排除中间目录前缀 | 🔄待采用（高价值） |
| F-4 | 只有确实写出文件才删原隐写文件 | 隐写者 `Steganographier.py:2634-2648` | 与 F-3 联动的删/留策略。 | 防误删唯一副本 | 无 | 🔄待采用 |
| F-5 | 成员级容错：单成员 CRC 错/写失败只跳过该成员、continue | 隐写者 `Steganographier.py:2915-2917` | `except (OSError,IOError): continue`，不让一个坏成员毁整轮。 | 部分损坏也能捞出其余 | 原实现会因成员密码错冒泡毁整轮（要修） | 🔄待采用 |
| F-6 | CRC 引导恢复：靠文件级 CRC32 判断哪段可信 | sevenz-rust `src/reader.rs:113-149` | 重建出的条目用 CRC32 校验，通过即可信。 | 免费质量信号 | 无 | ✅已采用（`src/rescue.rs` 用 CRC32 校验重建的本地头） |
| F-7 | 偏移修复用于「剥离隐藏数据、还原可播放原视频」 | 隐写者 `Steganographier.py:3198-3294`（reveal 侧是死代码，只隐写侧调用） | 复原时把 `stco/co64` 减去插入长度 + 容器递归。 | 可还原原始视频 | 与 C-7/C-9 同源的覆盖缺口 | 🔄待采用 |
| F-8 | 错密码会留下 0 字节/截断半成品且不清理（污染后续判定） | 隐写者 `Steganographier.py:3303-3365, 3103-3105` | 无预验证的三条路径都会先建文件。 | —— | 半成品不在快照排除名单→跨调用污染 | ⚠️已评估-不采用（我方：先预验证+原子落盘，绝不产半成品） |
| F-9 | `kAdditionalStreamsInfo` 不支持 → 遇到就整个打不开 | sevenz-rust `src/reader.rs:269-271` | 备用流直接报错。 | 明确失败 | 少数 SFX/加固包被挡 | ⚠️已评估-不采用（保持明确报错，不做静默降级） |
| F-10 | reveal 方法队列「静默回退」会掩盖失败 → 应记录实际命中方法 | 隐写者 `Steganographier.py:2502-2656` | 依次试 7zip→mp4_trailing→mp4_zarchiver→free_atom→mkv，任一成功即停，只写日志。 | 容错强 | 「某方法坏了」在用户体验上=没问题（掩盖缺陷） | 🔄待采用（我方上报**实际命中**的方法，不只布尔值） |
| F-11 | 批量统计必须真实：`不抛异常` ≠ 成功 | 隐写者 `Steganographier.py:3646-3680` | `reveal_file` 无返回值，调用方把「不抛异常」计成功→统计恒等于处理数（假的）。 | —— | 100% 成功假象 | 🔄待采用（我方 CLI 回报真实成败 + 失败名单） |

---

## 最值得采用的 Top 5

1. **7z 起始头双 CRC + 总长推导**（`sevenz-rust src/reader.rs:242-244, 366-369`）——已进 `src/scan.rs`，定位近乎零误报，是整个 7z 支持的基石。
2. **尾部 EOCD 反推 ZIP 起点 base**（`隐写者 Steganographier.py:2658/2833`）——已进 `src/layout/zip.rs`，把「整份伪装文件当 ZIP 打开」和普通模式一网打尽。
3. **`free` 原子封装 + 64–256B 随机 pad**（`隐写者 Steganographier.py:2093-2154, 2427-2438`）——Steganographier 普通模式的核心布局，**读侧必须支持**（配合 J-10 的正确判据，不能只看首字节）。
4. **`stco/co64` 偏移改写 + 容器递归**（`隐写者 Steganographier.py:3198-3294`）——ZArchiver 模式与「剥离还原原视频」都要它；同时必须补 `tfhd/sidx/saio` 或直接拒绝 fMP4。
5. **输出目录快照仲裁**（`隐写者 Steganographier.py:2476-2500, 2634-2639`）——唯一能防「假成功删原件」的落地校验，与 D-15 密码预验证一起用最稳。
6. **把 `sevenz-rust2` 当 7z 解压内核引入**（`src/encryption/*`, `src/decoder.rs`）——AES/中文密码 UTF-16LE/BCJ2/全家桶开箱即用；但定位、分卷、偏移、内存上限全在我方自己实现（R-1/R-2/J-17）。

---

## 附录 A：行号速查（本文引用）

**sevenz-rust（sevenz-rust2 0.24.0）**
| 主题 | 位置 |
|---|---|
| `Archive::read` 硬编码 `seek(Start(0))` | `src/reader.rs:197-199` |
| 版本校验 major==0 | `src/reader.rs:210-215` |
| 头有效性探针（bytes12..32 全0） | `src/reader.rs:219-227` |
| `StartHeaderCRC` 校验 | `src/reader.rs:242-244` |
| 损坏头恢复（含 `?` bug） | `src/reader.rs:299-341`（bug `L330-331`） |
| `init_archive`：长度/偏移越界 + NextHeaderCRC | `src/reader.rs:343-431`（350-359/366-376） |
| `read_encoded_header`（头被压缩/AES） | `src/reader.rs:433-486` |
| `kAdditionalStreamsInfo` 不支持 | `src/reader.rs:269-271` |
| `BoundedReader` / `SharedBoundedReader` | `src/reader.rs:25-49 / 54-101` |
| `Crc32VerifyingReader` | `src/reader.rs:113-149` |
| `bounded_count` 上限校验 | `src/reader.rs:292,521,836,872,1035` |
| `MAX_PREALLOC_BYTES=4MiB` | `src/reader.rs:20-23,1706-1707,1740` |
| `ArchiveReader`/`set_thread_count`/默认核数 | `src/reader.rs:1254-1330` |
| `read_file` / `BlockDecoder` | `src/reader.rs:1730-1745 / 1794-1897` |
| `build_decode_stack2`（BCJ2 四路） | `src/reader.rs:1423-1515` |
| `SIGNATURE_HEADER_SIZE=32` + NID 常量 | `src/archive.rs:5-38` |
| method-id 表 | `src/archive.rs:311-357` |
| AES props/KDF/raw-key/上限/缓存 | `src/encryption/aes.rs:131-230` |
| `Password`（UTF-16LE） | `src/encryption/password.rs:53-61` |
| coder→decoder 映射；LZMA2 字典算法 | `src/decoder.rs:92-208 / 255-271` |
| 错误类型 | `src/error.rs:5-54` |
| Zip-Slip `safe_join` / 默认 extract | `src/util/decompress.rs:153-172 / 189-234` |

**SteganographierGUI（`Steganographier.py`）**
| 主题 | 位置 |
|---|---|
| `compress_files`（反指纹打包） | `:1891-2051` |
| 有密码→pyzipper AES / 无密码→zipfile | `:1962-1985 / 1995-2051` |
| `hide_file` 主流程 | `:2058-2339` |
| 普通模式 mp4 分支 | `:2093-2154` |
| 随机 pad `randint(64,256)` | `:2137-2142` |
| mkv 分支（mkvmerge 附件） | `:2158-2198` |
| zarchiver 分支 | `:2201-2327` |
| 顶层原子遍历 | `:2264-2299` |
| **噪声块（误缩进进 while）** | `:2301-2323` |
| `get_output_file_path` | `:2342-2420` |
| `add_empty_mdat_box` | `:2422-2425` |
| `create_free_atom_header` / `create_free_atom_with_data` | `:2427-2438 / 2440-2443` |
| `load_passwords`（ASCII 过滤 bug） | `:2450-2469`（bug `:2458`） |
| `snapshot_output_files` | `:2476-2500` |
| `reveal_file`（方法队列+快照仲裁） | `:2502-2656` |
| `_try_mp4_direct_extraction` | `:2658-2705` |
| `_extract_with_7zip` | `:2707-2761` |
| `_extract_with_zipfile` / `_extract_with_pyzipper` | `:2833-2923 / 2925-3015` |
| `extract_with_offset_correction`（zarchiver 读端） | `:3018-3148` |
| `read_mp4_atoms` | `:3151-3195` |
| `find_and_update_offsets_in_atom`（stco/co64） | `:3198-3257` |
| `update_container_atom_offsets` | `:3259-3294` |
| `extract_from_free_atom` | `:3296-3390` |
| `_try_mkv_extraction` | `:3392-3466` |
| `build_temp_archive_path` | `:521-536` |
| `sanitize_path` | `:539-584` |
| `read_in_chunks`（8MB） | `:1699-1704` |
| `_resolve_conflict_path` / `_resolve_hide_conflict_path` | `:1733-1749 / 1759-1783` |
| `_extract_zip_members` / `_move_extracted_files` | `:1785-1813 / 1815-1834` |
| `choose_cover_video_file` / `initialize_cover_video_files` | `:1843-1889 / 1837-1841` |
| `check_size_and_duration` | `:670-687` |
| `process_hidden_files`（统计假成功） | `:3646-3680` |
| CLI `-t` choices 只有 mp4/mkv | `:3740` |

## 附录 B：要点提醒（避免踩坑）
- **不要**把带偏移的 `File` 直接交给 sevenz-rust（`BadSignature`）→ 用 `Window`（R-1）。
- **不要**用 `Password::from_raw` 喂 GBK/UTF-8 原始字节（7-Zip 只认 UTF-16LE）→ 先把文本解码成 `&str` 再 `Password::from`（D-2）。
- **不要**依赖库内头恢复（`try_to_locale_end_header` 用 `?` 是 bug）→ 自己写多候选+CRC（F-1）。
- **不要**照抄 zarchiver 的噪声缩进（产物不可播）与「只看首字节=PK」判据（漏 pad 布局）（C-7/J-10）。
- **不要**只按签名认定压缩包（作者故意撒 7 种假码）→ 一律结构校验（J-8/J-9）。
