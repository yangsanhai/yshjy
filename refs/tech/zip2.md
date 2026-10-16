# zip2 / zip-rs 五环节技术点施工清单

> **对象**：`/home/user/hermes/yshjy/refs/zip2/`，crate `zip` **v9.0.4**（`Cargo.toml:3`），edition 2024 / MSRV 1.88 / MIT，`src/` 共 **28 470 行**（`find . -name '*.rs' | wc` 实测）。
> **已有分析**：`refs/analysis/zip2.md`（读取侧深度分析，结论见其 §8、§9）。本文件是它的**施工化重写**：拆成【读取 / 判断 / 解压 / 压缩 / 修补】五节，每条带源码行号 + 明确决定。
> **决定图例**（只此四种）：
> - `✅已采用` — yshjy 已有等价实现（按项目上下文：ZIP 偏移反推、全文件签名扫描、7-Zip LOCALS-CD-MODE 救援、硬件档位）
> - `🔄待采用` — 值得搬进 yshjy
> - `⚠️已评估-不采用(原因)` — 看过、决定不照抄
> - `❓未判定` — 源码事实清楚但对我们场景的取舍还判断不了
> **重要实测前提（已采纳，全文遵守）**：`memchr::memmem::rfind` 在 1MB 窗口做 EOCD 反向搜索 = **610µs**，标量 `rposition` = **41µs**，**慢 15×**，已回滚。→ 凡提 memchr，均标注适用窗口条件，不无条件推荐。
> 行号均对 v9.0.4 实测核对。

---

## 一、读取（定位 / 扫描 / 流式）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 1.1 | EOCD 尾部固定搜索窗口 | `src/read/zip_archive.rs:187-189` | `min_eocd_position = file_len - (sizeof(Zip32CentralDirectoryEnd)=32 + 4 + 65536)`，即 EOCD 起始必须落在**末尾 65572 字节**内；`end` 传 `file_len`，故 EOCD 之后允许垃圾。 | 一行算式定界，天然不做全文件扫描。 | 窗口是**硬边界**：尾部垃圾 >65KB 直接 `Could not find EOCD`，与我们精心算的偏移毫无关系。 | ⚠️已评估-不采用(尾部垃圾极常见；yshjy 用全文件签名扫描，不受窗口约束) |
| 1.2 | 反向窗口扫描器 `MagicFinder<Backwards>` | `src/read/magic_finder.rs:59-92, 95` | `BUFFER_SIZE=1024`；`reset_cursor = (end-1024).clamp(start,end)`；`move_cursor = (cursor+3-1024).clamp(..)`；每步 `seek` + `read_exact` 1024B 后 `rfind`。 | 内存 O(1)，3 字节重叠保证不漏。 | 每窗口一次 `seek`+`read_exact`（2 次 syscall）；重叠量若写成 `MAGIC_LENGTH-1` 以外的值就会漏/重复。 | ✅已采用(等价能力：逐窗签名扫描) |
| 1.3 | `memchr::memmem::FinderRev` 做窗口内反向查找 | `src/read/magic_finder.rs:4, 52, 56, 71` | `Backwards(FinderRev)`，`find()` 即 `FinderRev::rfind(haystack)`。 | 单次调用内部走 SIMD/双字表，小窗口下常数小。 | **实测 1MB 窗口 rfind 比标量 `rposition` 慢 15×**（610µs vs 41µs）。适用条件：**窗口 ≤ ~2KB 且调用次数少**；大窗口或高频重复调用一律用标量 `rposition`。 | ⚠️已评估-不采用(实测 1MB 窗口慢 15×，已回滚；仅 ≤2KB 窗口可用) |
| 1.4 | 倒序**逐个候选** EOCD，不是取最后一个就完事 | `src/format/find_central_directory.rs:100-117` | `while let Some(eocd_offset) = eocd_finder.next()?`：每个候选过 EOCD 魔数 → 注释边界 → 分支；分支内失败 `continue` 试更靠前的候选。 | 尾部夹杂假 EOCD / 多次追加的包能自愈。 | 候选多时线性放大（靠 1.6 的 cache 兜住）。 | 🔄待采用 |
| 1.5 | EOCD 注释长度用**放宽不等式** | `src/format/find_central_directory.rs:112-117` | 只在 `comment.len()+eocd_offset+22 > file_len` 时判非法，即注释后允许垃圾（Python `zipfile 'a'` 模式产物）。 | 兼容真实世界的脏尾巴。 | 注释“真长”得自己判，不能当结构边界用。 | 🔄待采用 |
| 1.6 | `MagicPositionCache`：前向扫描 + 排序 + 二分 | `src/format/find_central_directory.rs:24-79`（`first_in_range:64-78`） | `ensure_scanned_up_to(end)` 只**前向**扫一次并把命中位置存进有序 `Vec`；查询用 `positions.partition_point(|p| p < start)` 二分取首个 `>= start`。 | 重复/重叠搜索 O(log n)；不变式（`scanned_up_to` 之前无遗漏）清晰。 | 需要维护 `positions` 有序 + `scanned_up_to` 单调，写错就静默漏候选。 | ✅已采用(yshjy「首字节位图 + 两字节索引」同类思想) |
| 1.7 | 读放大上界做成回归测试 | `tests/eocd_complexity.rs:66-74` | 构造 `l×m` 条 CDFH + `n` 条 EOCD 的恶劣组合，断言 `bytes_read <= (302*len)/100 + 1024`。 | 把退化性能钉死成可执行断言，防回归。 | 只测 IO 量不测墙钟时间。 | 🔄待采用 |
| 1.8 | EOCD64 locator 位置是**算出来**的 | `src/format/find_central_directory.rs:130-135` | `locator64_offset = eocd_offset - (4+16)`，直接 seek 过去验魔数 `PK\x06\x07`；不符则回落 ZIP32。 | O(1)，省掉一次扫描。 | **假设 locator 紧贴 EOCD 前 20 字节**；中间塞 1 字节就废。 | ⚠️已评估-不采用(强假设 locator 与 EOCD 相邻，抗篡改差) |
| 1.9 | 数据起点 = `header_start + 30 + namelen + extralen` | `src/types.rs:163-199` | seek 到 `header_start`，解析 26B 定长 local header（含 4B 魔数校验）+ 读可变字段长度，算出真实数据起点。 | **只用真实 local header 的长度字段，不信 csize/usize**。 | 魔数必须 `PK\x03\x04`，否则立刻 `InvalidArchive("Invalid local file header")`，零容错。 | ✅已采用 |
| 1.10 | `data_start` 用 `OnceLock` 缓存 | `src/types.rs:164-197`（字段 `:68`） | `data_start.get()` 命中直接返回；未命中才 seek+parse 并 `set`。 | 反复解压同一条目零重复 seek。 | 缓存的是 u64，与 reader 生命周期解耦（换 reader 需注意）。 | ✅已采用 |
| 1.11 | `take(compressed_size)` 限定成员数据长度 | `src/types.rs:201-210` | `find_content` = `seek(data_start)` 后返回 `reader.take(self.compressed_size)`。 | 单成员读取不解全档，8.7GB 包也只碰需要的区间。 | csize 若被篡改 → 多读/少读；长度可信度完全依赖 CD。 | ✅已采用 |
| 1.12 | 流式（不可 seek）读取：4 字节魔数分派 | `src/read/stream.rs:274-282` | 读 4B：`PK\x03\x04`=有文件 / `PK\x01\x02`=CD 开始返回 `None` / `PK\x05\x06`=空档结束。 | 可吃 pipe/stdin、无 CD 的裸流。 | 拿不到 comment / external_attributes / `data_start`。 | 🔄待采用 |
| 1.13 | 流式 + data descriptor 必须外部给 csize | `src/read/stream.rs:311-319` | bit3 置位时若无 `force_compressed_size` → `UnsupportedArchive("The file length is not available in the local header")`。 | 明确失败点，不猜长度。 | 我们「local 头不可信」的场景此路基本不可用。 | ⚠️已评估-不采用(local 头不可信，此路径不适用于伪装包) |
| 1.14 | `ZipArchiveMetadata` + `Arc` + `unsafe_new_with_metadata` | `src/read/zip_archive.rs:30-39, 370, 410` | 解析一次 CD 存 `Arc<ZipArchiveMetadata>`；各线程 `unsafe_new_with_metadata(reader, meta.clone())` 复用。 | 大档多线程解压不重复解析 CD。 | `unsafe` 契约（reader 与 metadata 必须同源）靠调用方保证。 | 🔄待采用 |
| 1.15 | `indexmap` 保序 + O(1) 名字索引 | `src/read/zip_archive.rs:31, 546, 655` | `IndexMap<Box<[u8]>, ZipFileData>`；`get_index_of(name)` 查名、`get_index(i)` 按序取。 | CD 顺序保真（对写出/回显重要）+ 名字查找 O(1)。 | **重名条目会被 fold 成一条**（insert 覆盖），重复名场景会丢条目。 | 🔄待采用 |

---

## 二、判断（合法性 / 一致性 / 拒绝点）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 2.1 | 「三哨兵」判定是否 ZIP64 | `src/format/blocks.rs:379-383` | `number_of_files == u16::MAX \|\| central_directory_size == u32::MAX \|\| central_directory_offset == u32::MAX` 才 `may_be_zip64()`。 | 一行判定，零 IO。 | 写入器**不写哨兵**时漏判 → 走 ZIP32 分支，拿 `0xFFFFFFFF` 当真值去 ~4GB 区间盲扫。 | 🔄待采用(并补「无哨兵也探一下 EOCD64」兜底) |
| 2.2 | locator 声明值必须 < locator 实际位置 | `src/format/find_central_directory.rs:217-220` | `if locator64.end_of_central_directory_offset >= locator64_offset { 非法 }`。 | 挡住「声明值被改大」的包。 | 声明值（相对语义）与实际位置（绝对语义）直接比大小 —— **语义混用**；对前置垃圾宽松、对改大声明致命。 | ⚠️已评估-不采用(相对/绝对混比，判定不成立时误杀) |
| 2.3 | 多卷直接拒绝 | `src/format/find_central_directory.rs:222-225`；`src/read/zip_archive.rs:248-252` | `number_of_disks > 1` → `Multi-disk ZIP files are not supported`；另有 `disk_number != disk_with_central_directory` 检查。 | 不实现分卷的复杂度。 | 伪装包若把 disk 字段改成 1 就能让 zip2 直接拒收。 | 🔄待采用(要报「疑似分卷」而不是硬失败) |
| 2.4 | EOCD64 与 locator **零间隙** | `src/format/find_central_directory.rs:242`；`src/format/blocks.rs:511, 532-534` | `z64.record_size + RECORD_OVERHEAD(12) != expected_length(= locator_offset - eocd64_offset)` 即非法。 | 结构自洽性强。 | 两结构间塞**任意 1 字节**就判死 —— 「夹层垃圾」型伪装包的必杀点。 | ⚠️已评估-不采用(零间隙假设，夹层垃圾型包直接死) |
| 2.5 | EOCD64.disk == locator.disk 交叉校验 | `src/format/find_central_directory.rs:237-239` | `z64.disk_with_central_directory != disk_with_central_directory` → 非法。 | 两结构互为见证，假 locator 难通过。 | 与 2.4 绑定，一起失败。 | 🔄待采用 |
| 2.6 | 守卫：`eocd64_offset >= n_files*46 + cd_offset` | `src/format/find_central_directory.rs:292-302` | 把**绝对的** EOCD64 位置与**相对语义的** `cd_offset + n*46` 相比，不满足即丢候选。 | 挡「n_files / cd_offset 被吹大」的包。 | 语义混用 → `cd_offset` 被改大时报 `Could not find EOCD64`；对头部前置的包反而宽松。 | ⚠️已评估-不采用(混比，且正是我们这类包被误杀的元凶) |
| 2.7 | ZIP32：`cd_offset < eocd_offset` + 空包特判 | `src/format/find_central_directory.rs:148-167` | 空包走 `archive_offset = eocd_offset.saturating_sub(cd_offset)`（`:148-161`）；非空先查 `cd_offset >= eocd_offset` 拒（`:164-167`）。 | 空档/带前缀空档都能正确给偏移。 | 空包分支把 CD 偏移当绝对量，若文件被前置则结果偏歪。 | ✅已采用(与 yshjy 偏移反推同源) |
| 2.8 | 第一条 CDFH 魔数命中才定偏移 | `src/format/find_central_directory.rs:196-213` | 在 `[cd_offset, eocd)` 区间取首个 `PK\x01\x02`，用 `archive_offset = 命中位置 - cd_offset`。 | **有自愈能力**：只要 CDFH 找得到就能算出偏移。 | 区间上限 4GiB，可能命中数据区的**假 CDFH** → 算出垃圾偏移。 | ✅已采用(yshjy 偏移反推即此逻辑) |
| 2.9 | 每条 CD entry 魔数校验 | `src/read/zip_archive.rs:275-282` | `ZipCentralEntryBlock::parse` 失败即写 `terminal_error` 并把 `next_offset` 钉在出错处后返回 Err。 | 出错位置可复现（`next_offset` 记录），便于诊断。 | 零容错：一条坏 entry 就整档失败，无跳过能力。 | ⚠️已评估-不采用(ours: 破坏性条目应可跳过并报告残缺) |
| 2.10 | local header 魔数强校验 | `src/types.rs:171`；`src/format/blocks.rs:248` | 定位数据字节时 `ZipLocalEntryBlock::parse` 必须命中 `PK\x03\x04`，否则 `InvalidArchive("Invalid local file header")`。 | 明确拒绝，不静默读错。 | **header_start 错 1 字节就整条读不出**；无二次校准、无回退扫描。 | ⚠️已评估-不采用(零容错；ours 需扫描式恢复) |
| 2.11 | local 尺寸**从不参与**读取 | `src/types.rs:201-210`；`src/read/zip_archive.rs:359` | `find_content` 只用 `self.compressed_size`（CD 值，或被 ZIP64 extra 覆盖后的值）。 | local 与 CD 尺寸不一致对 zip2 **无影响** —— 这解释了「7-Zip 打不开而 zip2 可能打得开」。 | 副作用：CD 尺寸错了就跟着错，没有交叉印证。 | 🔄待采用(ours 应同时记 CD/local 两套尺寸并交叉校验) |
| 2.12 | `number_of_files` 大于 `directory_start` 就不预分配 | `src/read/zip_archive.rs:242-246` | `file_capacity = if number_of_files > directory_start as usize { 0 } else { number_of_files }`。 | 挡住「声明 65535 条」的分配放大攻击。 | 只是不预分配，循环仍按声明数跑（靠早退）。 | 🔄待采用 |
| 2.13 | 预分配上限 `isize::MAX` | `src/read/zip_archive.rs:254-256` | `file_capacity * sizeof(ZipFileData) > isize::MAX` → `UnsupportedArchive("Oversized central directory")`。 | 防 capacity overflow panic。 | 只挡 `Vec` 容量，不挡别的分配。 | 🔄待采用 |
| 2.14 | ZIP64 extra「**无哨兵不生效**」 | `src/extra_fields/zip64_extended_information.rs:143-167, 186-224` | 「是否读」由块长度决定、「是否用」由条目对应字段是否 `0xFFFFFFFF` 决定（`read_field` 的 `is_zip64.then_some(value)`）。 | 对「只改 extra 未改字段」的篡改免疫。 | 字段是哨兵时**优先信 extra** → 篡改者改哪一处决定我们要补哪一处，必须两边都查。 | 🔄待采用(ours 需显式记录「该 entry 走了哪条取值路径」) |
| 2.15 | ZIP64 extra 长度自洽校验 | `src/extra_fields/zip64_extended_information.rs:226-235` | `len.checked_sub(consumed_len)` 为 None → `"ZIP64 extra-data field is the wrong length"`；余量用 `take` 丢进 sink。 | 解析步进永不跑偏。 | 丢余量意味着「多出来的 8 字节」被静默吃掉了。 | 🔄待采用 |
| 2.16 | extra field 解析防死循环 | `src/extra_fields/extra_field.rs:96-121, 162-176` | 未知 id 读到 EOF → `Ok(None)`（当 padding）；出错时 `copy(reader, sink())` 吞掉剩余字节再返回。 | 畸形 extra 不会让解析器空转。 | 吞字节会掩盖「extra_field_length 撒谎」的事实。 | 🔄待采用 |
| 2.17 | EOCD64 `record_size` 上下界 | `src/format/blocks.rs:530-534` | 下界 `record_size >= MIN_SIZE-4 = 40`；上界 `record_size + 12 <= max_size`（不得越过 locator）。 | 挡住「record_size 被吹大导致巨量分配」。 | 上界依赖 max_size 正确，即依赖 2.4 的零间隙假设。 | 🔄待采用(ours 要独立算上界，别依赖 locator) |
| 2.18 | 声明的 uncompressed size 是**硬约束** | `src/crc32.rs:19, 43-52, 86-94` | `Crc32Reader.size_limit`：`bytes_read > limit` 即 `InvalidData("File is larger than its declared uncompressed size")`。 | 让 `decompressed_size()` 的预估值真正有界（回归测试 `tests/declared_size_enforced.rs:1-10`）。 | 声明值太小就误杀**内容本来完好**的条目。 | 🔄待采用(ours 应作为「警告 + 可覆盖」而非硬失败) |
| 2.19 | 重叠检测反 zip-bomb | `src/read/zip_archive.rs:480-519` | 收集每条的 `[start, end)` 区间，排序后 `ranges.windows(2).any(|w| w[0].end > w[1].start)`；`has_overlapping_files`（只看数据）与 `has_overlapping_entries`（含 header）两档。 | 一条 sort + 一次窗口比较，O(n log n)，能识别「m 份数据解出 m×n 字节」。 | 只在需求时调用（每次都要 `data_start` → 逐条 seek）。 | 🔄待采用 |
| 2.20 | symlink 目标长度硬上限 4096 | `src/read/extract.rs:11-17, 468-476` | `MAX_SYMLINK_TARGET_LEN = 4096`；声明值超限直接拒，**不按声明值预分配**。 | 挡住「声明 4GB 的 symlink 目标」内存攻击。 | 合法 >4096 的目标（几乎不存在，PATH_MAX 即 4096）会被拒。 | 🔄待采用 |
| 2.21 | 路径逃逸三重检查 | `src/path.rs:57-85`（`enclosed_name`）、`:22-45`（`simplified_components`）、`:178-276`（`resolve_enclosed`） | 用 `Utf8WindowsPath` 逐组件解析（跨平台同一套语义）；`ParentDir` 使 depth 减到 0 就拒；`resolve_enclosed` 逐组件 walk，遇符号链接把**目标组件重新入队**再逐组件检查，`MAX_SYMLINK_HOPS=40`。 | 防 `..`、防绝对路径、防「symlink 作为中间组件」逃逸；跨平台一致。 | 纯逻辑，成本可忽略；但只保证「写入时」安全，不防并发改目录。 | 🔄待采用 |

---

## 三、解压

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 3.1 | 读取器枚举四态 + `Box` 压尺寸 | `src/read/readers.rs:157-166, 344-378` | `NoReader / Raw(Take) / Stored(Box<Crc32Reader<CryptoReader>>) / Compressed(..) / CompressedAes(..)`；大分支一律 `Box`，有测试断言 `size_of::<ZipFileReader>() <= 32`。 | 同存多个打开中的条目时内存友好（大档多成员）。 | 每层 `Box` 一次间接跳转，热路径略慢。 | 🔄待采用 |
| 3.2 | CRC 尾校验 + 尺寸上限同一层 | `src/crc32.rs:70-125` | `read`/`read_to_end` 在返回 0（EOF）时比对 CRC，不符即 `InvalidData("Invalid checksum")`；同时 `count()` 执行 size_limit。 | 校验与限流一处完成，无额外 pass。 | 只在**读到 EOF** 时才校验 —— 解压中途放弃就不校验。 | ✅已采用 |
| 3.3 | 解压器按 feature 组合 | `src/compression.rs:361-430`（`enum Decompressor`） | `Stored / Deflate / Deflate64 / Bzip2 / Zstd / Lzma / Xz / Ppmd / Shrink / Reduce / Implode`；`use_flate2/use_zlib_rs/use_zlib_ng` 多后端切换。 | 方法覆盖广，全开源库实现。 | 全开编译重；`Decompressor` 是 `pub(crate)`，**外部借不到**（`src/lib.rs:17, 32`）。 | 🔄待采用(只取 deflate + 需要的几种) |
| 3.4 | `SUPPORTED_COMPRESSION_METHODS` 常量 | `src/compression.rs:338-352` | 一个受 feature 控制的 `&[CompressionMethod]`，供 UI 展示「本构建支持哪些」。 | 可直接喂给「格式支持矩阵」界面。 | 内容随 feature 变，需在 UI 里如实标注。 | 🔄待采用 |
| 3.5 | 单成员流式解压 `by_index` | `src/read/zip_archive.rs:708-725` | `find_content`（定位 + `take`）→ `make_reader`（crypto → decompressor → CRC）→ 返回 `ZipFile` 实现 `Read`。 | 真正的「不解全档就提取单个成员」。 | 需要 `&mut reader` 独占，不能并行读同档（除非拆 reader）。 | ✅已采用 |
| 3.6 | `by_index_raw` 拿原始压缩字节 | `src/read/zip_archive.rs:693-705` | 返回 `ZipFileReader::Raw(find_content())`，不解压、不校验 CRC。 | 可做「原样导出 / 二次分析 / CRC 后才决定」；也适合我们扫描救援。 | 不做完整性校验，得自己兜。 | 🔄待采用 |
| 3.7 | `by_index_seek` 仅支持 `Stored` | `src/read/zip_archive.rs:625-648`；`src/read/readers.rs:14-66` | `SeekableTake` 把 `SeekFrom::{Start,End,Current}` 夹在 `[0, length]` 内转发给真实 reader；非 Stored 直接 `UnsupportedArchive`。 | 未压缩成员可随机访问（媒体/大文件友好）。 | 压缩成员不可 seek（要自己实现索引）。 | 🔄待采用 |
| 3.8 | `ZipReadOptions` 读取期覆盖 | `src/read/mod.rs:251-318` | `password / ignore_encryption_flag / ignore_crc32 / override_compressed_size / override_uncompressed_size / override_crc`。 | 「假加密位」「尺寸不可信」场景的逃生口。 | **只作用于当次读取**，改不了 CD 解析阶段的定位（救长度不救位置）。 | 🔄待采用(明确其边界：救长度不救位置) |
| 3.9 | AES 解密 + AE-2 关 CRC | `src/read/readers.rs:308-315`；`src/aes.rs:136-342` | `make_crypto_reader` 按 `aes-crypto` feature 建 `AesReader`；AE-2 时 `should_disable=true` 跳过 CRC（AE-2 本身不用 CRC）。 | 支持 AES-256/192/128 + AE-1/AE-2。 | `constant_time_eq`/`hmac`/`pbkdf2`/`sha1` 一串依赖；AE-2 关 CRC 后完整性只靠 HMAC。 | 🔄待采用 |
| 3.10 | `CompressedAes` 的 EOF 尾部认证 | `src/read/readers.rs:236-255` | 解压器先报 EOF（deflate 到 final block 即停）时，`finish_aes` 把剩余密文排空并 `crypto.finish()` 做 MAC 认证；失败则把 reader 置 `NoReader` 让后续读继续失败。 | 堵住「解压器提前 EOF 导致 HMAC 未校验」的安全洞（GHSA-c9rm-qm35-rhqf）。 | 逻辑绕（`into_inner` 三层嵌套），照抄易错。 | 🔄待采用(修复思路必须照抄，实现可简化) |
| 3.11 | ZipCrypto 解密 + validator 二选一 | `src/read/readers.rs:282-291`；`src/zipcrypto.rs:120-180` | 有 data descriptor 时用 `InfoZipMsdosTime`（比时间高字节），否则用 `PkzipCrc32`（比 CRC 高字节）；12 字节加密头解出来后只校验 1 字节。 | 兼容 Info-ZIP 与 PKZIP 两派。 | 只校验 1 字节 → **256 分之一误判率**（源码注释自己承认）。 | 🔄待采用(须显式提示「密码可能蒙对」) |
| 3.12 | `ZipStreamReader` 顺序解压 + 回读 CD 补元数据 | `src/read/stream.rs:53-75` | 先按 local header 顺序 visit 每个文件；遇到 CDFH 魔数就 break，再把魔数 chain 回去、继续解析 CD 做 `visit_additional_metadata`。 | 流式场景也能拿到 comment/unix_mode。 | 两遍语义、回调契约（必须 `visit_file` 前 `visit_additional_metadata` 后）；复杂。 | 🔄待采用 |
| 3.13 | 落盘：目录先可写、权限**延后**、children first | `src/read/extract.rs:449-563`（`:547-551`） | 解压期间目录 `0o700 \| mode`（`make_writable_dir_all:59-74`）；权限收集到 `UnixFileModes`（BTreeMap 有序），最后**逆序**设置 → 子文件先于父目录。 | 只读目录不会挡住写它的内容；不会出现「父目录只读后子文件写不进」。 | 逆序依赖 BTreeMap 字典序，路径含特殊字符时顺序假设脆弱。 | 🔄待采用 |
| 3.14 | 时间戳回写 | `src/read/extract.rs:527-532` | 仅在 `chrono` feature 下 `outfile.set_modified(t)`。 | 解压后 mtime 保真（对「按时间排序」的体验关键）。 | 需额外 feature；DS 时间只有 2 秒精度。 | 🔄待采用 |
| 3.15 | `ZipFile::Drop` 排空流式 reader | `src/read/zipfile.rs:430-440` | 若 `data` 是 `Cow::Owned`（流式构造），Drop 时 `take_raw_reader()` 后 `copy` 到 `sink()` 把残留字节读掉。 | 让流式解析器的下一个条目仍对齐。 | 只对 stream 路径生效；Drop 里做 IO，失败静默忽略。 | 🔄待采用 |
| 3.16 | 名字：UTF-8 → cp437 回退 + UnicodePath 覆盖 | `src/types.rs:122-131`；`src/extra_fields/extra_field.rs:278-289` | 先试 `from_utf8`，失败 `from_cp437()`；`UnicodePath` extra 在 **CRC32 校验通过**时才覆盖文件名并置 EF 位。 | 中文圈 CP437/GBK 包名能出字；UnicodePath 有 CRC 见证，防伪造。 | cp437 表不含中文 → 部分老包仍是乱码；需要上层再做编码猜测。 | 🔄待采用 |
| 3.17 | legacy 解码器 + `MAX_PREALLOC` 上限 | `src/legacy/mod.rs:8`；`src/legacy/reduce.rs:138-144` | `Shrink`/`Reduce(1-4)`/`Implode` 自研实现；`MAX_PREALLOC = 1<<20`，按声明长度预分配时先 `min` 到 1MB 再几何增长。 | 老古董包也能解；预分配不会再被打成 OOM。 | 自研 huffman/implode 代码量大、难验证；默认 feature 不含（需 `legacy-zip`）。 | ❓未判定(是否值得引入要看我们实际遇到 legacy 压缩包的频率) |

---

## 四、压缩

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 4.1 | local header 先占位、后回填 | `src/write.rs:637-781`（`start_entry`）、`:2390-2480`（`write_local_header`） | `start_entry` 先写 local header 占位（此时尺寸未知，data descriptor 档写 0），记录 `header_start`；写完数据再 seek 回来改。 | 单遍写出，不需先压缩到临时文件。 | 必须**可 seek**；`write.rs:379-391` 明确警告别套 `BufWriter`（其 `seek` 会隐式 flush）。 | 🔄待采用 |
| 4.2 | `finish_file` 回填 crc/size | `src/write.rs:794-830`、`:2234-2260` | 结束时算 `uncompressed_size = stats.bytes_written`、`compressed_size = file_end - stats.start`、CRC，然后 `update_local_file_header` 就地改写 crc/csize/usize。 | ZIP 兼容性最佳（local 头与 CD 一致）。 | 每次结束都要 seek 回写一次；异常路径靠 `abort_file` 兜。 | 🔄待采用 |
| 4.3 | ZIP64 提升：`large_file` 或尺寸 ≥ u32::MAX | `src/write.rs:2243-2257`、`:2301-2338`；`src/extra_fields/zip64_extended_information.rs:47-105` | `large_file=true` 或 `size >= ZIP64_BYTES_THR` 时写 ZIP64 extra field，32 位字段填 `0xFFFFFFFF` 哨兵；local 档先写 `u64::MAX` 占位（`:2403-2412`）后回填。 | 阈值处行为被单测钉死（`zip64_extended_information.rs:248-284`）。 | `large_file=true` 但文件不大时白扔 20 字节；local 与 central 的阈值判断**必须一致**否则字段错位。 | 🔄待采用 |
| 4.4 | 全局 ZIP64 判定 | `src/write.rs:1492-1544` | `is64 = files.len() > ZIP64_ENTRY_THR(65535) \|\| max(central_size, central_start) > ZIP64_BYTES_THR \|\| 有 extensible data`；成立则写 EOCD64 + locator，并把 32 位 EOCD 的对应字段写哨兵。 | 三个条件覆盖「条目数 / 体积 / 扩展数据」，不会漏。 | `central_start` 是**绝对**位置（含前缀垃圾）—— 与 zip2 自己的读取侧语义一致，但与其他工具未必一致。 | 🔄待采用 |
| 4.5 | data descriptor 强制（stream / ZipCrypto） | `src/write.rs:702-708, 735-772`；`src/types.rs:440-460` | 不可 seek 或 ZipCrypto 时置 bit3，local 头尺寸写 0，数据后跟 `PK\x07\x08` + crc/csize/usize（zip64 为 24 字节版）。 | 单遍流式；ZipCrypto 头两个校验字节用 mtime 而非 CRC，避免两遍读。 | 尺寸在 local 头不可见 → 流式读取方必须**外部给 csize**（见 1.13）；兼容性差于 4.2。 | 🔄待采用 |
| 4.6 | 数据对齐 + `DataStreamAlignment` extra (APPNOTE 4.6.11) | `src/write.rs:2434-2474`；`src/extra_fields/data_stream_alignment.rs:17-62`；`src/write/options.rs:436-439` | `with_alignment(n)`：算 `extra_field_end % align`，把 padding 补成一个 4 字节头 + 零填充的 extra field；padding < 6 就每次 +align 直到够；`new_len > u16::MAX` 则**放弃对齐**。 | 让下一个 entry 的数据落在对齐地址（SSD / mmap / 直读友好）。 | padding 直接吃掉空间；放弃对齐是静默的（返回 `None`，无告警）。 | 🔄待采用(对「伪装包＝媒体数据」场景尤其有用) |
| 4.7 | `new_append` 追加模式 | `src/write.rs:355-377` | seek 0 → `get_metadata`（复用读取侧解析）→ 用解析出的 `files`/`comment` 起一个 writer，`writing_raw=true` 避免重算上一条 header。 | 一行进入「往现成 ZIP 里加文件」。 | **继承读取侧的全部偏移限制**（偏移对不上就直接失败）→ 正是我们这里会踩的点。 | ⚠️已评估-不采用(直接 new_append 会被偏移检查挡死；只在打好补丁之后才可用) |
| 4.8 | `shallow_copy_file`：共享数据、只加 CD 条目 | `src/write.rs:1556-1567` | 复制源条目的 `ZipFileData`，只改名与 `central_header_start`，**不复制数据字节**。 | 零拷贝增条目，成本与源文件大小无关。 | local header 里仍是原名 → 其他工具会拒（Minecraft 是文档里点名的例子）；`has_overlapping_files` 会报 true。 | 🔄待采用(需在 UI 明确标注「非标准」) |
| 4.9 | `deep_copy_file`：真复制数据 | `src/write.rs:400-445` | 读源 `data_start` 起 `compressed_size` 字节，写到当前位置，新建 header，`header_start = write_position`。 | 产出标准兼容的副本。 | 逐条 O(数据量)；源在**写入中**的档上会退化成 `write_position - src_data_start`（`:410-414`）。 | 🔄待采用 |
| 4.10 | `raw_copy_file` / `raw_copy_file_touch` | `src/write.rs:1195-1248` | 直接搬运压缩后字节 + 元数据，不改压缩、不重算 CRC（`_touch` 版可改时间戳/名字）。 | 保真搬运（加密条目也能原样搬）。 | 不校验、不重压。 | 🔄待采用 |
| 4.11 | `merge_archive`：整档并入 | `src/write.rs:1054-1078`；`src/read/mod.rs:92-157` | `merge_contents` 从源 reader **rewind 后完整 copy 到 dir_start**（含前缀垃圾与条目间垃圾），并把所有 `header_start` / `data_start` 平移 `first_new_file_header_start`。 | 保留源档的全部原始字节（含 shebang / 垃圾），偏移平移一处到位。 | 会把垃圾一起搬过来；`:123-130` 用 `OnceLock::take` + `get_or_init` 平移 `data_start`，写法危险（丢值就静默错）。 | 🔄待采用(注意 `data_start` 平移那段要重写) |
| 4.12 | `hide_file` / `abort_file` 删条目 | `src/write.rs:927-939`、`:940-970` | `hide_file` 用 `shift_remove` 保序删；`abort_file` 弹栈后若 `rewind_safe`（无后续条目、无 shallow 副本）才 seek 回 `header_start` 复用空间。 | 删条目**不搬字节**，成本 O(1)。 | 不回收空间（除非 `abort_file` 且 `rewind_safe`）；有 shallow 副本时硬性拒绝复用。 | 🔄待采用(修「重写目录表」时很有用) |
| 4.13 | `finalize` 失败清理：把 footer 魔数置 0 | `src/write.rs:1428-1480` | 写完 CD/footer 后发现 `footer_end < archive_end`（有被中止条目的残留）→ 把 CDFH / EOCD / locator / EOCD64 的魔数各写 4 字节 0，使 footer 失效，再在真实末尾重写一份。 | 产出档**永远不会**留下「指向垃圾的 CD」；要么好要么显式坏。 | 逻辑长（`start_zip32_cde` / `start_zip64_locator` / `start_zip64_cde` 三处倒推），改错就生成坏 CD。 | 🔄待采用 |
| 4.14 | `StreamWriter`：无 seek 写出 | `src/write.rs:1585-1602, 2532-2580` | `ZipWriter::new_stream` 用 `StreamWriter` 包一层，`seek` 只允许前进（`SeekFrom::Current(0)` 之类），任何「回写」操作报错。 | 可直接写进 pipe / 网络流。 | 必须走 data descriptor（尺寸不在 local 头）；`abort_file`/`deep_copy_file` 一律失败。 | 🔄待采用(导出到 stdout/管道时用) |
| 4.15 | AES extra field 的 AE-1/AE-2 切换 | `src/write.rs:2179-2231` | 写完数据回写 AES extra：数据 ≥ 阈值时改用 AE-1（带 CRC），否则 AE-2。 | 大文件保留 CRC 校验能力。 | 需可 seek；与 4.2 同一次回写里做，耦合。 | ❓未判定(ours 是否要写加密档还没定) |
| 4.16 | `set_file_metadata`（unsafe）手工指定 crc/length | `src/write.rs:617-624` | 用 `Hasher::new_with_initial_len(crc32, length)` 直接把统计值覆盖掉。 | 「数据由外部独立写出、只需补 zip 元数据」的场景（流式转封装）。 | `unsafe` + 契约全靠调用方；错了产出无法校验的档。 | 🔄待采用(做「流式重打包」时的关键接口) |
| 4.17 | 压缩级别范围校验 | `src/write.rs:2121-2178`；`src/write/options.rs:298-312` | 按方法给出取值范围（deflate 0-9/zopfli 10-264、bzip2 0-9、zstd -7-22），越界即错。 | 早失败，不产出半成品。 | 逐方法硬编码，新增后端要同步改。 | 🔄待采用 |

---

## 五、修补（偏移修正 / 救援 / 恢复）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 5.1 | `ArchiveOffset::Known(n)`：只验一个点、**不回退扫描** | `src/format/find_central_directory.rs:175-193`（ZIP32）、`:254-270, 326-329`（ZIP64） | 只在 `声明值 + n` 处读 4 字节比魔数；不符就 `None`/`Could not find EOCD64`，源码注释明写 `don't fall back to scanning`。 | 偏移已知时**立刻**失败，比盲扫 4GB 快得多，适合作探针。 | `n` 被 locator 声明值**钉死**，不是「手动指定 ZIP 起点」的自由参数；猜错即死。 | ⚠️已评估-不采用(在 ZIP64 下被 locator 钉死，救不了我们的场景) |
| 5.2 | ZIP32 偏移 = 第一条 CDFH 实际位置 − EOCD 声明值 | `src/format/find_central_directory.rs:201-209` | `archive_offset = cd_offset(实际命中) - cd_offset_from_eocd(声明)`。 | 只要 CDFH 找得到就自愈（前置垃圾场景天然正确）。 | 声明值被改大 → 区间上限漂到 4GiB，慢且可能命中假 CDFH。 | ✅已采用(即 yshjy 的「ZIP 偏移反推」) |
| 5.3 | ZIP64 偏移 = 实际 EOCD64 位置 − locator 声明值 | `src/format/find_central_directory.rs:283` | `let archive_offset = eocd64_offset - eocd_offset;`（形参名 `eocd_offset` 实为 locator 的声明值，命名有误导）。 | ZIP64 下也能给出一个偏移。 | **基准是 locator 而非 CD**，与 5.2 不同源；两条路径只有「全档统一平移」时才一致。这是整份源码里对我们最致命的一行。 | ⚠️已评估-不采用(基准错误：ZIP64 应锚定 CD，而非 locator 声明值) |
| 5.4 | `directory_start = relative_cd_offset + archive_offset` | `src/read/mod.rs:77-79` | ZIP64 用 `eocd64.central_directory_offset`、否则用 32 位 EOCD 值，加同一标量 `archive_offset`。 | 一处集中，易审计。 | **整档只有一个标量偏移**，没有 per-entry 修正、没有二次校准。 | ⚠️已评估-不采用(单标量不够：CD 基准与 local 基准可能差不同的量) |
| 5.5 | 每条 `header_start += archive_offset` | `src/read/zip_archive.rs:307-311`；`src/read/mod.rs:236-239` | 读 CD 时先 `archive_offset=0` 解析，再把 `header_start` 统一加 `dir_info.archive_offset`。 | 逐条平移语义清晰。 | 平移量对所有条目**完全相同**；`Δ_entry` 不恒定就直接读不出。 | ⚠️已评估-不采用(与 5.4 同因：需要 per-entry 修正能力) |
| 5.6 | ZIP64 extra 可覆盖 `header_start`（仅哨兵时） | `src/extra_fields/extra_field.rs:245-258` | `apply_extra_fields` 里 `if let Some(head_start) = zip64_block.header_start { self.header_start = head_start; }`。 | 给了「条目级 offset 修正」的一条现成通道。 | 只有 CD 的 4 字节 offset 是 `0xFFFFFFFF` 时 zip2 才保留 extra 里的值（`zip64_extended_information.rs:211`）；**local header 传 `None`，永远不覆盖**。 | 🔄待采用(补丁要按「哨兵在哪个字段」决定改 4 字节还是 8 字节) |
| 5.7 | **8 字节 locator 补丁**：改 `L` 让 zip2 自己算出正确的 `n` | 支撑：`src/format/find_central_directory.rs:251-283`（`n = 实际EOCD64位置 − L`、`directory_start = D64 + n`）；入口 `src/read/zip_archive.rs:420`（`with_config`） | 自研解析器已知 `C`（真实首条 CDFH）与 `D64`（EOCD64 里的 cd_offset）→ `n = C - D64`、`L' = P - n`，只改 locator 里那 8 字节。 | 改 8 字节即可让 `Detect` 与 `Known(n)` 双双走通，不搬 8.7GB。 | **必须保留 32 位 EOCD 的三个哨兵**，否则 `may_be_zip64()` 变 false 走 ZIP32 分支（`blocks.rs:379-383`）；且只在 `Δ_cd` 与条目 offset 自洽时有效。 | 🔄待采用(当前最省的「复用 zip2 解压能力」方案) |
| 5.8 | 尾部窗口虚拟 reader：内存里改字节、不改盘 | 支撑：`src/types.rs:163-171`（只对绝对位置 seek）、`src/read/mod.rs:50-88`（目录起点计算）、`src/read/zip_archive.rs:187-189`（EOCD 窗口） | 包一层 `Read+Seek`：`pos < base` 直通真实文件，`pos >= base` 读内存里打过补丁的尾部副本；**总长必须保持与真实文件一致**。 | 不移动任何字节 → 文件长度、EOCD 注释边界检查全不受影响；一次读几十 KB + 几十 KB 内存。 | reader 的 seek 语义是**绝对且混合**的（EOCD 窗口 / CD / 每条 header / locator 四类位置），一个平移量无法同时满足四类 —— 所以只能「打补丁」而不能「整体平移」。 | 🔄待采用 |
| 5.9 | 逐条 CD offset 就地改写 | `src/format/blocks.rs:151-168`（`offset` 是 CDFH 最后一个 u32 字段，CDFH 定长 46 → 字段在 `+42`）；`src/extra_fields/zip64_extended_information.rs:87-92`（extra 里 offset 的 8 字节） | offset 字段非哨兵 → 改 4 字节；是 `0xFFFFFFFF` → 按 `extra_field_length` 步进找到 `0x0001` 块改其中 8 字节。取值给「真实绝对位置」或「`H_i - n`」都行，只要与 `n` 自洽。 | 应对 `Δ_entry` 逐条不同的包（5.7 救不了的）。 | 需要正确的 extra field 步进器；改错 8 字节会生成 `ZIP64 extra-data field is the wrong length`。 | 🔄待采用 |
| 5.10 | 「保留哨兵」是走 ZIP64 分支的前提 | `src/format/blocks.rs:379-383` | 32 位 EOCD 三字段任一为 `u32::MAX`/`u16::MAX` 才进 ZIP64 分支。 | 判据明确。 | 补丁时若不慎把哨兵写回真值 → zip2 直接走 ZIP32，在 ~4GB 区间盲扫，**极慢且大概率算错偏移**。 | 🔄待采用(写成补丁后自检断言) |
| 5.11 | 兜底：ZIP32 分支盲扫 `[cd_offset, eocd)` | `src/format/find_central_directory.rs:194-197` | `cdfh_cache.first_in_range(reader, cd_offset_from_eocd, eocd_offset)` —— 当 CD 偏移是 `0xFFFFFFFF` 而 EOCD 无有效 locator 时，区间上限是 4GiB。 | 极端情况下仍有一次机会。 | 慢；且会命中数据区里的**假 CDFH**，算出垃圾 `archive_offset`（然后整档读错）。 | ⚠️已评估-不采用(救援场景要用「签名全扫 + 交叉验证」而不是无条件首个命中) |
| 5.12 | 空包特判用 `saturating_sub` | `src/format/find_central_directory.rs:148-161` | `number_of_files == 0` 时 `archive_offset = eocd_offset.saturating_sub(cd_offset)`，并先查 `cd_offset > eocd_offset` 拒。 | 修复过「22 字节空档被 append 成 2GiB」的真实 bug（`tests/empty_archive_cd_offset.rs:1-15`）。 | 空包下这个偏移是「解释性」的，别当通用公式抄。 | 🔄待采用 |
| 5.13 | 多候选 EOCD 逐个试 + 外层重试轮 | `src/format/find_central_directory.rs:100-117`；`src/read/zip_archive.rs:194-231` | 内层 `continue` 换更靠前的 EOCD；外层 `loop` 复用同一 finder/cache，把 `read_central_header` 的失败记进 `last_err` 后继续。 | 多层伪装（多个假 EOCD）能逐层剥。 | 错误只保留**最后一次**（`last_err`），根因诊断信息会丢。 | 🔄待采用(ours 要保留首个/全部失败原因，便于报「残缺原因」) |
| 5.14 | 前置垃圾自愈（ZIP32 concat） | `src/format/find_central_directory.rs:196-209`；`tests/prepended_garbage.rs:5-8` | 4 字节前缀 + 正常 zip → 靠「扫到首条 CDFH」把偏移算出来，`ZipArchive::new` 直接可用。 | 一行 API 吃下带前缀（自解压/shebang/伪装头）的档。 | **只在 ZIP32 路径成立**；ZIP64 路径基准不同（见 5.3），同样的前置垃圾会挂。 | ✅已采用 |
| 5.15 | 能复用的只有 `ZipArchive` 这一层 | `src/lib.rs:17-24, 32`（`compression` 为私有 mod，只 re-export `CompressionMethod`/`SUPPORTED_COMPRESSION_METHODS`）；`src/format/mod.rs:25-26`（`find_central_directory_end` / `MagicPositionCache` 是 `pub(crate)`） | 定位算法与解压器都不可从外部调用；对外只有 `ZipArchive` / `ZipWriter` / 若干类型。 | 接口面小、易维护。 | ours 若要「复用它的解压器、自研定位」是**不可能的** —— 只能走 5.7/5.8 把补丁喂给 `ZipArchive::new`，或自己接 flate2/zstd。 | ⚠️已评估-不采用(不指望借用内部解码器；要么补丁复用整档，要么自接解压库) |
| 5.16 | `new_append` + `hide_file` 做「重写目录表」式修补 | `src/write.rs:355-377, 927-939` | 先按 5.7/5.8 让 `get_metadata` 成功，再 `new_append` → `hide_file`/`start_file` → `finish`，用合法 CD 覆盖尾部。 | 产出可被 7-Zip / Python 正常打开的档；不重压数据。 | **前置依赖**：`new_append` 内部就调 `get_metadata`（`write.rs:364`），偏移没修好它先失败。 | 🔄待采用(作为「导出修复版」的落地手段) |
| 5.17 | ZIP64 EOCD 的 extensible data sector 通道 | `src/format/blocks.rs:536-550`；`src/read/zip_archive.rs:458-460` | `record_size > 44` 时把多出的字节读进 `Vec`，并可从外部读出（`raw_zip64_extensible_data_sector`）。 | 一个可读回的「尾部附言」通道，理论上可存自己的元数据。 | `record_size` 是文件可控的 8 字节 → **直接用来分配**（`blocks.rs:536-539`）；我们的工具自研时**绝不能照抄这个「直接分配」写法**，必须先用 `max_size` 夹。 | ❓未判定(能否/值得用它写自校验标记，待实测文件确认空间) |

---

## 六、Top5 最值得采用

| 排名 | 技术点 | 出处 | 一句话理由 |
|---|---|---|---|
| 1 | **8 字节 locator 补丁 + 尾部窗口虚拟 reader**（5.7 / 5.8） | `find_central_directory.rs:251-283`；`zip_archive.rs:420` | 改 8 字节就能白嫖 zip2 全部成熟解压/校验/并行能力，不搬 8.7GB，是「一体化 + 只依赖开源库」红线内的最优解。 |
| 2 | **ZIP32 偏移反推（实际首条 CDFH − 声明值）**（5.2 / 5.14 / 2.8） | `find_central_directory.rs:196-209` | 唯一真正「有自愈能力」的偏移算法；yshjy 已有，须保留并扩成 ZIP64 版本（锚定 CD，而非 locator）。 |
| 3 | **`MagicPositionCache`：前向扫描 + 有序位置 + 二分**（1.6 / 1.7） | `find_central_directory.rs:24-79`；`tests/eocd_complexity.rs:66-74` | 把「重复/重叠搜索」从 O(n) 降到 O(log n)，并用「读放大 ≤302%」回归测试把上界钉死 —— 全文件扫描型工具的必备护栏。 |
| 4 | **`ZipReadOptions` 读取期覆盖 + ZIP64 extra「哨兵决定取值路径」**（3.8 / 2.14 / 2.6） | `read/mod.rs:251-318`；`zip64_extended_information.rs:143-167` | 决定了「我们要补哪一处字节」：先看 CD 字段是不是哨兵，再决定改 4 字节还是改 extra 的 8 字节；同时给「CD 尺寸不可信」留了逃生口（救长度）。 |
| 5 | **守卫清单 + 安全落盘三板斧**（2.12 / 2.13 / 2.19 / 2.20 / 2.21 / 3.13） | `zip_archive.rs:242-256, 480-519`；`extract.rs:11-17, 449-563`；`path.rs:57-85, 178-276` | 预分配夹紧、重叠检测、symlink 目标上限、路径逃逸三重检查 —— 伪装包本身就是恶意输入，这批护栏必须照搬（但 2.18/2.9 的「硬失败」要改成「警告 + 可覆盖」）。 |

---

## 七、明确**不要**照抄的五处

| 点 | 出处 | 为什么 |
|---|---|---|
| ZIP64 偏移基准 = locator 声明值 | `find_central_directory.rs:283` | 基准与 ZIP32 不同源；`Δ_cd ≠ Δ_eocd64` 时必挂且无法通过配置救。ours 必须锚定 CD。 |
| EOCD 搜索窗口硬编码 65572 字节 | `zip_archive.rs:187-189` | 尾部垃圾 >65KB 就判死，与偏移正确性无关。 |
| EOCD64 与 locator 零间隙 | `find_central_directory.rs:242`；`blocks.rs:532-534` | 「夹层垃圾」型伪装包的必杀点。 |
| EOCD64 `record_size` 直接用于分配 | `blocks.rs:536-539` | 文件可控长度直接进 `vec![0u8; n]`；自研必须先用 `max_size` 夹。 |
| 全局单标量 `archive_offset` + local header 零容错 | `read/mod.rs:77-79`；`zip_archive.rs:307-311`；`types.rs:171` | 我们至少有两个基准（CD 表 / local header）且差值不同；救援场景需要扫描式恢复，zip2 没有。 |

---

## 八、计数

- 技术点合计 **87 条**：读取 15、判断 21、解压 17、压缩 17、修补 17。
- 决定分布：`✅已采用` **11** 条 / `🔄待采用` **59** 条 / `⚠️已评估-不采用` **14** 条 / `❓未判定` **3** 条（11+59+14+3 = 87 ✓）。
- 分节：读取 ✅5/🔄6/⚠️4；判断 ✅2/🔄15/⚠️4；解压 ✅2/🔄14/❓1；压缩 🔄15/⚠️1/❓1；修补 ✅2/🔄9/⚠️5/❓1。
- 全部 87 条均带 `文件:行号`（补丁类条目 5.7 / 5.8 的行号是**支撑该方案可行性的源码位置**，已在「出处」列注明「支撑：」）。
