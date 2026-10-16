# 7-Zip 技术点拆解 · 五环节施工清单

> 对象：`/home/user/hermes/yshjy/refs/7zip/`（tag `26.04`，commit `9128b80`）
> 出处列的行号**全部**以该 checkout 为准。**只在 `CPP/` 下取源**（GUI / 加密 / 安装包未看）。
> 「决定」四值：✅已采用 / 🔄待采用 / ⚠️已评估-不采用(附原因) / ❓未判定
> 「已采用」= 当前 yshjy(`src/`) 已落地；「待采用」= 能搬进 Rust 但还没做。

---

## 一、读取（I/O 层：文件、缓冲、虚接流）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 1.1 | 短读重试循环 `ReadStream` | `Common/StreamUtils.cpp:54` | 把任意大小拆成 ≤`kBlockSize`(1<<31) 的块循环 Read，累计 processedSize，`processedSize==0` 才收工 | 屏蔽 OS/网络盘短读；一次调用拿到"要多少给多少" | 死循环风险（若 stream 返回 0 而不报错，靠 67 行提前 return 兜住） | ✅已采用（我们的 `read_exact`/`Read` 语义等价） |
| 1.2 | 短读与错误**分开表达** `ReadStream_FALSE` / `_FAIL` | `Common/StreamUtils.cpp:73,80` | 短读返回 `S_FALSE`，读错误返回 `E_FAIL`，调用方分别处理 | 让"文件被截断"与"磁盘坏了"走不同分支——救援模式必需 | 多一层错误码映射 | 🔄待采用（我们的 `Truncated` 与 IO 错误已分开，但缺少统一的"读满 N 字节"助手） |
| 1.3 | 全量写循环 `WriteStream`（0 进度 = 失败） | `Common/StreamUtils.cpp:87` | 写循环里若 `processedSizeLoc==0` 直接 `E_FAIL` | 防"写了一半当成功" | 无 | 🔄待采用 |
| 1.4 | 无 `GetSize` 接口时用 seek 尾模拟取长度 | `Common/StreamUtils.cpp:35` `InStream_GetPos_GetSize` | 先试专用接口，失败则 `SeekToEnd` 取长度再 seek 回原位置；取长度失败会回滚位置 | 兼容"只能顺序读"的数据源（手机 SAF 流） | 每次多两次 seek；对不可 seek 源无效 | ✅已采用（`io/window.rs` 由上层直接携带 `len`，不靠探测） |
| 1.5 | 统一块缓冲 `CInBuffer`（自带 `ReadBlock`） | `Common/InBuffer.cpp:20,51,117,165` | 一块 `MidAlloc` 缓冲 + `_buf/_bufLim` 指针对，`ReadBytes`/`Skip` 尽量在缓冲内完成，跨界才 `ReadBlock` | 头解析时把大量小 Read 合并成块读；`Skip` 不用真读数据 | 缓冲大小固定，需按用途调（见 1.13） | 🔄待采用（现在靠 `BufReader`，`Skip` 会真读数据） |
| 1.6 | 输出环形缓冲 `COutBuffer` + `FlushPart` | `Common/OutBuffer.cpp:9,40,49,86` | `_pos/_streamPos` 双指针环形，`FlushPart` 只提交未落盘区间，`GetProcessedSize` 单独统计总量 | 解压输出零拷贝落盘；进度统计与落盘解耦 | 环形指针在解析器复位时容易错（7-Zip 用 `_overDict` 兜） | 🔄待采用 |
| 1.7 | LZ 回拷窗口 `CLzOutWindow::CopyBlock` | `Compress/LzOutWindow.h:18`；窗口大小见 `Compress/DeflateDecoder.cpp:173` | 两支路径：不跨界用逐字节拷，跨界用"环回 + `FlushWithCheck`" | 32KB/64KB 窗口内不解引用越界 | 自己写就是一堆边界 bug | ⚠️已评估-不采用（Deflate 交给 `flate2`/`miniz_oxide`，窗口在库内，自研无收益） |
| 1.8 | 生产者/消费者"只递指针"`CStreamBinder` | `Common/StreamBinder.cpp:72,114` | Write 侧只把 `(ptr,size)` 交给 Read 侧，用事件+信号量（maxCount=3）交接；读完才释放写方 | 大块数据零拷贝，解压→落盘不落中间缓冲 | 需为"写方被中断"设计 `k_My_HRESULT_WritingWasCut` | 🔄待采用（用于"解压流直通写盘"，避免整包进内存） |
| 1.9 | 边读边算 CRC `CSequentialInStreamWithCRC` | `Archive/Common/InStreamWithCRC.cpp:7` | 在 Read 包装层里累计 `CrcUpdate`，不额外过一遍数据 | 零成本校验；`GetSize/GetCRC` 随取 | CRC 表初始化必须先行 | 🔄待采用（`scan.rs:crc32` 已有，缺"解压时同步校验"接线） |
| 1.10 | 尾部零字节探测 `ReadZeroTail` | `Archive/Common/FindSignature.cpp:66` | 2KB 块扫到第一个非零字节即返回，并累计已扫零字节数、超上限即停 | 快速判定"尾部是不是零填充/被截断" | 只判零，不判含义 | 🔄待采用（用于"尾部补零"型伪装包判定） |
| 1.11 | 文件块读 256KB `kClusterSize` | `Common/FileStreams.cpp:65,164` | 设备/大文件走 256KB 块缓冲读，`MidAlloc` 惰性分配，跳过区间不做真实读 | 机械盘顺序吞吐拉满；读大包时减少 syscall | 内存多 256KB；对随机读无益 | 🔄待采用（我们现在扫描用的块大小需与硬件档位联动，见 `src/hardware.rs`） |
| 1.12 | 单一解析缓冲 + 前向预读 `LookAhead` | `Archive/Zip/ZipIn.cpp:32,888,282` | Zip 头解析全程复用同一块 `kSeqBufferSize=1<<17`(128KB)，`LookAhead` 只做"memmove 掉已消费部分 + 续读" | 不重复分配，头解析跨多条目仍只读一遍 | `_bufPos/_bufCached` 两个游标容易算错 | 🔄待采用（`rescue.rs` 现在按需小读，大包会很多次 syscall） |
| 1.13 | 环形扫描窗口：前留 64KB / 后留 1MB | `UI/Common/OpenArchive.cpp:2362,2363,2364`；窗口维护 `2510-2526` / `2528` | `kBeforeSize=1<<16`、`kAfterSize=1<<20`、`kBufSize=1<<22`（须 > 前者之和）；不够时 memmove 保留前段、续读后段 | 让 `SignatureOffset>0`（TAR=257、EXT=0x438）与"需要回读"的格式不丢命中 | 内存 4MB 常驻；`kBufSize>kBefore+kAfter` 是硬约束，写错就静默漏段 | ✅已采用（`scan.rs` + `io/segment.rs` 已是窗口模型；**但"前留 64KB"这一条需核对**） |
| 1.14 | 只有 u32 的流接口 | `Common/StreamUtils.cpp:9,60` | 全部读写切 ≤2GB 块 | 兼容老 32 位平台 | Rust `Read/Write` 用 `usize`，无需此约束 | ⚠️已评估-不采用（COM `UInt32` 接口的产物，Rust 侧无对应问题） |
| 1.15 | 内存受限流 `CLimitedSequentialInStream` | `Common/LimitedStreams.h:24,48,59` | 把 `[pos,pos+size)` 映射成"从 0 开始、读到 size 即 EOF"的流 | 内嵌包解析的标准做法（解析器以为自己在文件头） | 越界需返回 `S_FALSE` 而非 panic | ✅已采用（`io/window.rs`） |
| 1.16 | 偏移输出流 `COffsetOutStream` | `Common/OffsetStream.h:11` | 写入时统一加基址，用于"已知偏移、直接落盘" | 局部覆盖写（回填 local header）必需 | 需要可写 + 可 seek 的目标 | 🔄待采用（未来打包端回填 local header 用） |
| 1.17 | 多线程共享输入流加锁 `CLockedSequentialInStreamMT` | `Archive/7z/7zDecode.cpp:137,152` | 多线程解压共用一个打包流时，Read 全程持锁 | 避免各线程 seek 打架 | 串行化了输入侧，锁竞争 | 🔄待采用（仅在做"多文件并行解压"时才需要） |
| 1.18 | Linux 端不用 `posix_fadvise`/`O_DIRECT`，仅 `O_RDONLY` | `Windows/FileIO.cpp:906`（全树 grep 无 fadvise/O_DIRECT） | 完全依赖 OS 页缓存 + 大块顺序读 | 跨平台实现零特判；Android 同样受益 | 机械盘随机读无抢占提示 | ⚠️已评估-不采用（平台特有 API 违反跨平台红线；瓶颈已在 1.11 块读解决） |
| 1.19 | 读越界用 C++ 异常表达"半成品可用" `CUnexpectEnd` | `Archive/Zip/ZipIn.cpp:937,2069-2074` | 读到意外结束抛 `CUnexpectEnd`，上层 catch 后**保留已解析条目**继续 | 截断包能"捞到多少算多少" | 控制流靠异常，Rust 里要显式建模 | 🔄待采用（Rust 用 `(Vec<Entry>, Truncated)` 返回，语义等价，需补"部分结果"API） |

---

## 二、判断（识别：签名扫描 + 弱/强判定）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 2.1 | 2 字节 u16 哈希 + 链表索引签名 | `UI/Common/OpenArchive.cpp:1073,2362-2424` | `kNumHashBytes=2`、`HASH_VAL=GetUi16`，建 `hash[65536]` + `prevs[]` 链，把每个签名的首两字节映射到候选格式 | 单遍扫描即可服务全部格式；比"每签名各扫一遍"快一个数量级 | 签名 <2 字节只能进 difficult 表 | ✅已采用（`scan.rs:SigIndex`，与"首字节位图"组合） |
| 2.2 | 哈希空位快跳粗筛 | `UI/Common/OpenArchive.cpp:2596` | `for (; buf<bufLimit && hash[HASH_VAL(buf)]==0xFF; buf++);` 一步跳过整片"不可能命中"的字节 | 8.7GB 上主要加速来源 | 依赖"空 = 0xFF"哨兵，索引建错就漏 | ✅已采用（`scan.rs` 位图/索引粗筛） |
| 2.3 | 命中即整段跳过 | `UI/Common/OpenArchive.cpp:2855` | 成功识别一段后 `pos = pi.Offset + pi.Size`，跳过整段 | 防止 ZIP 内部成百上千个 `PK\x03\x04` 被误当独立段 | 段长算错会**跳过真包** | ✅已采用（`scan.rs` 命中后按段前进） |
| 2.4 | 未知段填空、无缝覆盖 `[0,EOF)` | `UI/Common/OpenArchive.cpp:224,253,2984` | `AddItem` 前先 `AddUnknownItem(item.Offset)`，收尾再补 `fileSize` | 输出天然是"外壳/真身/尾部"三段，直接支撑"丢壳取核" | 段序维护需按 Offset 二分插入 | ✅已采用（`io/mod.rs:Segment` + 尾部未知段） |
| 2.5 | **弱判定**：`searchLimit==0` 时只认流首 4 字节 | `Archive/Zip/ZipIn.cpp:628-653` | 只要当前位置是 `PK\x03\x04`/`PK\x05\x06`/`PK\x06\x06` 之一即返回 `S_OK`，错误交给后面的解析暴露 | 这正是"把 handler 摆到内嵌 local header 上"的关键，坏 CD 的包才能进救援路径 | 弱判定会放进假阳性，必须靠后续结构校验兜 | ✅已采用（`rescue.rs` 从扫描命中的 local header 直接起解，不要求 CD 自洽） |
| 2.6 | **强判定**：全文件找 marker + 内容校验 | `Archive/Zip/ZipIn.cpp:618,723-756` | 128KB 缓冲里用 `FindPK_4` 找 `PK`，再交给 `IsArc_Zip_2` 做结构判断，非 `NO` 才算命中，并记录 `MarkerPos/MarkerPos2` | 起点可从 Sfx/分卷/垃圾段中恢复 | 每命中一次都要 `rem` 长度检查，边界易错 | 🔄待采用（我们缺"在扫描态下反向确认 CD/ECD 一致性"的第二道判断） |
| 2.7 | 签名 + 起始头 CRC 双重校验 | `Archive/7z/7zIn.cpp:323,328,349` | `TestStartCrc`: `CrcCalc(p+12,20)==Get32(p+8)`；`TestSignature2` = 签名通过 **且** CRC 通过 | 假阳性率降到 ≈2⁻³² ，8.7GB 视频里几乎不会误判 | 每候选多一次 CRC32(20B)（可忽略） | 🔄待采用（**7z 支持尚未落地**，接入时必用） |
| 2.8 | 7z 专属 6 字节步长签名搜索 | `Archive/7z/7zIn.cpp:300-320` | 每次前进 6 字节只读 `p[5]`（=签名末字节 `0x1C`），命中才回退 0~5 字节确认整签名 | 比逐字节 memcmp 快 ~6×；内层循环无分支依赖 | 只对"末字节唯一"的固定长签名成立 | 🔄待采用（做 7z 扫描时照抄；其他格式不适配） |
| 2.9 | 每格式签名表 + `SignatureOffset` + flags | `Archive/Zip/ZipRegister.cpp:12-25`、`Archive/7z/7zRegister.cpp:12-18`、`Common/RegisterArc.h` | 注册 `(名字, 扩展名, 签名串, 相对起点偏移, flags)`；`kMultiSignature` 一组多签名、`kFindSignature` 可搜、`kUseGlobalOffset` 用全局偏移、`kStartOpen` 归入 difficult | 一处登记、扫描+打开+扩展名匹配全复用 | 7z 签名是"首字节 +1 存储"、注册时减回，**直接抄常量会抄错**（`7zRegister.cpp:12`） | ✅已采用（`scan.rs:SIGNATURES`，但**没抄 7z 的 +1 陷阱**，需在接入 7z 时注意） |
| 2.10 | difficult 格式（无签名/短签名/需 startOpen）逐位置试 | `UI/Common/OpenArchive.cpp:2375,2424` | 建表时把"签名 <2 字节 / 无签名 / 带 `kStartOpen` / 旧接口"挑出来单独成表，每个字节位置都试一次 | TAR(`ustar`@257)、LZMA、Base64、IHex 这类才有机会被发现 | 成本 O(n)×格式数，必须放在主扫描之后并加预算 | 🔄待采用（`scan.rs` 已有 TAR 偏移 257 特判，但没做成"低优先+限速"的独立通道） |
| 2.11 | 起始搜索 8MB 硬上限，扫描模式取消 | `UI/Common/OpenArchive.cpp:45,1666-1669,2578-2580` | 默认 `maxStartOffset=kMaxCheckStartPosition=1<<23`；`CanReturnParser` 时**跳过该限制一直扫到 EOF** | 解释了"普通模式看不到 119MB 处内嵌包" | 无上限 = 全盘开销，必须与硬件档位联动 | ✅已采用（`scan.rs:scan_whole_file` 扫全文件；8MB 限制我们不复刻） |
| 2.12 | "打不开但确认是归档"也要留证据 `IsArc_After_NonOpen` | `UI/Common/OpenArchive.cpp:1979,2747,2756,3116` | handler 返回失败但标志"这确实是归档"时，保留 `NonOpen_ErrorInfo`，用于继续扫描与更准确的报错 | 把"损坏"与"不是包"彻底分开——正是我们要的白话报错 | 需在 handler 层显式区分两种失败 | 🔄待采用（对应 `error.rs` 的 `NotArchive` vs `Truncated`: 已有雏形，缺"确认是归档但没打开"这一态） |
| 2.13 | 时间字段合理性校验 `CheckDosTime` | `Archive/Zip/ZipIn.cpp:380,467,1230` | 校验 DOS 时间字段合法；不合法只置 `HeadersWarning` 而不拒收 | 廉价降低误报，且对坏包宽容 | 只置警告，不阻断 | 🔄待采用（`rescue.rs` 未校验时间字段） |
| 2.14 | `IsStrangeItem` 单条目闸门 | `Archive/Zip/ZipIn.cpp:2036,2122,2138` | 名字长度 >16KiB 或压缩方法 >256 → 判"不像真归档"，单条目时直接 `S_FALSE` | 防"随机字节恰好长得像 local header" | 阈值硬编码，极端合法包会被拒 | ✅已采用（`rescue.rs:is_strange` 注释即注明对译自 L2036） |
| 2.15 | 描述符双重验证 `CheckDescriptor` | `Archive/Zip/ZipIn.cpp:1577-1623` | 读到数据描述符后校验 `PK\x07\x08` + descriptor 内 packSize 与实际位置一致 | 流式 ZIP 的尺寸/CRC 可信 | 不支持"无签名的老 PKZIP 描述符" | ✅已采用（`rescue.rs:find_descriptor` 做了等价双验） |
| 2.16 | 候选顺序：扩展名匹配优先 | `UI/Common/OpenArchive.cpp:2118,2189,2194` | 第一遍按"扩展名命中 + 长签名 + kFindSignature"排序试开，签名命中的插到队首 | 常见格式先成功，省掉整轮扫描 | 顺序表本身要维护 | 🔄待采用（`scan.rs` 未利用文件扩展名做加权） |
| 2.17 | OLE/EXE/ELF/Mach-O 标记为"可做外壳的开头" `kPreArc` | `Archive/PeHandler.cpp:3001`；`Archive/IArchive.h:58-67`(NArcInfoFlags) | 把可执行体标记为 preArc，扫描时特殊对待（SFX 自解压） | 明确"前段是壳"而不是"垃圾" | 与我们的"外壳=MP4/图片"场景不同 | ⚠️已评估-不采用（我们的外壳是视频/图片，MP4 box 无全文魔数；该机制解决的是 SFX 问题） |

---

## 三、解压（解码器装配、校验、线程与内存）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 3.1 | 解码器工厂：methodId → coder | `Common/CreateCoder.cpp:447,453,483` | `FindMethod_Index(methodId, encode)` 查注册表，再 `CreateCoder_Index` 实例化 | 格式层与算法层解耦，一种算法多格式复用 | 注册表初始化顺序问题 | 🔄待采用（`rescue.rs` 目前只出条目、不解数据；需建 coder 注册表） |
| 3.2 | ZIP method ↔ coder 映射 + 插件兜底 | `Archive/Zip/ZipHandler.cpp:1154-1197` | 内建 Store/Shrink/Implode/LZMA/Xz/PPMd/Zstd/LZFSE；其余走 `kMethodId_ZipBase + id` 查外部解码器，查不到报 `kUnsupportedMethod` | 老 ZIP 方法也能解；未知方法只影响单条 | `id > 0xFF` 直接放弃 | 🔄待采用（**"未知方法只废单条不废整包"这条要写进我们的错误策略**） |
| 3.3 | 解压后 CRC 比对 | `Archive/Zip/ZipHandler.cpp:1499` | `crcOK = (outStream->GetCRC() == item.Crc)`，`needCRC` 才比 | 唯一能证明"内容真的对"的手段 | 缺 CRC 的条目无从校验 | 🔄待采用 |
| 3.4 | 用解开尺寸判截断 `useUnpackLimit` | `Archive/Zip/ZipHandler.cpp:1246-1251,1501-1503` | 满足 Store / 无描述符 / Size≥4GiB / Zip64 之一时必须校验 `outStream->GetSize()==item.Size`，不等则 `truncatedError` | 抓"CRC 恰好没覆盖"的截断 | 有条件启用，不是全能 | 🔄待采用（救援模式**尤其**需要，残缺文件要标残） |
| 3.5 | 多余数据判定 `dataAfterEnd` | `Archive/Zip/ZipHandler.cpp:1236,1459-1473,1535` | `processed < coderPackSize` 且"预期读满流"时置位 → 结果降级为 `kDataAfterEnd` | 抓"packSize 写大了/包拼接错位" | 需先判 `isFullStreamExpected` | 🔄待采用 |
| 3.6 | 结果分级（而非成功/失败二元） | `Archive/IArchive.h:136-145` | `kOK / kUnsupportedMethod / kDataError / kCRCError / kUnavailable / kUnexpectedEnd / kDataAfterEnd / kIsNotArc / kHeadersError / kWrongPassword` | 逐条上报，UI 能给出"351 文件 / 347 完整" | 定义要一次定好 | 🔄待采用（`error.rs` 有 `Truncated/DataLost/NotArchive` 等，**缺 CRCError / UnsupportedMethod / DataAfterEnd / HeadersWarning**） |
| 3.7 | local header 不可信 → 只标 `kUnavailable` | `Archive/Zip/ZipHandler.cpp:1598-1616`；`Archive/Zip/ZipIn.h:407` | `IsLocalOffsetOK(item)` 为假则该条 `kSkip`/`kUnavailable`，**其他条目照常解** | 部分成功优于整体失败 | 用户需能分辨"跳过"与"没这个文件" | 🔄待采用（配合 5.12 一起做） |
| 3.8 | 逐条独立事务：PrepareOperation/SetOperationResult | `Archive/Zip/ZipHandler.cpp:1586-1672` | 每条文件走完整的准备→解码→回报，单条失败不 `return` 中断循环 | 大包解到第 900 条坏了不白干 | 回调状态机写错会导致条目错位 | 🔄待采用 |
| 3.9 | 解码器实例复用（按 method 缓存） | `Archive/Zip/ZipHandler.cpp:1145-1201` | `methodItems` 里按 `ZipMethod` 找已建 coder，找到就复用 | 省掉每条重建 coder 的初始化开销 | coder 有内部状态时必须 Reset | 🔄待采用 |
| 3.10 | 线程数注入解码器 `SetNumberOfThreads` | `Archive/Zip/ZipHandler.cpp:1205-1213`；`Compress/Lzma2Decoder.cpp:198` | QI `ICompressSetCoderMt`，有就 `SetNumberOfThreads(numThreads)` | 单条大文件也能多核解 | 部分解码器不支持 | 🔄待采用 |
| 3.11 | **内存上限推导线程数** | `Compress/Lzma2Decoder.cpp:110-129,204` | `okThreads = useLimit / (outBlockMax + inBlockMax + kOverheadSize)`，再夹到 [1, numThreads]；`outBlockMax` 由字典推导（见 3.12） | 内存不足时自动降线程而不是 OOM | 公式里的开销常量要实测标定 | 🔄待采用（**补强我们的"资源档位"：档位不该只给枚举，应给内存预算→线程数换算**） |
| 3.12 | 字典大小 → 块大小 | `Compress/Lzma2Decoder.cpp:61-68` | `blockSize = dictSize<<2`，夹在 `[dictSize, 1<<28]`（≥1MB） | 让"块 = 一次可独立解码单元"可控 | 与 Lzma2 规范耦合 | 🔄待采用 |
| 3.13 | 自建 MT 解码框架 `CMtDec` | `C/MtDec.h:20-120`、`C/MtDec.c` | 四回调 `Parse/PreCode/Code/Write`，多线程块流水，缓冲溢出降级为单线程 `MTDEC_PARSE_OVERFLOW` | 解码吞吐翻倍，且能安全回退 | 实现复杂度极高，并发正确性难验证 | ⚠️已评估-不采用（Rust 侧用 `liblzma` MT / `flate2` 即可；自研 3000 行并发框架收益不抵风险） |
| 3.14 | 7z solid 块多 coder 流水 `CoderMixer2` | `Archive/7z/7zDecode.cpp:29,240,280,290,503-576` | 把 folder 转成 bond 绑定图，多输入/多输出 coder 混流，MT/ST 两套 mixer | 7z solid + BCJ2 多流必须靠它 | 只有自研 7z 解析才需要 | ⚠️已评估-不采用（7z 解析交给 `sevenz-rust2`，不自己写 mixer） |
| 3.15 | 解密后必须读到结尾 | `Archive/Zip/ZipHandler.cpp:885-907,1505-1527` | `SkipStreamData` 把剩余打包数据读完，再校验 MAC；有剩余数据则 `dataAfterEnd` 或认证失败 | 防"截断的加密包被当成解成功" | 多一次全量读（只对加密条目） | 🔄待采用（ZipCrypto/AES 支持时必做） |
| 3.16 | 密码字节编码（ANSI vs UTF-8）的取舍注释 | `Archive/Zip/ZipHandler.cpp:1090-1130` | ZipCrypto 用本地 ANSI（CP_ACP，历史 API 用 OEM）；Unix 主机 + UTF-8 时用 UTF-8 | 说明"同一密码不同编码"的坑根因 | 7-Zip 只做单向转换，**没做多编码重试** | ✅已采用（我们 DESIGN 里的 GBK↔UTF-8 重试正好补上 7-Zip 的短板） |
| 3.17 | ZipCrypto 校验字：有描述符用时间低位 | `Archive/Zip/ZipAddCommon.cpp:295-314` | `descriptorMode` 时 check = `fileTime & 0xFFFF`（Info-ZIP 改法）；否则 check = `crc>>16` | 流式 + 加密的解/压双方对得上（隐写者 zip 的关键） | 两套规则都要实现 | 🔄待采用 |
| 3.18 | 时间字段解析/写回（DOS + NTFS + Unix 三套） | `Archive/Zip/ZipOut.cpp:141-168`；`Archive/Zip/ZipIn.cpp:1204,1230` | local/CD 里写 DOS 时间，可选写 `0x000A` NTFS 时间（8B×3）与 `0x5455` Unix 时间 | 跨平台时间不丢 | extra 长度超 64KiB 会抛（`ZipOut.cpp:193`） | 🔄待采用 |

---

## 四、压缩（打包端：写头、选法、并行）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 4.1 | Local header 写入 + 条件 Zip64 extra | `Archive/Zip/ZipOut.cpp:171-246,227-232` | `DOES_NEED_ZIP64(packSize/size)` 任一为真才写 `0x0001` extra（8B size + 8B packSize），并把版本升到 Zip64 | 小文件不为 Zip64 付 20B 代价 | extra 长度超 `UInt16` 直接抛 `E_FAIL` | 🔄待采用 |
| 4.2 | 流式写 + 数据描述符（尺寸/CRC 后置） | `Archive/Zip/ZipOut.cpp:207-219,249-268,271-290` | 有描述符时 local header 的 CRC/尺寸写 0，数据后追加 `PK\x07\x08`+CRC+尺寸；`WriteLocalHeader_Replace` 可在已知尺寸后回填 | 不必预知压缩后大小，可流式边压边写 | 生成"老解压器不友好"的包；ZipCrypto 下必须走描述符 | 🔄待采用 |
| 4.3 | Zip64 提前触发阈值 `0xF8000000` | `Archive/Zip/ZipAddCommon.cpp:120-132` | 解开尺寸 ≥0xF8000000 就**预判**用 Zip64（压缩可能膨胀 ~3%）；未知时先塞 `1<<60` 逼出 Zip64 | 避免"压完才发现 32 位放不下"重写包 | 阈值是启发式 | 🔄待采用（我们打包端若做 4G+ 包必需） |
| 4.4 | 中央目录 + EOCD64 + Locator 写序 | `Archive/Zip/ZipOut.cpp:357-410` | 先写全部 CD 条目，再按需写 ECD64→Locator→ECD；Zip64 触发条件 = 偏移/大小/条目数（≥0xFFFF）任一越界 | 与主流解压器完全兼容 | 三步顺序错位会让包"看起来正常但打不开" | 🔄待采用 |
| 4.5 | **试多种方法取最小** | `Archive/Zip/ZipAddCommon.cpp:243-479`（判定 `471-478`） | `MethodSequence` 里逐个方法压，压完 `if (PackSize < UnpackSize) break;`（加密另加 header 开销阈值）取第一个"变小了"的结果 | 可压缩的用 deflate、不可压的自动回落 store，不浪费 | 最坏情况压 N 遍（多遍 = N 倍 CPU） | 🔄待采用（**但只对 <1MB 小条目启用，大文件单遍**） |
| 4.6 | UTF-8 文件名 extra `0x7075` | `Archive/Zip/ZipOut.cpp:129-138`；`Archive/Zip/ZipHeader.h:97` | 写 `0x7075` extra：version=1 + `CRC32(原始名)` + UTF-8 名字 | 中文文件名在 Windows/安卓都能正确显示 | 名字变长；解压端需支持 | 🔄待采用（中资源圈刚需） |
| 4.7 | 时间 extra（NTFS 0x000A / Unix 0x5455） | `Archive/Zip/ZipOut.cpp:144-168` | local header 默认**不写** NTFS（省体积），CD 写；Unix 时间只在需要时可写 | 体积与兼容的折中 | 纯 local-header 解析的救援模式**读不到精确时间** | ⚠️已评估-不采用（我们把时间写齐到 local header，救援模式才拿得到；体积代价可忽略） |
| 4.8 | 逐条目并行压缩（每线程一个 coder + 共享输出定序） | `Archive/Zip/ZipUpdate.cpp:202-326,1247-1520` | `CThreadInfo` 每线程独立 coder + `COutMemStream`，主线程按序把内存块写盘；`CMtProgressMixer2` 汇总进度 | 多核打包命中率最高；输出顺序仍是确定的 | 内存块管理与取消路径复杂（`StopWait_Close`） | ⚠️已评估-不采用（打包端非核心场景；并发写同一输出需要复杂定序，移动端收益低） |
| 4.9 | 每线程内存常量 | `Archive/Zip/ZipUpdate.cpp:795-797` | `kMemPerThread = sizeof(size_t)<<23` → 64 位 = **16MB/线程**，32 位 = 8MB | 把"线程数 × 常数"直接当内存预算 | 是上界估计，非精确 | 🔄待采用（我们的档位换算直接用这个数） |
| 4.10 | 线程数 = 内存 / 每线程开销（deflate 每线程 4MB） | `Archive/Zip/ZipUpdate.cpp:1160-1179` | deflate/PPMd 分支：`threadMemUsage = kMemPerThread + methodMemUsage(4MB)`，`numThreads = _memUsage_Compress / threadMemUsage` 再往下夹 | 内存自适应，不需要用户填参数 | 4MB 是硬编码 | 🔄待采用 |
| 4.11 | 线程数还要受"文件数 + fd 上限"夹 | `Archive/Zip/ZipUpdate.cpp:1024-1035`；`Windows/System.cpp:407-421` | `numZipThreads_limit = min(线程数, 待压文件数, Get_File_OPEN_MAX_Reduced_for_3_tasks())`；后者 = `(_SC_OPEN_MAX - 10) / 3` | 避免并发打开文件把 fd 用光（默认 1024 的 Linux 极易踩） | 只减不增 | 🔄待采用（**移动端尤其要紧**） |
| 4.12 | 线程数与块数匹配（BZip2/Xz） | `Archive/Zip/ZipUpdate.cpp:1110-1150` | BZip2 线程数不超过块数（`avgSize/blockSize+1`，上限 64）；Xz 由平均块数×LZMA 线程数决定 | 线程数超过块数纯属浪费 | 需先知道平均文件大小 | 🔄待采用 |
| 4.13 | 7z 头部也压缩 `kEncodedHeader` | `Archive/7z/7zOut.cpp:561-584,936-954` | 用同一套编码器把 header 流压成 `kEncodedHeader`，再写 NextHeader 指针 | 大量小文件时头部不膨胀 | 只有自研 7z 写端才需要 | ⚠️已评估-不采用（打包端只做 ZIP） |
| 4.14 | 给编码器"预期大小"提示 | `Archive/Zip/ZipAddCommon.cpp:400-437` | QI `ICompressSetCoderPropertiesOpt`，把 `kExpectedDataSize` 传下去 | 编码器可据此调参数（如分块） | 仅部分编码器支持 | 🔄待采用 |
| 4.15 | **资源档位：压缩 80% / 解压 53% 内存** | `Archive/Common/HandlerOut.h:35-55` | 取 RAM，`_memUsage_Compress = memAvail*80%`（`Calc_From_Val_Percents_Less100`），`_memUsage_Decompress = memAvail/32*17`（≈53%）；32 位夹到 `7<<28`；取不到 RAM 时退化为 `sizeof(size_t)<<28` | 一套默认档位适配 3 平台，无需用户调参 | 80%/53% 是经验值，需要实测校准 | 🔄待采用（`src/hardware.rs` 已有档位枚举，缺"按 RAM 自动定档 + 硬上限"） |
| 4.16 | `-mmt` 参数语义解析 | `Common/MethodProps.cpp:110-140`；`Archive/Common/HandlerOut.cpp:88-97` | `mt` 为空 → 取核数且标记"未强制"；`mt=false` → 1 线程并标记强制；也接受 BSTR 表达式 | "自动/强制"两态区分，全局默认不会覆盖用户显式设置 | 只在需要时生效 | 🔄待采用（`hardware.rs` 的档位注释已引用 `HandlerOut.h:29`，对齐即可） |
| 4.17 | 默认线程数 = 核数并有硬上限 | `Archive/Common/HandlerOut.h:29`；`Archive/Zip/ZipUpdate.cpp:1024-1035,1037-1041` | Linux 用 `GetNumberOfProcessors()`；再夹 `8<<(sizeof(size_t)/2)`（32 位 32 线程 / 64 位 128 线程） | 防止 192 核机器上开 192 线程把内存吃光 | 上限值偏保守 | 🔄待采用 |

---

## 五、修补（救援：坏目录表、坏头、部分成功）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| 5.1 | **LOCALS-CD-MODE 兜底触发** | `Archive/Zip/ZipIn.cpp:2530,2735-2742` | 第一遍走 CD-MODE；CD 读出来却与第一个 local item 不一致（`res != S_OK`）→ 清空条目、`ArcInfo.Base=0`、**顺序读全部 local header** | 完全不看中央目录也能重建目录树（救援模式现成方案） | 慢（O(条目数) 次 seek/读）；入口条件判断错会误入/漏入 | ✅已采用（`rescue.rs` 注释明确"核心思路综合自 LOCALS-CD-MODE，触发点 L2735"） |
| 5.2 | 顺序解析全部 local header `ReadLocals` | `Archive/Zip/ZipIn.cpp:2076-2143` | `while (_signature == kLocalFileHeader)`：读 local item，有描述符就 `FindDescriptor`，否则 `IncreaseRealPosition(PackSize)` 跳过压缩数据 | 每条的 packSize 自带 → 无需任何索引就能一路走到底 | `PackSize ≥ 1<<62` 判为越界抛异常；进度需节流 | ✅已采用（`rescue.rs:rescue_by_locals`） |
| 5.3 | 描述符定位的双重验证 | `Archive/Zip/ZipIn.cpp:1463-1574`（校验 `1508-1558`） | 找 `PK\x07\x08` 后必须满足 ①其后 4 字节是 `PK\x03\x04`/`PK\x01\x02` ②描述符内 packSize == 实际偏移；32/64 位两套 | 把"数据里恰好出现 PK\x07\x08"排除掉 | 不支持无签名描述符 | ✅已采用（`rescue.rs:find_descriptor`，注释写明对译 L1463 的"双重验证"） |
| 5.4 | 强制救援开关 `Force_ReadLocals_Mode` | `Archive/Zip/ZipIn.cpp:2558-2562`；`Archive/Zip/ZipIn.h:363` | 置位则直接 `res=S_FALSE` 跳进 LOCALS-CD-MODE，等价"永远不信 CD" | 自检/救援模式的干净入口 | 默认关闭，正常包会慢 | ✅已采用（我们的救援是独立模式 `ExtractMode::Rescue`，比 flag 更明确） |
| 5.5 | 从 CD/ECD64 反推 Base 并回改所有条目偏移 | `Archive/Zip/ZipIn.cpp:2992-3060`（`newBase = cdAbsOffset - cdInfo.Offset` @3020，回改 @3051-3059） | 用 ECD64 locator 或 ECD 的 `cdOffset` 反推 Base；算出后把**所有** `LocalHeaderPos` 统一平移 delta | 唯一能让"CD 偏移被篡改"的包正确定位的手段 | 反推后**必须验起点**，否则全线错位 | ✅已采用（`layout/zip.rs:locate` 的反推公式 + `verify_base` 校验 `PK\x03\x04`） |
| 5.6 | 32 位偏移溢出模式识别 `Overflow32bit` | `Archive/Zip/ZipIn.cpp:3033-3042`（置位 @3039）、`3128-3131` | 若"推算 Base 与实际 Base 的差"低 32 位为 0（差值非零）→ 判定包把 64 位偏移截成了 32 位，之后比对条目偏移时只比低 32 位 | 能救"老工具产出的大包偏移被截断"这一类 | 只在差值低位为 0 时置位，边界窄 | 🔄待采用（**我们的反推若遇 `verify_base` 失败，这就是下一个该试的假设**） |
| 5.7 | 条目数 16 位溢出标记 `Cd_NumEntries_Overflow_16bit` | `Archive/Zip/ZipIn.cpp:3203-3209` | CD 条目数与 ECD 声明数不等但**低 16 位相等** → 判为 16 位溢出，只警告不报错 | 老 7-Zip/批处理工具产的大包不会被判坏 | 需与"真不一致"区分 | 🔄待采用 |
| 5.8 | CD 与 Local 合并（以 Local 为准） | `Archive/Zip/ZipIn.cpp:3100-3176` | 逐条用 `LocalHeaderPos` 把 CD 条目与 local 条目配对；名字/尺寸/CRC 不一致 → 置 `HeadersError` **但不丢条目**；配不上的 CD 条目追加到末尾 | 属性取 CD、数据位置取 Local，两边好处都拿 | 合并策略错会丢文件（`items2` 兜底） | 🔄待采用（我们现在是"CD 路线"与"救援路线"二选一，缺合并） |
| 5.9 | ECD/CD 三方一致性校验 | `Archive/Zip/ZipIn.cpp:2969-2991,3178-3230,3224-3230` | 比对 `cdInfo.NumEntries/Size/Offset` vs 实际读到的条目数/CD 字节数/相对偏移；不一致置 `HeadersError`（不抛） | 能报"目录表被改过"而不是硬失败 | 只置标志，需上层消费 | 🔄待采用 |
| 5.10 | 段长（PhySize）靠"不清缓冲"算出来 | `Archive/Zip/ZipIn.cpp:3222`（注释原文：we can't clear buf varibles, we need them to calculate PhySize） | 解析结束时保留缓冲游标，据此得到归档物理长度 → 扫描层拿它当段长 | 精确段长 = 精确"丢壳取核" | 解析器状态与段长耦合，易被后续代码破坏 | 🔄待采用（我们现在靠"下一个签名"划界，属兜底，见 `scan.rs`） |
| 5.11 | CD 在但 local header 坏 → 逐条降级重读 | `Archive/Zip/ZipIn.cpp:1354-1430`（CRC/描述符位不一致仅置 `headersError` @1416-1425） | `Read_LocalItem_After_CdItem`：按 CD 的偏移去读 local header，签名不对/条目不等就返回 `S_FALSE`，由上层标 `kUnavailable`/`kHeadersError` | "90% 的条目能解 + 10% 明确标坏" | 每条多一次 seek | 🔄待采用 |
| 5.12 | 抽出时按 `IsLocalOffsetOK` 单条跳过 | `Archive/Zip/ZipIn.h:407,414,442`；`Archive/Zip/ZipHandler.cpp:1598-1616,1620-1638` | 位置不可信 → `kUnavailable`；`FromLocal` 为假先尝试 5.11 重读，仍失败则按 `isAvail` 区分 `kHeadersError`/`kUnavailable` | 报告能与"完整/残缺/失败"三态对齐 | 需要每条都跑一次可用性判断 | 🔄待采用 |
| 5.13 | `NoCentralDir` → `UnconfirmedStart`（告知而非报错） | `Archive/Zip/ZipIn.cpp:2827`；`Archive/Zip/ZipHandler.cpp:323`；`PropID.h:123-132` | 读不到 CD 时置 `NoCentralDir`，映射为 `kpv_ErrorFlags_UnconfirmedStart` 位 | "目录不可信"是**告知**不是致命错误 | 位定义需与 UI 约定 | 🔄待采用 |
| 5.14 | 错误标志位体系 | `PropID.h:123-132`；`Archive/Zip/ZipHandler.cpp:274-326` | `IsNotArc/HeadersError/UnavailableStart/UnconfirmedStart/UnexpectedEnd/UnsupportedMethod/DataError` 位掩码 + `kpidWarning`（`Overflow32bit`、16 位溢出）文本 + `kpidCharacts`（`Local/Central/Zip64/Unsorted_CD` 标签） | 一次返回"哪里可疑 + 为什么"，天然支撑白话报告 | 位定义一多就要文档 | 🔄待采用（`error.rs` 是枚举，可加一个 `flags` 位域/`Vec<Warning>`） |
| 5.15 | **7z 头部恢复：字段全 0 时从尾部反扫** | `Archive/7z/7zIn.cpp:1655-1692` | 若 `crcFromArc==0 && nextHeaderOffset==0 && nextHeaderSize==0 && nextHeaderCRC==0`：读末尾 512B，要求**末字节为 0**，从后往前找 `kEncodedHeader+kPackInfo` 或 `kHeader+kMainStreamsInfo` 边界 → 重算 `nextHeaderSize/Offset/CRC`，置 `StartHeaderWasRecovered` | 头 32 字节被清零的 7z 也能救回来（**7z 唯一的"修补"路径**） | 只在特定损坏形态生效；512B 窗口可能不够 | 🔄待采用（**高价值**：我们 7z 支持尚未落地，接入时必做） |
| 5.16 | 7z 签名 CRC 校验与恢复模式弱校验 | `Archive/7z/7zIn.cpp:323-341` | `FORMAT_7Z_RECOVERY` 下：CRC 对就算真；否则要求头 8..32 字节全 0 且 `p[6]/p[7]` 非 0（版本号非零）也算候选 | 把"被清零头"的 7z 与随机数据区分开 | 恢复模式的签名判定比正常模式弱 | 🔄待采用 |
| 5.17 | 7z 精确长度与意外结束标志 | `Archive/7z/7zIn.cpp:1720-1729` | `PhySize = kHeaderSize + nextHeaderOffset + nextHeaderSize`；`_fileEndPosition - StartPos < nextHeaderOffset+nextHeaderSize` → `UnexpectedEnd` 并返回失败 | 长度 bit-exact，段划分精确 | 依赖头字段可信 | 🔄待采用 |
| 5.18 | 解析异常转标志而非抛出 | `Archive/7z/7zIn.cpp:74-86,1785-1813` | `CInArchiveException` → `db.ThereIsHeaderError=true; return S_FALSE`；`CUnsupportedFeatureException` → `db.UnsupportedFeatureError`；`E_NOTIMPL` → 转不支持 | 上层永远能拿到"已解析的部分 + 原因" | 异常路径容易漏 | 🔄待采用（对应 Rust：`Result`，但要在错误里带 `partial: Vec<Entry>`） |
| 5.19 | 「头部有一个字节坏」的宽容点集合 | `Archive/Zip/ZipIn.cpp:1219-1245`（extra 解析失败忽略、时间不合法只警告、名字有尾零只警告） | 明确列出可容忍项：extra 解析失败**忽略**、DOS 时间不合法**警告**、名字长度不符但内容 OK **警告** | 实用主义：坏一个字节不该整包失败 | 宽容度不当会放过真损坏 | ✅已采用（`rescue.rs` 的 `is_strange`/`headersError` 思路一致） |
| 5.20 | RAR 恢复记录识别 | `Archive/Rar/Rar5Handler.cpp:758-768,1483-1486`；`Rar5Handler.h:375,378` | 读 Locator 的 Recovery 标志与大小（`Recovery` 额外字段） | 能告知"包里有恢复记录，值得用 RAR 工具修" | 7-Zip 并不据此修复，只报告 | ⚠️已评估-不采用（RAR 算法专有、我们只解不压；且不做 RAR 内部解析） |
| 5.21 | APK 签名块识别（CD 前的特殊块） | `Archive/Zip/ZipIn.cpp:2804-2823` | 读 local headers 走完若签名不是 CD，检查是不是 `APK Sig Block 42`（16B 魔数 + 长度自洽）→ 是则 `IsApk=true` 不算错 | 防"APK 被判 `NoCentralDir`" | 与伪装包场景无关 | ⚠️已评估-不采用（我们的输入是伪装 MP4/图片，不处理 APK） |

---

## 六、Top 5 最值得采用

1. **5.15 + 5.16 — 7z 头部恢复 + 签名 CRC 双重校验**（`Archive/7z/7zIn.cpp:1655-1692`、`323-341`）
   我们现在**完全没有 7z 支持**；这两条决定"内嵌 7z 的伪装包能不能救"，且 CRC 双校验直接把扫描误报率降到 ≈2⁻³²。
2. **3.11 + 4.9/4.10 + 4.15 — 内存驱动的线程档位**（`Compress/Lzma2Decoder.cpp:110-129`、`Archive/Zip/ZipUpdate.cpp:795-797,1160-1179`、`Archive/Common/HandlerOut.h:35-55`）
   `src/hardware.rs` 现在只有档位**枚举**；补上"RAM → 内存预算 → 线程数"的换算公式，才真正满足"不想让小白调参数"的目标。
3. **3.4/3.5/3.6 — CRC + 解开尺寸双闸 + 结果分级**（`Archive/Zip/ZipHandler.cpp:1246-1251,1499-1546`；`Archive/IArchive.h:136-145`）
   救援模式的死穴是"看起来成功其实缺数据"。双闸 + 10 档结果，才能给出"351 个文件 / 347 完整 ✓ / 4 损坏"这种可信报告。
4. **5.11 + 5.12 + 3.7 — CD 在但 local 头坏 → 逐条降级**（`Archive/Zip/ZipIn.cpp:1354-1430`；`Archive/Zip/ZipHandler.cpp:1598-1638`）
   把"整包失败"变成"90% 解出 + 10% 明确标坏"，与我们"部分成功优于整体失败"的产品定位完全一致。
5. **5.6/5.7 — `Overflow32bit` 与 16 位条目数溢出两个已知坏包模式**（`Archive/Zip/ZipIn.cpp:3039,3208`）
   我们 `layout/zip.rs` 的 `verify_base` 一旦失败目前就直接报错；这两条是"反推失败后的下一批假设"，成本极低、覆盖面明确。

> 反过来，**明确不做**的三类：自建 MT 解码框架（3.13）、自研 7z coder mixer（3.14）、GUI/Windows 专属机制（2.17、5.20、5.21）——理由都写在各行「代价/风险」里。

---

*生成：基于 `/home/user/hermes/yshjy/refs/7zip`（tag 26.04 / `9128b80`）逐文件 grep + 阅读；所有行号已用 `grep -n` / `sed -n` 复核。*
