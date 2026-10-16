# XADMaster 五环节施工清单

**参考库**：`refs/XADMaster/`（MacPaw Way 版 The Unarchiver 引擎，538 个源文件 / 约 8.1 万行 `.m+.h`；51 个解析器）
**读取范围**：`XADArchiveParser`、`XADZipParser`、`XADArchive`、`CSHandle/CSStreamHandle/CSSubHandle/CSFileHandle/CSSegmentedHandle/CSMultiFileHandle/CSInputBuffer`、`XADSkipHandle`、`XADPath`、`XADString`、`XADUnarchiver`、`Scanning`、`XADCRCHandle`、`XADCRCSuffixHandle` + 各 SFX/嗅探器（ZipSFX / EmbeddedRAR / RAR5 / 7zSFX / GzipSFX / CABSFX / ARCSFX / NSIS / ISO9660 / SplitFile / ALZip / StuffItSplit）
**不读**：Cocoa GUI（`Extra/`、`English.lproj/`、`XADMaster.xcodeproj/`）、`lsar.m`/`unar.m`/`CommandLineCommon`（CLI 外壳）、各解码算法内部（BWT/LZSS/LZW/CRC 表）
**决定取值**：`✅已采用` / `🔄待采用` / `⚠️已评估-不采用(附原因)` / `❓未判定`

---

## 0. 命名纠正（任务书点名的文件在库里不存在）

| 任务书写的 | 实际文件 | 说明 |
|---|---|---|
| `XADPrefixHandle.m` | **不存在** | 前置数据/偏移由「属性袋传偏移 + `-parse` 开头 seek」实现，不是 handle 层（见 ①-05、②-03） |
| `XADSplitFileHandle.m` | **不存在**；只有 `XADSplitFileParser.m` | 分卷拼接靠 `CSSegmentedHandle`+`CSMultiFileHandle`（①-07） |
| `CSBoundedHandle` | **不存在** | 等价物是 `CSSubHandle`（起止裁剪，①-06） |
| `CSSeekableHandle` | **不存在** | 等价物是 `CSStreamHandle`（流式 + 有限 seek，①-04） |
| `XADSeekableHandle` | — | 同上 |
| `XADArchive.m`「存档管理」 | 存在（1239 行） | 真正的存档门面 + **同名条目合并**（③-14） |

---

## ① 读取（文件定位 / 缓冲 / 分卷 / 可中断）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| ①-01 | handle 是唯一 I/O 抽象，自身可复制 | `CSHandle.m:64-77`、`CSHandle.m:539-542` | `copyWithZone` 走 `initAsCopyOf:`，只复制**位读状态**（parent 引用共享 + bitoffs/readbitsleft），不复制 fd。 | 子流可廉价分叉，解码器互不干扰。 | 共享 parent 位置，需靠 `multilock` 或子流各自 seek 兜底。 | 🔄待采用 |
| ①-02 | 尾部窗口分块倒扫 + 20B 重叠 | `XADZipParser.m:118-191`（图解注释 `:176-183`） | 64 KiB 一块从尾往前扫；每块保留 20 字节重叠，因 Zip64 locator 恰为 20 字节，卡块边界会被漏；`pos>=preservedBytes` 保证 4 字节签名完整。 | 不信任「EOCD 必须在末尾 65557 字节」这一惯例，EOCD 被推远也能找到。 | 极端情况全文件读一遍（源码自己吐槽 "1MB in memory is not always a good idea"）。 | 🔄待采用 |
| ①-03 | 无 EOCD 不放弃，直接降级 | `XADZipParser.m:194-233` | `centraloffs==-1` → `parseWithoutCentralDirectory`；文件 >4 GiB 且无 Zip64 locator → 也降级（「某些天才写了 64 位文件却没写 64 位扩展」）。 | 损坏包仍有救；4 GiB 边界不会误读中央目录。 | 线性扫描 O(n)，大包慢。 | 🔄待采用 |
| ①-04 | 流式 handle + 可推回一个字节 | `CSStreamHandle.m:45-93`（默认 4 KiB 输入缓冲 `:67`）、`CSStreamHandle.m:156-181` | 解码器基类：`streamAtMost:` 由子类实现，基类管 `nextstreambyte` 推回、`seek` 只允许前进，回退必须 `resetStream` 从头重来。 | 统一解压器写法；明确「不可回退」避免半途状态错乱。 | 回退代价=整条流重解（对 solid 包尤其痛）。 | 🔄待采用 |
| ①-05 | 偏移契约：属性袋传「内层在哪」 | `XADArchiveParser.m:143-144`、`XAD7ZipParser.m:1046+1055`、`XADRARParser.m:844+884`、`XADGzipParser.m:154+164`、`XADCABParser.m:387+399` | 识别期把偏移写进 dict（`XADFoundSignatureOffset` 或私有 `7zSFXOffset`/`RAREmbedOffset`/`GzipSFXOffset`/`CABSFXOffset`/`NSISOlderOffset`…），解析期 `-parse` 开头 `seek(offs)`；**偏移只表示发现位置，字节流本身不变**。 | 识别器与解析器解耦，一个新 SFX 布局只改两行。 | 字符串键 + 弱类型，拼错键静默失效。 | ✅已采用（`src/io/window.rs`：Window 把 base 之后映射为逻辑 0 起点） |
| ①-06 | 子流裁剪（copied / nonCopied 两版） | `CSHandle.m:345-393`、`CSSubHandle.m:25-103` | `subHandleFrom:length:` 复制 parent 状态；`nonCopiedSubHandleFrom:length:` 直接用 self，省一次复制；子流对外表现为定长文件。 | 「条目 = 主流的 [start,start+len) 切片」零拷贝表达。 | `seekToFileOffset` 越界直接抛 EOF 异常。 | 🔄待采用 |
| ①-07 | 多卷=分段虚接，段表预先展开 | `CSSegmentedHandle.m:175-209`、`:136-150`、`CSMultiFileHandle.m:78-102` | `_open` 一次算出各段累计终点数组，`segmentSizeAtIndex` 用 `stat`；读跨段时自动切到下一段并 `seek(0)`。 | 上层解析器看到的是一条连续文件，分卷对它透明。 | 段大小必须可 stat（末段可未知）；跨段 seek 走线性查终点。 | ✅已采用（`src/io/segment.rs`：段索引表 + 二分定位 + 顺序读缓存） |
| ①-08 | 卷扫描 = 主动列目录 + 正则 + 排序 | `XADArchiveParser.m:842-890` | 用文件名正则匹配同目录文件，`XADVolumeSort` 让 `firstext`（如 `z01`）排最前，其余按名字不区分大小写排序。 | 用户只给一个卷，也能自己找齐；顺序确定可复现。 | 必须能列目录（流输入不可用）；密集数字命名需数字排序（源码把 NSNumericSearch 注释掉了）。 | 🔄待采用 |
| ①-09 | 「洞」模型：把不连续字节伪装成连续流 | `XADSkipHandle.m:142-166`（区间表）、`:54-108`（二分 + 双向映射）、`:204-228`（读跨越洞自动跳） | 登记 `addSkipFrom:to:`，之后 `skipOffset↔actualOffset` 双向换算，读时遇洞直接 seek 到洞后。 | 分卷头部、条目间垃圾、包装头都能「删掉」，上层代码零改动。 | 区间不可重叠/相邻（直接抛异常，源码留 TODO）；维护区间表出错即错位。 | ✅已采用（段模型等价：`src/io/segment.rs`；「挖洞」表达法未单列 → 若需删字节再补） |
| ①-10 | 「跳过的字节也是数据来源」两个键 | `XADArchiveParser.m:905-924`、`:926-930` | 有 `XADSkipOffsetKey` 就走 `XADSkipHandle` 无洞流（`XADSkipLengthKey` 再裁尾），否则走 `XADDataOffsetKey` 直切原流；skipHandle 懒创建。 | 同一 API 同时支持「跳过坏区」与「直切数据区」。 | 两条路径语义不同，混用会静默读错位置（我们需显式类型区分）。 | 🔄待采用 |
| ①-11 | 条目内嵌前缀跳过（真实用法） | `XADALZipParser.m:87-97`、`:157`；`XADStuffItSplitParser.m:106-113`、`:153`、`:174` | ALZip 分卷每个卷头 16 字节+尾 8 字节被 `addSkipFrom` 挖掉；StuffItSplit 每 100 字节头也挖掉；之后条目记 `XADSkipOffsetKey` 指到洞后。 | 「档案内部条目之间还夹着垃圾」有正式表达。 | 跳过量必须精确，错一字节全盘错位。 | 🔄待采用 |
| ①-12 | 一次性读满 maxheader 分发给所有候选 | `XADArchiveParser.m:252-257`、`:356-358` | 初始化时取所有解析器 `requiredHeaderSize` 的最大值（RAR embedded = 0x80000），入口一次 `readDataOfLengthAtMost:maxheader`。 | 识别期零额外 I/O，各解析器拿同一块字节各自扫。 | 512 KiB 常驻；巨型 SFX 可能超出。 | 🔄待采用 |
| ①-13 | 缓冲填充保留尾部重叠 | `CSInputBuffer.m:161-183` | 缓冲耗尽时 `memmove` 保留未消费尾部，再从 parent 补齐；读多一个字节即预读。 | 位读与字节读共用一套缓冲语义。 | 固定 4 KiB 默认值偏小（大顺序读需另调）。 | 🔄待采用 |
| ①-14 | 顺序丢弃固定 16 KiB 块 | `CSHandle.m:324-341` | `readAndDiscardAtMost:` 用栈上 16 KiB 缓冲循环丢弃，避免 seek 大跳。 | 对不可 seek 流也成立；跳大距离时省 syscall。 | 拷贝开销（实测只在跳超大距离时才划算）。 | ⚠️已评估-不采用（我们已有 `RescueOptions.seek_threshold` 按大小分流 seek/读，见 tech/README「已进代码」） |
| ①-15 | 可中断：解析层与解压层各自轮询 | `XADArchiveParser.m:894-901`、`XADUnarchiver.m:743-749`、`:652-654` | 每条目 / 每块前问一次 delegate「要不要停」，置位后循环退出。 | 取消是协作式的，随时可停且状态一致。 | 逐条目检查，单条目超长时无细粒度控制（对比我们 256 KiB 粒度）。 | ✅已采用（`src/io/interrupt.rs`） |
| ①-16 | 失败即删输出文件 | `XADUnarchiver.m:415-419` | 解压出错（含用户取消）就把目标文件删掉，不留下半截文件。 | 不留毒文件，用户重试无歧义。 | 已覆盖同名旧文件时有数据丢失风险（需先写临时名）。 | 🔄待采用 |
| ①-17 | 进度节流上报 | `XADUnarchiver.m:668-680` | 按时间（`updateinterval`）而非按字节上报，且未知名大小时改用 handle 自估进度。 | 回调开销与速率无关。 | 需要 handle 提供 `estimatedProgress`。 | 🔄待采用 |
| ①-18 | 底层就是 stdio + 每 copy 串行锁 | `CSFileHandle.m:194-202`、`:158-162`、`:145-156` | `fread/fseeko/ftello`；多个 handle 共享同一 `FILE*` 时用 `NSLock` 把「seek+read」串起来。 | 实现极简，几十行跑通全部格式。 | 共享 fd + 全局锁，无并行、无 mmap；`feof` 语义被显式绕开（注释在 `:164-169`）。 | ⚠️已评估-不采用（Rust 侧用独立 `File`/`pread` 更安全；共享句柄+锁是 C 的历史包袱） |

---

## ② 判断（格式嗅探 / 签名 / 结构校验 / 编码）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| ②-01 | 有序注册表，**顺序即优先级** | `XADArchiveParser.m:156-258` | 一个静态数组列 51 个解析器类，注释分组：Common → Mac → Less common → **Detectors that require lots of work** → **Over-eager detectors** → LibXAD。 | 「贪心的、弱的放最后」被写成显式注释，新人不会乱插。 | 顺序是隐式契约，靠人工维护。 | 🔄待采用 |
| ②-02 | 主循环：每个候选都从 seek(0) 起 | `XADArchiveParser.m:260-295` | 轮询每个类，每次都 `seekToFileOffset:0` + 清空 props，命中即 `new` 实例并回填属性返回。 | 识别器可以任意移动流位置，不需要自我复位。 | 每个候选一次 seek（本地无成本）。 | ✅已采用（`src/scan.rs::scan_whole_file` 单遍扫描 + `SigIndex` 两字节索引，效果等同、更快） |
| ②-03 | **浮动签名仲裁：最小偏移胜** | `XADArchiveParser.m:297-327`、`:146-152`、`:269-286` | 只在 ZipSFX / EmbeddedRAR / EmbeddedRAR5 三个类上启用；三者都跑，各自报 `XADFoundSignatureOffset`，缺省当 0x100000，按偏移升序取第一个。 | 「一个文件里出现多种格式签名」有了确定且可解释的裁决规则（内层更靠前者胜）。 | 只覆盖三类；同偏移时顺序决定（未定义平局规则）。 | 🔄待采用 |
| ②-04 | 异常吞噬式识别 | `XADArchiveParser.m:329-339` | `@try` 包住 `recognizeFile...`，任何异常一律当「不是这种格式」。 | 允许写贪心而脆弱的探测逻辑，靠候选表兜底。 | 真错误被吞掉，只能靠 `reportInterestingFile` 埋点观测。 | 🔄待采用 |
| ②-05 | ZIP 三条 magic（含偏移 4 的怪例） | `XADZipParser.m:44-58` | 认 `PK\x03\x04`@0、`PK\x05\x06`@0、**`PK\x03\x04`@4**。 | 覆盖「前面有 4 字节垃圾/长度前缀」的常见畸形包。 | 偏移 4 这条会略微增加误判面。 | 🔄待采用 |
| ②-06 | RAR 双签名 + 只认偏移 0 | `XADRARParser.m:94-107`、`:117-128` | 新签名 `Rar!\x1a\x07\x00`、远古签名 `52 45 7E 5E`；识别只看偏移 0，偏移任意由 `XADEmbeddedRARParser` 负责。 | 职责分离：普通 RAR 快判，SFX 交给重探测类。 | 远古签名 4 字节较弱。 | 🔄待采用 |
| ②-07 | RAR5 前 1 MiB 全扫 | `XADRAR5Parser.m:77-99`、`:83`（`signatureLocationInData:`） | 在 header 缓冲里线性找 8 字节签名，记下位置；1 MiB 常量集中一处（`RAR5MaximumSFXHeader`）。 | 对「RAR5 被塞在任意位置」一次搞定。 | 需要 1 MiB header 预算。 | 🔄待采用 |
| ②-08 | SFX 探测：宿主特征 + 内层签名 + 合理性 | `XADZipSFXParsers.m:26-47`、`XAD7ZipParser.m:1032-1051`、`XADGzipParser.m:135-160`、`XADCABParser.m:375-395`、`XADARCParser.m:345-399` | 先要求宿主 magic（`MZ` / `#!`），再扫内层签名；7z/NSIS 按 **512 字节对齐步长**扫（页对齐假设）；ZipSFX 还校验版本 `[10,40)` 且 flags 高字节为 0。 | 误报率低（宿主特征难伪造），且能覆盖 SFX 常见页对齐布局。 | 非页对齐的 SFX 会被 512 步长漏掉。 | 🔄待采用 |
| ②-09 | Info-ZIP Unicode Path (0x7075) **先验 CRC 再采用** | `XADZipParser.m:859-888` | 读 version、CRC32、UTF-8 名；用 **CRC32(原始 name 字节)** 与字段里记录的 CRC 比对，一致才覆盖文件名，并把原名存 `ZipRegularFilename`；顺带剥掉尾部填充的 NUL。 | 有校验的编码修正，不会因半截 extra 字段把好名字改坏。 | 依赖打包器正确写 CRC（Info-ZIP 系才写）。 | 🔄待采用 |
| ②-10 | 文件名编码三级判定 | `XADZipParser.m:1013-1016`、`:1000-1011` | flags&0x800 → 强制 UTF-8；否则走 `XADPathWithData:separators:`（自动探测）；system==0(MS-DOS) 时允许 `\` 也当分隔符（IZArc 的兼容坑）。 | 三种来源都有明确分支，Mac/Win/DOS 包都能出正确路径。 | 无 UTF-8 位且探测器猜错时名字仍会错（需上层人工改名）。 | ✅已采用（UTF-8 优先 + GBK 回退：`src/layout/zip.rs:408-418`；探测式自动判定未做 → 🔄） |
| ②-11 | 编码探测器 + ASCII 快路 + Mac 偏好 | `XADString.m:81-93`、`:342-395`、`:449-455` | 全 ASCII 直接按 ASCII 定案（省一次探测）；否则 `UniversalDetector` 出 MIME charset，失败回落 windows-1252；归档被标为 Mac 归档时把 cp1252/Shift_JIS 换 Mac 编码。 | 快、稳、可解释；Mac 老包不出现乱码。 | 探测器按「整包累积」而非逐名判断；置信度低时无二次确认。 | 🔄待采用 |
| ②-12 | 固定编码覆盖开关 | `XADArchiveParser.m:685-692`、`:992`、`XADString.m:410-424` | 允许外部强制指定编码（`setFixedEncodingName`），被标 Mac 归档时置 `prefersMacEncodings`。 | 用户可手工纠正乱码，不用改包。 | 全局生效，混合编码包无解。 | 🔄待采用 |
| ②-13 | 结构参数反推（无 magic 的格式） | `XADLZMAAloneParser.m:28-46` | 源码吐槽「Geez, put some magic bytes in your file formats, people!」后：设置字节 `>=9*5*5` 拒、字典大小必须是 2 的幂或 3×2^n（乘性逆元 `*0xaaaaaaab` 判 3 的倍数）、尺寸全 0 拒。 | 无签名格式也能高置信度识别。 | 纯启发式，恒有误报空间。 | 🔄待采用 |
| ②-14 | 用**已有校验和**做二次确认 | `XADNowCompressParser.m:35-60`（区块和 == BE32）、`XADMacArchiveParser.m:34-76`（CRC16）、`XADISO9660Parser.m:30-60`（两段 CRC16） | 先用字段约束（计数范围、名字全可打印）过滤，再用包里自带的 CRC/和值做最终确认。 | 把「看起来像」提升到「算得过就算是」，误报率极低。 | 只有自带校验和的格式可用。 | 🔄待采用 |
| ②-15 | 两级怀疑：便宜线索 → 昂贵验证 | `XADZipParser.m:1036-1041`（`.bin` → `XADMightBeMacBinaryKey`）、`XADMacArchiveParser.m:379`、`:360-430` | 解析期发现 `*.bin` 只打个「可能是 MacBinary」标记；真正拆数据/资源叉时再用 `macBinaryVersionForHeader:` 四级判定确认。 | 识别期不做昂贵判断，解析期按需付费。 | 标记需在条目属性间传递（弱类型）。 | 🔄待采用 |
| ②-16 | ZIP 中央目录：local header 是名字的真源 | `XADZipParser.m:400`（`skipBytes:cdr.namelength`）、`:313-363`、`:332-333` | 读中央目录记录时**跳过**名字，只取 extra（找 Zip64）；随后 seek 到 local header，用 **local 里的名字**建条目；local 签名不是 `PK\x03\x04/\x05\x06` 就只标 `XADIsCorruptedKey` 不中断。 | CDR 与 local 不一致（改过名/修补过）时以 local 为准，更接近真实数据。 | 多一次 seek/读；两处名字不一致时需上层提示。 | 🔄待采用 |
| ②-17 | 条目数 65535 溢出补丁 | `XADZipParser.m:303-310` | 读完 `numentries` 后，若 `centraloffset+centralsize-next > 65536*46`，判定「没用 Zip64 就写了超 65535 条目」，把 `numentries+=65536` 继续读（可反复触发）。 | 拿到完整列表，而不是只列前 65535 个。 | 46 是魔数，非标准做法。 | 🔄待采用 |
| ②-18 | 「可疑/异常」上报钩子 | `XADArchiveParser.m:1254-1263` + 调用点 `XADZipParser.m:992-993`、`:1248-1250` | `reportInterestingFileWithReason:` 在遇到不支持的方法/罕见特性时打一条结构化报告（Reduce 压缩、未支持方法号…）。 | 无需崩溃即可收集「现实世界的边角」，天然 fuzz 语料生成器。 | 只上报不处理。 | 🔄待采用 |
| ②-19 | 只看文件名的识别器（有意为之） | `XADNSAParser.m:34-43`、`XADSARParser.m:28-37`、`XADTarParser.m:77-84` | Nsa/Sar 仅凭 `^arc[0-9]*\.(nsa\|sar)$` 返回 YES，内容字节完全不看（源码把字节检查注释掉）；TAR 名字匹配 `.tar` **或** 512 字节头非 V7。 | 覆盖无签名的老格式；代价被接受。 | 与「只看内容不看名字」相反，会误判同名文件。 | ⚠️已评估-不采用（违背我方原则：DESIGN.md 16.1「只看内容，不看名字」；且会污染假签名判定） |
| ②-20 | 弱格式放最后 + 长度下限设防 | `XADXZParser.m:28-37`、`XADBzip2Parser.m:28-44`、`XADStuffItSplitParser.m:30-44` | XZ 只认 6 字节 `FD 37 7A 58 5A 00`（旧 LZMA magic 分支被注释掉留存）；Bzip2 认 `BZh[1-9]` **且** 紧随的块 magic 命中两个已知值之一；StuffItSplit 要求三字节头 + 部件数 ∈[1,63] + 名字非 0 字节。 | 短签名格式靠「第二组字节」把误报压下来。 | 每种格式都要手工挑第二判据。 | 🔄待采用 |
| ②-21 | 文件存在性作为识别阳性条件 | `XADSplitFileParser.m:30-48` | 名字形如 `.001` 时，检查 `.001`/`.002` 中另一个**确实存在**才认。 | 单文件 `.001` 不会被当成裸分卷；与 ①-08 卷扫描天然衔接。 | 需文件系统可访问（流输入不可用）。 | 🔄待采用 |
| ②-22 | 格式「再套一层」标记 | `XADGzipParser.m:106`、`XADBzip2Parser.m:61`、`XADSplitFileParser.m:75-77`、`XADArchive.m:593-599`、`XADArchive.h:127` | 解包器解析完后在条目上置 `XADIsArchiveKey`（.gz/.bz2/裸 .zip 分卷等都置）；`entryIsArchive:` 供上层决定是否下钻。 | 嗅探结果可传递，递归下钻有明确依据。 | 只是提示，真假仍要靠实际解析（见 ③-08 兜底）。 | 🔄待采用 |
| ②-23 | 路径安全化（防穿越） | `XADPath.m:439-476`、`XADPath.m:284-330`（canonical 折叠） | 先折叠 `.`/`..`（中间 `..` 被消掉），去掉开头 `/`，残余 `..` 改名 `__Parent__`，再逐段过 `XADPlatform sanitizedPathComponent:`。 | 恶意条目名无法逃出目标目录，且不同平台一致。 | 改名会与真实文件名冲突（需去重策略）。 | ✅已采用（`src/safety.rs::resolve_safe`：可疑输入直接报错、不「尽力而为」） |
| ②-24 | 归档级脏标记而非失败 | `XADArchiveParser.m:1012-1017`、`XADArchive.m:404-409` | 任一子项带 `XADIsCorruptedKey`（或加密标记）就提升为归档级属性；`isCorrupted` 只是查询，不抛错。 | 「能读出来但已知有问题」有正式表达，上层可继续并提示。 | 需要上层主动查，否则静默。 | 🔄待采用 |

---

## ③ 解压（解码器 / 密码 / 分卷 / 嵌套 / 选择性提取）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| ③-01 | 解码器 = 流包装器（handle 链） | `CSStreamHandle.m:45-93`、`XADZipParser.m:1192-1210` | 每个条目返回的 handle 是一串包装：`源 → 解密 → 解压 → CRC 校验`；上层只 `readAtMost`。 | 「加密/压缩/校验」三种正交能力自由组合，解压循环只有一份。 | 每层一次函数调用+缓冲拷贝（实测开销可接受）。 | 🔄待采用 |
| ③-02 | 压缩方法 → handle 路由表 | `XADZipParser.m:1212-1252` | 一个 `switch(method)` 把 0/1/6/8/9/12/14/96/97/98 映射到具体 handle；未知方法**只上报不上抛**，返回 nil 让上层当「不支持」。 | 加一种压缩法只改一处；不支持也有优雅出口。 | 方法号是硬编码表，需随规范更新。 | 🔄待采用 |
| ③-03 | Deflate 故意不传长度 | `XADZipParser.m:1220-1225` | `case 8` 用 `deflateHandleWithHandle:`（不带 size），注释说明：>4 GiB 的包很多没写 Zip64 扩展，size 完全可能是错的，只能一路解到底。 | 面对错误长度仍能解出正确数据。 | 失去提前截断保护，解压炸弹更难拦（必须靠我们的 Limits 兜）。 | 🔄待采用 |
| ③-04 | 尺寸缺失时的「描述符搜索」解压路径 | `XADZipParser.m:515-524`、`:656-682` | flags&0x08（未记录尺寸/CRC）时，用匹配函数在数据流里搜 12/20 或 16/24 字节的数据描述符，命中后回填 crc/compsize/uncompsize。 | 无中央目录的包也能定出每条边界。 | 扫描需先验签（见 ⑤-02），否则数据里偶合会误判。 | ✅已采用（`src/rescue.rs::find_descriptor`：描述符双重验证） |
| ③-05 | 加密分两族、统一入口 | `XADZipParser.m:1141-1210`、`XADWinZipAESHandle.m:24-30`、`XADZipCryptHandle.m:25-45`、`XADRARAESHandle.m:26-98` | method==99 → WinZipAES（校验 version∈{1,2}、vendor==0x4541 'AE'、keySize∈[1,3]→16/24/32 字节）；否则 ZipCrypto，用 `ZipLocalDate` 或 CRC 高字节做 testByte 验密码。 | 密码对错在**读第一个字节**时就知道，不用解完。 | testByte 天然有 1/256 假阳性。 | 🔄待采用 |
| ③-06 | 密码「拉取式」，无爆破 | `XADArchiveParser.m:647-708`、`:1235-1250`、`XADUnarchiver.m:134-137` | parser 只持有密码与密码编码名；需要时回调 delegate 索取；`encodedPassword` 按 parser 的字符串源编码转字节。 | 密码生命周期清晰，绝不偷偷试密码。 | delegate 不支持密码时直接失败。 | 🔄待采用 |
| ③-07 | 逐条目 CRC 校验（可选、惰性） | `XADZipParser.m:1195-1207`、`XADCRCHandle.h:36-56`、`CSHandle.m:821-835` | `wantChecksum` 决定是否套 `XADCRCHandle`；`hasChecksum`/`isChecksumCorrect` 惰性计算，只在读到流末尾时判对错。 | 不解压也能验 CRC（`testChecksum`）。 | 惰性判定要求读完整流。 | ✅已采用（CRC32 已实现 `src/scan.rs:510`；「校验作为 handle 层惰性判定」未做 → 🔄） |
| ③-08 | 解压失败兜底：退回当普通文件 | `XADArchive.m:340-357`（子归档解压失败 → `extractEntry:dataFork:resourceFork:`）、`:301-307` | 被标记为「内层归档」的条目若实际打不开，则按普通文件解出来。 | 误判 `XADIsArchiveKey` 时不会丢数据。 | 需要区分「格式错」与「真损坏」两类错误码。 | 🔄待采用 |
| ③-09 | 单条目自动递归下钻 | `XADArchive.m:45-61`、`XADArchiveParser.m:470-520` | `recursiveArchiveForFile:` 循环：只要当前包**只有 1 个条目**就把它当包继续开，直到不是单条目或失败。 | 「包中包中包」一次调用到底。 | 无深度上限（我们需加 max_depth 与炸弹检测）。 | 🔄待采用 |
| ③-10 | 嵌套子归档继承父级密码与编码 | `XADArchiveParser.m:483-512` | 从条目建子解析器时，把父 parser 的 password / passwordencodingname / stringsource 传给子 parser。 | 内层加密包不用二次输入密码。 | 编码继承可能对内层包不适用。 | 🔄待采用 |
| ③-11 | 目录/软链接延迟创建（两阶段收尾） | `XADUnarchiver.m:319-378`、`:537-553` | 目录与链接先入 deferred 队列，解压全部结束后按「深度降序、数据叉先于资源叉」统一创建并补属性。 | 即使包内目录条目在后、权限只写在目录上，结果也正确。 | 需要两阶段且中途失败要能回滚。 | 🔄待采用 |
| ③-12 | 属性回填集中一处 | `XADUnarchiver.m:537-560`、`XADZipParser.m:1057-1101` | 权限来源优先级：Unix system==3 → `extfileattrib>>16`；MS-DOS → 同样取高 16 位**且要求 `perm >= S_IFIFO`** 才认；都没有时（仅 macOS）按 umask 造 0777&~mask。 | 「权限字段不可信」被显式处理，避免解出 000 权限的坑。 | 平台分支多；Win 包权限靠猜。 | 🔄待采用 |
| ③-13 | 选择性提取：列完再挑 | `XADArchiveParser.m:1002-1017`、`XADUnarchiver.m:147-291` | 解析期逐条回调 `foundEntryWithDictionary:`，可置 `shouldstop` 提前停止解析；解压期由 delegate 决定每条要不要、落到哪个路径。 | 「像下载一样先列后挑」天然支持（我们的 💎 功能）。 | 停止后无法继续解析（需重新打开）。 | 🔄待采用 |
| ③-14 | 同名条目合并（数据叉 + 资源叉） | `XADArchive.m:257-335` | 用名字做 key 存 index；同名的资源叉条目去匹配已有数据叉，反之亦然，合并成一条。 | 一个文件的多个分片在包里出现多次也能合一。 | 仅处理「两个叉」，多于两片无法合并。 | 🔄待采用 |
| ③-15 | 输出循环：固定块 + 边解边写 | `XADUnarchiver.m:645-713`（`bufsize=0x40000` 在 `:648`） | 申请 256 KiB 缓冲，循环 `readAtMost → 输出函数`，无数据即结束；结束后统一验 CRC 或比长度。 | 内存恒定，与条目大小无关。 | 校验在末尾，错误要等解完。 | ✅已采用（分块解压 + 可中断思路一致：`src/io/interrupt.rs`；精确块大小待对齐 → 保留 🔄） |
| ③-16 | 不可 seek 流一律按 solid 处理 | `XADArchiveParser.m:939-957`、`:962-983` | solid 条目用 `currsolidhandle` + `nonCopiedSubHandleFrom:start length:` 从同一条解压流里切片；solid 对象变化时才重开流。 | 顺带支持了「必须从头解」的 solid 包。 | solid 坏一个文件后面全废（见 ⑤-08）。 | ⚠️已评估-不采用（依赖 RAR 解码器，我方 ③「全部缺失」，先做非 solid；先在文档留坑位） |

---

## ④ 压缩（打包 / 伪装布局 / 分卷 / 加密）

> **结论先行**：XADMaster 是**纯解压库**，没有任何归档创建代码。全库仅 `XADPNGWriter`（给 NDS/SWF 解析器输出 PNG 用，`XADNDSParser.m:297`、`XADSWFParser.m:564`）涉及「写」。所以本节只有「可反向利用」的 9 条。

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| ④-01 | 全库无压缩器（反面证据） | 全库 grep：仅 `XADPNGWriter.m:25` 有 writer 语义 | 没有任何 `createArchive` / deflate 编码路径；写侧只有 `CSHandle.writeBytes:`（`CSHandle.m:105`、`:398-423`）与 PNG 输出。 | 明确边界：④ 的算法/布局必须自己定或另找参考库。 | 别指望从 XADMaster 抄到打包器。 | ⚠️已评估-不采用（无内容可抄；作为「不要把工时投在这份参考的 ④ 上」的证据） |
| ④-02 | 分卷命名方案全集（写侧规范来源） | `XADZipParser.m:60-97`、`XADZipSFXParsers.m:137-154`、`XAD7ZipParser.m:106-119`、`XADRARParser.m:852-880`、`XADSplitFileParser.m:46-62` | 读侧实际要认的命名：`.z01/.z02…+ .zip`、`.zip.001`、`.1.zip`/`.2.zip`、`.7z.001`、RAR 新式「保留原数字位数」（`^prefix[0-9]{N}suffix\.(rar\|exe\|sfx)$`）、裸 `.001`。 | 我们生成分卷时只要落在这些模式里，竞品全都能接着解。 | 命名即兼容性契约，改一处要全量回归。 | 🔄待采用 |
| ④-03 | 分卷排序的第一卷特权 | `XADArchiveParser.m:842-854` | `XADVolumeSort` 让 `firstext`（如 `z01`）永远排最前，其余按名字不区分大小写排。 | 排序规则可预测，避免 `z2` 排在 `z10` 前之类问题（但源码把数字排序注释掉了）。 | 我们自己写排序应用数字自然序，而非照抄。 | 🔄待采用 |
| ④-04 | 卷内前缀/尾缀布局可被读侧挖掉 | `XADALZipParser.m:87-97` | ALZip 分卷每卷 16 字节头 + 8 字节尾会被跳过；说明「每卷加小头部」这一布局有现实解码器支持。 | 我们的分卷若需加自制头，至少 ALZip 类读者不会拒绝。 | 非通用：只有 ALZip 解析器会挖，ZIP/7z 读者会把垃圾算进数据。 | ⚠️已评估-不采用（会破坏 Z1 之外读者的兼容性；伪装布局应放在整包外层而不是每卷） |
| ④-05 | WinZipAES extra field 结构（写侧需照抄） | `XADZipParser.m:889-896`、`:1155-1177` | 0x9901 字段：version(2) + vendor `0x4541`('AE') + keySize(1) + 真实压缩法(2)；method 写成 99，version==2 时 CRC 放进 AES 认证块尾部。 | 我们要写 AES zip 时字段顺序照抄即可被主流工具解。 | 任一字段错位则别人解不开但自己可能「能解」（假通过）。 | 🔄待采用 |
| ④-06 | 数据描述符 + flag 0x08 的写侧含义 | `XADZipParser.m:590-617`、`:656-681` | 流式写 ZIP 时可选「先写数据、后写 crc/尺寸描述符」，读侧靠 12/16 字节（zip64:20/24 字节）模式识别。 | 允许打包时不回填尺寸（流式输出）。 | 描述符模式与数据偶合会误判 → 我们若生成，须同时设 flag 0x08，别无脑用。 | 🔄待采用 |
| ④-07 | ZIP64 必须写全（否则读者会退化） | `XADZipParser.m:1221-1224`、`:303-310`、`:742-747` | 读侧代码明说：>4 GiB 不写 Zip64 的包「只能一路解到底」；条目数 >65535 不写 Zip64 靠 46 字节魔数猜测。 | 反面设计约束：我们打包 >4 GiB / >65535 条目时必须写 Zip64。 | — | 🔄待采用 |
| ④-08 | 编码写入的确定性要求 | `XADZipParser.m:1013-1016`、`XADString.m:137-161`、`:104-121` | 读侧只有 flags&0x800 才认 UTF-8；非 UTF-8 时靠探测（可能猜错）。`escapedASCIIDataForString:` 是「编不出来就转义成 `%%%02x`」的兜底。 | 明确：我们写包**必须设 0x800 + 写 UTF-8**，并同时写 0x7075 extra。 | 老读者不认 extra，仍按本地代码页猜。 | 🔄待采用 |
| ④-09 | 尾部可留任意字节（写侧空间） | `XADZipParser.m:249-257`（comment 字段）、`src/layout/zip.rs:58`、`:65-66` | EOCD 后有注释字段（长度上限 65535）与任意尾部垃圾；读侧容忍，我们 `ZipLayout.comment_len` 已记录尾余。 | 伪装封面/尾注可合法放在 EOCD 之后，不影响任何读者的 ZIP 语义。 | 尾部超过 65535 字节的部分不属于合法 EOCD，但读侧容忍度因实现而异。 | ✅已采用（`src/layout/zip.rs:58` 已记录「EOCD 之后还剩多少字节」，用于诊断尾部夹带） |

---

## ⑤ 修补（重同步 / 部分抢救 / 偏移修复 / 救援）

| # | 技术点 | 出处(文件:行) | 做法(1-3句) | 优点 | 代价/风险 | 决定 |
|---|---|---|---|---|---|---|
| ⑤-01 | 无中央目录 → 线性重建条目表 | `XADZipParser.m:460-586` | `seek(0)` 后循环读 4 字节 id：`PK\x03\x04`（和怪例 `PK\x05\x06`）当 local 记录解析并建条目，`PK\x01\x02` 出现即停（说明 CD 其实在后面）。 | 中央目录被删/截断也能列全条目（= 我们的 LOCALS-CD-MODE 同族）。 | 遇到假 `PK` 需要重同步（⑤-03）。 | ✅已采用（`src/rescue.rs::rescue_by_locals`，走 7-Zip LOCALS-CD-MODE 路线） |
| ⑤-02 | 数据描述符搜索用「下一字节必须是 PK」二次确认 | `XADZipParser.m:590-617`、`:619-654`、`:663-675` | 匹配函数 checksum+size 对齐后，**再看描述符之后 2 字节是否为 `PK`**；不够字节则返回「候选」让扫描器继续；zip64 版按 20/24 字节。 | 误判率从 1/2^32 级再降一档，修补不会切错位置。 | 末尾条目无后继则需兜底。 | ✅已采用（`src/rescue.rs::find_descriptor`：描述符双重验证） |
| ⑤-03 | 损坏后重同步：扫下一个 PK 头 | `XADZipParser.m:686-701`（`MatchZipEntry`）、`:564-570` | 匹配 `PK` + 第 3 字节为 0 + 第 4 字节 ∈ {1,3,5}（即 `PK\x01\x02`/`\x03\x04`/`\x05\x06`），用 4 字节窗口滑过垃圾，并置 `XADIsCorruptedKey`。 | 垃圾区之后的数据照样能救回来。 | 同步点仍可能撞上数据里的假 PK（需额外置信校验）。 | 🔄待采用 |
| ⑤-04 | 部分抢救：先吐出已收到的部分 | `XADRARParser.m:246-248`、`:298-300`、`:554-574`（`addEntryWithBlock:...isCorrupted:`）、`XADRAR5Parser.m:206-229` | 分片流未收完或分片衔接错位（名字不匹配/首片标记异常）时，把已收集部分当一条条目吐出并标 `isCorrupted:YES`，然后重置分片列表继续。 | 「残缺也薅出来」有正式语义；坏一个文件不毁后面。 | 输出长度可能与声明不符（上层需明示）。 | 🔄待采用 |
| ⑤-05 | 中央目录记录本身坏 → 标脏继续 | `XADZipParser.m:360-363`、`:374-381` | local 签名不符时只标 `XADIsCorruptedKey`；CDR 签名不符才抛（注释留 `// could try recovering here`）。 | 局部坏不阻断整体。 | 未实现的恢复点（源码自己标注）。 | 🔄待采用 |
| ⑤-06 | 4 GiB 无 Zip64 → 干脆放弃中央目录 | `XADZipParser.m:217-232` | 找不到 Zip64 locator 且文件 >4 GiB 时，判定「32 位偏移已回绕」，直接走本地头线性重建。 | 避免按坏 CD offset 跳到垃圾区，反而丢光数据。 | 大包走慢路径。 | 🔄待采用 |
| ⑤-07 | 洞修补：用 skip handle 绕开坏区 | `XADSkipHandle.m:142-166`、`XADArchiveParser.m:905-924` | 把损坏/无关区间登记成 skip，条目数据偏移改用 `XADSkipOffsetKey` 指到洞后；读时自动跳洞。 | 无需重写文件即可「虚拟切除」坏区。 | 区间登记需精确；重叠直接抛异常。 | 🔄待采用 |
| ⑤-08 | solid 流损坏后重启链 | `XADRARParser.m:344-369` | 非 solid 文件到达时清空 solid 列表；发生 mismatch 时也清空，避免用污染过的解码状态继续。 | 阻止错误沿 solid 链传播。 | 后续 solid 文件必须重解（性能代价）。 | ⚠️已评估-不采用（我方不做 solid（见 ③-16）） |
| ⑤-09 | CRC 作为「切点确认」而非仅校验 | `XADCRCSuffixHandle.m:25-42`、`:71-101` | 在数据流后追加 CRC（4 或 2 字节）的 handle：读完主流后再读 CRC 字节比对，`isChecksumCorrect` 一次性判定。 | 把「尾部校验字段」变成可参与边界判定的证据。 | 只在格式自带尾 CRC 时可用。 | 🔄待采用 |
| ⑤-10 | 大文件按 64 KiB 块倒扫（坏包定位） | `XADZipParser.m:131-190` | 同样的分块反扫思路，用于「EOCD 位置未知/被推远」的包；块内从后往前找第一条签名。 | 定位成本可控（不用把整文件读进内存）。 | 极端情况退化为全文件扫描。 | 🔄待采用 |
| ⑤-11 | 解析期二次定位签名 | `XADRAR5Parser.m:143-155`（`readUntilSignature`）、`:166` | 识别期算出的位置不信任，解析开头再用同一函数在 1 MiB 内重扫一次，找不到直接抛专用异常。 | 识别与解析之间被外部改动/缓冲差异不会导致错位。 | 多一次 1 MiB 扫描。 | 🔄待采用 |
| ⑤-12 | 「无长度」流靠描述符/EOF 收尾 | `XADZipParser.m:515-528`、`:581-585` | 未记录尺寸时用描述符搜索定尾；循环结束仍有缓存的待定条目（疑似目录）则补吐出去。 | 不丢最后一条。 | 待定条目逻辑（目录猜测）较绕。 | 🔄待采用 |
| ⑤-13 | 目录/文件判定靠后续条目反推 | `XADZipParser.m:1018-1034` | 名字以 `/` 且未压缩大小 0 → 目录；否则若**下一个条目名字以「本名+/」开头**，回头把前一条改标为目录。 | DOS 包不写目录属性也能恢复层级。 | 需要缓冲一条（`prevdict`），且依赖条目顺序。 | 🔄待采用 |
| ⑤-14 | 汇总校验：先看脏标记再算 CRC | `XADUnarchiver.m:683-697` | 条目已被标 `XADIsCorruptedKey` 就直接返回解压错误、不做 CRC（避免二次误导）；有 CRC 用 CRC，只有大小则比长度。 | 错误归因干净：是「结构坏」还是「内容坏」分得清。 | 无 CRC 无长度时无法校验。 | 🔄待采用 |
| ⑤-15 | 存档级 `isCorrupted` 查询入口 | `XADArchive.m:404-409`、`XADArchiveParser.m:1012-1017` | 解析结束后 `isCorrupted` 反映「过程中是否遇到过任何问题」，供 UI 提示。 | 用户知情，而不是静默给出可疑结果。 | 粗粒度（不指明哪个条目）。 | 🔄待采用 |
| ⑤-16 | 坏包救援的「代价上限」缺失（反面） | `XADZipParser.m:128-130`（`TODO` 注释）、`:151-152`（TODO 合并区间）、`XADSkipHandle.m:151-152`（TODO） | 源码多处留 TODO：全文件扫描的内存代价、区间表不能合并相邻/重叠、`atEndOfFile` 未处理 EOF 处的 skip。 | 明确哪些是「没做完」而不是「设计如此」，我们可正向做掉。 | 我们若照抄会继承同样的坑。 | 🔄待采用（作为改进项：区间合并 + 可配置扫描上限） |
| ⑤-17 | 救援模式的可用性边界（未定型） | `XADArchive.m:770-830`（未细读）、`XADLibXADParser.m:31-60` | 存档门面里有子归档/冲突处理分支，libxad 兜底路径未逐一验证。 | — | 未读完，不敢下结论。 | ❓未判定 |

---

## 附 A：最值得采用的 Top 5

1. **②-03 浮动签名仲裁（最小偏移胜）** —— 我们「一个文件里藏多种格式」的核心场景有现成裁决规则（`XADArchiveParser.m:297-327`）：候选各自报发现偏移，谁更靠前谁是内层真身。
2. **①-02 EOCD 全文件分块反扫 + 20 字节重叠** —— 我们只扫尾部 1 MiB 窗口（`layout/zip.rs::DEFAULT_WINDOW`），EOCD 被推远（实测 L3 布局距尾 118902 字节，再远就危险）会漏；分块倒扫 + 20 字节重叠是被源码图解过的正确做法（`XADZipParser.m:118-191`）。
3. **②-09 0x7075 Unicode Path「先验 CRC 再采用」** —— 中文文件名乱码的确定性解法：只有 `CRC32(原始名)` 对得上才用 UTF-8 名，避免半截 extra 把好名字改坏（`XADZipParser.m:859-888`）。
4. **③-01 + ③-02 解码器=流包装器 + 方法→handle 路由表** —— 我们把「解压」整块留白了，这套 `源→解密→解压→CRC` 的 handle 链是投入产出比最高的骨架（`CSStreamHandle.m:45-93`、`XADZipParser.m:1192-1252`）。
5. **⑤-04 部分抢救 + 脏标记语义** —— 「残缺也薅出来」：已收到的分片照吐、标 `isCorrupted` 而不抛错，坏一片不毁后续（`XADRARParser.m:246-248`、`:298-300`；标记语义 `XADArchiveParser.m:1012-1017`）。

## 附 B：❓未判定清单（明确没看懂 / 没读完，待补）

| # | 待查项 | 出处 | 卡在哪 |
|---|---|---|---|
| B1 | libxad 动态库兜底的实际覆盖面与 `.adf` 后缀特判 | `XADLibXADParser.m:31-60`、`:440` | 未审 `libxad/` 子目录与 `xadmaster.h`（1029 行），不确定兜住了哪些格式 |
| B2 | `XADArchive` 的子归档/冲突处理全部分支 | `XADArchive.m:770-830` | 该段未逐行读，不确定「同名冲突」与「子归档失败」的完整决策树 |
| B3 | NSIS 自解压流的构造（LZMA/bzip2 混合） | `XADNSISParser.m:96-128` 之后 | 只读了识别段，流解析未读 |
| B4 | RAR5 multipart 命名与分片合并细节 | `XADRAR5Parser.m:909-921` | 只知道有 Multipart 分类，规则未读 |
| B5 | `XADMacArchiveParser` 资源叉拆分与 AppleDouble 落地 | `XADMacArchiveParser.m:360-430` | 与 ③-14 相关但我们暂不做 Mac 专有，未深读 |
| B6 | PDF 解析器的「重装外部格式头」具体实现 | `XADPDFParser.m:60-` | 只有分析文档转述，未验证行号 |

## 附 C：与 yshjy 现状对照

| 环节 | XADMaster 给出且我们**已有** | XADMaster 给出而我们**缺** |
|---|---|---|
| ① 读取 | 偏移窗口（①-05）、分段虚接（①-07）、可中断（①-15） | EOCD 全文件分块反扫+重叠（①-02）、卷扫描（①-08）、洞模型显式化（①-09/①-11）、maxheader 一次读（①-12） |
| ② 判断 | 签名扫描（②-02）、Zip64（②-16/②-17 底座）、GBK 名（②-10）、路径安全（②-23） | **浮动签名仲裁（②-03）**、有序注册表+优先级（②-01）、异常吞噬（②-04）、0x7075 CRC 校验（②-09）、编码探测器（②-11/②-12）、结构参数反推（②-13）、校验和二次确认（②-14）、两级怀疑（②-15） |
| ③ 解压 | （仅列条目） | **几乎全部**：handle 链骨架（③-01）、方法路由（③-02）、解密分族（③-05）、密码拉取（③-06）、嵌套下钻（③-09/③-10）、延迟建目录/链接（③-11）、属性回填（③-12）、选择性提取（③-13） |
| ④ 压缩 | 尾部余量记录（④-09） | **XADMaster 无内容可抄**；分卷命名规范（④-02）、AES extra 字段（④-05）、Zip64 必写（④-07）、UTF-8+0x7075 必写（④-08） |
| ⑤ 修补 | 本地头线性重建（⑤-01）、描述符双重验证（⑤-02） | 重同步扫下一个 PK（⑤-03）、部分抢救（⑤-04）、洞修补（⑤-07）、CRC 切点确认（⑤-09）、解析期二次定位（⑤-11）、目录反推（⑤-13）、汇总校验归因（⑤-14） |
