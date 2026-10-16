# unar / XADMaster 支持的归档格式完整清单（识别特征 · 优先级 · 加密）

> 数据来源：`/home/user/hermes/yshjy/refs/XADMaster/` 源码逐文件阅读（Objective-C 侧 51 个 `XAD*Parser.m` + libxad C 侧 12 个已编译 client 文件）。
> 本文只写**源码可证**的内容，每条附 `文件:行号`。识别逻辑对应的机制分析见同目录 `xadmaster.md`。
> 版本特征：含 MacPaw Way Ltd. 2017+ 新增的 SFX 检测重构与 `XADNSAParser` / `XADSARParser` / `XADNDSParser` / `XADSWFParser` / `XADPDFParser` / `XADWARCParser`。

---

## 0. 计数与总览

| 项 | 数量 | 出处 |
|---|---|---|
| 主注册表 `parserclasses` 条目 | **62** | `XADArchiveParser.m:167-241` |
| 浮动签名组 `parsersWithFloatingSignaturesClasses` | **3** | `XADArchiveParser.m:244-249` |
| libxad 已编译 client（`XADLibXADParser` 背后） | **30** | `libxad/clients.c:51-69` |
| **格式标识合计** | **92**（62 + 30） | — |
| 归并同族变体（SFX / 内嵌 / 多卷别名）后的独立格式族 | **约 77** | 见 §1.7 归并说明 |
| `maxheader`（嗅探时预读字节数） | **0x80000 = 512 KiB** | `XADArchiveParser.m:251-257`；由 `XADEmbeddedRARParser`（`XADRARParser.m:830-833`）与 `XADISO9660Parser`（`XADISO9660Parser.m:28`）共同决定 |

> **注 1（扩展名列的可信度）**：XADMaster **没有扩展名注册表**，所有识别都是内容判定（详见 §1.7）。因此本文「扩展名」列是各格式的行业通用扩展名，属**推断**而非源码事实。凡是源码里真的把文件名当判据的解析器，都在「识别特征」列显式标注 `【文件名判据】`。
>
> **注 2（生成方式）**：「识别特征」列全部逐字摘录自各 `+recognizeFileWithHandle:firstBytes:name:propertiesToAdd:` 实现，十六进制按源码字节序写。凡无法从源码确定的，写「**未能确定**」，不猜。

### 0.1 注册表分组（顺序即优先级，`XADArchiveParser.m:167-241`）

| 组 | 条目数 | 设计意图（源码注释） |
|---|---|---|
| ① Common formats | 7 | 最常用、最强 magic，先测 |
| ② Mac formats | 11 | Mac 专用生态 |
| ③ Less common formats | 23 | 少见但 magic 可靠 |
| ④ Detectors that require lots of work | 11 | **需要大量 IO / 全量扫描** |
| ⑤ Over-eager detectors | 9 | **签名弱、易误报，刻意排最后** |
| ⑥ LibXAD | 1 | 外部 Amiga 生态兜底，只在前面全失败时运行 |

---

## 1. 主注册表 62 个解析器（按注册顺序）

### 1.1 组① Common formats（#1–#7）

| # | 格式（别名） | 扩展名（推断） | 识别特征（magic / 判定条件，源码逐字） | 解析器类 | 源码位置 | 压缩/加密 | 备注 |
|---|---|---|---|---|---|---|---|
| 1 | **Zip**（PKZIP / WinZip / Info-ZIP） | `.zip` `.zipx` `.jar` `.apk` `.xpi` `.epub` `.docx` `.xlsx` `.pptx` `.odt` `.cbz` `.ipa` `.whl` `.nupkg` | `50 4B 03 04`@0 **或** `50 4B 05 06`@0 **或** `50 4B 03 04`@**4**（`XADZipParser.m:46-58`） | `XADZipParser` | `XADZipParser.m:44-58`，hdr=8 | 归档+压缩；**加密：ZipCrypto（`XADZipCryptHandle.m`）+ WinZip AES（`XADWinZipAESHandle.m`，PBKDF2-HMAC-SHA1 1000 轮）** | `@4` 那条是怪例（容忍 4 字节前缀）。容错极多：EOCD 分块反向扫、Zip64、无中心目录线性重扫、`PK\x07\x08` 重同步，见 `xadmaster.md` §5 |
| 2 | **RAR 1.5–4.x**（WinRAR ≤4、RAR3） | `.rar` `.rev` `.r00` `.r01` | `52 61 72 21 1A 07 00`（`Rar!\x1a\x07\x00`）**或** 远古签名 `52 45 7E 5E`（`RE~^`）@0 | `XADRARParser` | `XADRARParser.m:88-97`（签名）、`:117-124`（识别），hdr=7 | 归档+压缩；**加密：RAR13/15/20 自研流密码（`XADRAR13CryptHandle` / `XADRAR15CryptHandle` / `XADRAR20CryptHandle`）+ `XADRARAESHandle`** | **只认偏移 0**；拼接在后面的 RAR 由 `XADEmbeddedRARParser` 负责（#45） |
| 3 | **RAR 5**（WinRAR 5+） | `.rar` `.part1.rar` | `52 61 72 21 1A 07 01 00`，**前 1 MiB 内任意偏移**（`XADRAR5Parser.m:77-99` + `signatureLocationInData:`@`:83-99`） | `XADRAR5Parser` | `XADRAR5Parser.m:70-99`，hdr=8 | 归档+压缩；**加密：`XADRARAESHandle`（PBKDF2-HMAC-SHA256 + HMAC 校验）** | 常量 `RAR5MaximumSFXHeader = 1<<20`（`XADRAR5Parser.m:35`）。`-parse` 期**重新定位签名**（`:144-155`），找不到抛 `RAR5SignatureCannotBeFound` |
| 4 | **7-Zip** | `.7z` | `37 7A BC AF 27 1C 00`（7 字节，`XAD7ZipParser.m:39-43`） | `XAD7ZipParser` | `XAD7ZipParser.m:96-106`，hdr=32 | 归档+压缩；**加密：`XAD7ZipAESHandle`（AES-256-CBC + PBKDF2）** | 支持 BCJ2 / 分支过滤（`XAD7ZipBCJ2Handle`、`XAD7ZipBranchHandles`） |
| 5 | **Gzip**（GNU zip / gzip） | `.gz` `.tgz` `.svgz` | `1F 8B 08` **或** `1F 9E 08` **或** `1F A1`（BSD-compress 变体，`XADGzipParser.m:33-45`） | `XADGzipParser` | `XADGzipParser.m:31-45`，hdr=3 | 纯压缩流（单文件）；无加密 | 源码注释 `/* BSD-compress variant */`（`:41`）。扩展名仅用于给输出命名（`:88`） |
| 6 | **Bzip2** | `.bz2` `.tbz2` `.tbz` | `42 5A 68 31..39`（`"BZh"[1-9]`）**且** 6 字节块 magic ∈ {`31 41 59 26 53 59`(`\x31AY&SY`), `17 72 45 38 50 90`(π 位)} | `XADBzip2Parser` | `XADBzip2Parser.m:28-46`，hdr=10 | 纯压缩流；无加密 | — |
| 7 | **Tar**（ustar / GNU tar / star） | `.tar` | 【文件名判据】`^.*\.tar$`（ICASE）**或** 512B 头解析结果 ≠ `TAR_FORMAT_V7`；`getTarType:` 判定四态：checksum 通过=`V7_RECOGNIZED`、`57 53 74 61 72 00 30 30`(`ustar\0""`)@257→`USTAR`/**`STAR`**、`75 73 74 61 72 20 20 00`(`ustar  \0`)→`GNU` | `XADTarParser` | `XADTarParser.m:42-84`（`getTarType:`@`:42-73`、识别@`:77-84`），hdr=512 | 纯归档（不压缩）；无加密 | checksum@148(8B 八进制) 双重校验（有符号 + 无符号），`isTarChecksumCorrect:`@`:158`。支持 sparse（`XADTarSparseHandle`）。**后缀 `.tar` 直接放行**是刻意的 |
| | | | | | | | |

### 1.2 组② Mac formats（#8–#18）

| # | 格式（别名） | 扩展名（推断） | 识别特征 | 解析器类 | 源码位置 | 压缩/加密 | 备注 |
|---|---|---|---|---|---|---|---|
| 8 | **StuffIt**（StuffIt 1.x–4.x / SIT） | `.sit` | `BE32@10 == 72 4C 61 75`（`"rlau"`）**且**（`BE32@0 == 53 49 54 21`(`"SIT!"`) **或** 安装包变体 `"STin"` / `"ST<数字><数字>"`，`XADStuffItParser.m:41-63`） | `XADStuffItParser` | `XADStuffItParser.m:41-63`，hdr=22 | 归档+压缩；**加密：`XADStuffItDESHandle`** | 只覆盖到 4.x 算法族 |
| 9 | **StuffIt 5** | `.sit` `.sit5` | 字面串 `53 74 75 66 66 49 74 20 28 63 29 31 39 39 37 2D` + 4 个任意字节 + `20 41 6C 61 64 64 69 6E 20 53 79 73 74 65 6D 73 2C 20 49 6E 63 2E 2C 20 68 74 74 70 3A 2F 2F 77 77 77 2E 61 6C 61 64 64 69 6E 73 79 73 2E 63 6F 6D 2F 53 74 75 66 66 49 74 2F 0D 0A`（`"StuffIt (c)1997-\xFF\xFF\xFF\xFF Aladdin Systems, Inc., http://www.aladdinsys.com/StuffIt/\r\n"`，`\xFF` 为通配） | `XADStuffIt5Parser` | `XADStuffIt5Parser.m:33-43`，hdr=100 | 归档+压缩；**加密：`XADRC4Handle`**（`:417`） | `\xFF` 位置是版本字节，识别器按通配处理 |
| 10 | **StuffIt 5 for Windows（EXE 自解压）** | `.exe` | `4D 5A`(`MZ`)@0 **且** `BE32@4100 == 42 03 E8 53` | `XADStuffIt5ExeParser` | `XADStuffIt5Parser.m:430-441`，hdr=8192 | 归档+压缩（SFX 封装） | 固定偏移 4100 的"魔数"，是 StuffIt 5 Win 安装器/解包器的特征 |
| 11 | **StuffIt split file**（SIT 分卷） | `.sit` `.sitx`（分片） | `B0 56 00`@0 **且** `bytes[4] ∈ [1,63]`（分片数）**且** `bytes[5..5+bytes[4])` 全部非 0 | `XADStuffItSplitParser` | `XADStuffItSplitParser.m:30-44`，hdr=100 | 归档+压缩 | 注释直白：`// Assume there are less than 256 parts.` |
| 12 | **StuffIt X** | `.sitx` | `53 74 75 66 66 49 74 21`(`"StuffIt!"`) **或** `...3F`(`"StuffIt?"`) | `XADStuffItXParser` | `XADStuffItXParser.m:282-292`，hdr=10 | 归档+压缩；**加密：`XADRC4Handle` + `XADStuffItXCyanideHandle`**（`:153`、`:184`） | Cyanide = 自研流密码 |
| 13 | **BinHex 4.0** | `.hqx` | 前 8192 B 内**扫描**字面串 `28 54 68 69 73 20 66 69 6C 65 20 6D 75 73 74 20 62 65 20 63 6F 6E 76 65 72 74 65 64 20 77 69 74 68 20 42 69 6E 48 65 78`（`"(This file must be converted with BinHex"`，40B），跳过行尾后必须遇到 `:`（`0x3A`），然后**真解一小段并校验 CRC16**（多项式反向 `0x1021`） | `XADBinHexParser` | `XADBinHexParser.m:30-62`，hdr=8192 | 归档（文本封装 + 资源叉/数据叉）；无加密 | 教科书式"贪心而脆弱"识别器：靠 `@try/@catch` 兜错（见 `xadmaster.md` §1.4、§2.3） |
| 14 | **MacBinary I / II / III** | `.bin` `.macbin` | 委托 `XADMacArchiveParser +macBinaryVersionForHeader:`（`XADMacArchiveParser.m:34-76`）：4 级判定 —— ①零填充约束（`bytes[0]==0`、`[74]==0`、`[82]==0`、`[108..115]` 全 0）②合法名（`bytes[1]∈[1,63]` 且名字区无 0）③校验和 A：`CRC(bytes,124,reverse 0x1021) == XADUnReverseCRC16(BE16@124)`，或校验和 B：`CRC(0xffff,bytes,124,crc_a001)^0xffff == BE16@124` ④`BE32@102 == 6D 42 49 4E`(`"mBIN"`) → v3，否则 v2；都失败退化为 v1 启发式 | `XADMacBinaryParser`（实际逻辑在 `XADMacArchiveParser`） | `XADMacBinaryParser.m:30-33`；核心 `XADMacArchiveParser.m:34-76`、`:360-430`，hdr=128 | 归档（Mac 双叉封装）；无加密 | **两级"怀疑"机制**：`XADIsMacBinaryKey` / `XADMightBeMacBinaryKey`；线索由别的解析器零成本埋下（`XADZipParser.m:1040` 条目名 `.bin` 结尾、`XADLZHParser.m:240`），昂贵验证推迟到有人问；**不可 seek 的流直接不验证**（`XADMacArchiveParser.m:374`） |
| 15 | **AppleSingle / AppleDouble** | `.as`（Single）、`._*`（Double） | `BE32@0 ∈ {00051600, 00051607, 00160500, 07160500}` **且** `BE32@4 ∈ {00020000, 00000200}` | `XADAppleSingleParser` | `XADAppleSingleParser.m:32-45`，hdr=8 | 归档（Mac 元数据/叉封装）；无加密 | 扩展属性解析在 `XADAppleDouble.m:85-96` |
| 16 | **DiskDoubler** | `.dd`（识别器还处理 `...dd` 去尾，`XADDiskDoublerParser.m:91`） | 三选一：①`BE32@0 == AB CD 00 54` **且**（`CRC(bytes,82,reverse 0x1021)==XADUnReverseCRC16(BE16@82)` **或** `BE16@82==0`，极老文件）②`BE32@0 == 44 44 41 52`(`"DDAR"`) **且** CRC 校验 @76 ③`BE32@0 == 44 44 41 32`(`"DDA2"`) **且** CRC 校验 @60 | `XADDiskDoublerParser` | `XADDiskDoublerParser.m:49-80`，hdr=124 | 归档+压缩；XOR 混淆（`XADXORHandle` / `XADXORSumHandle`，`:422-508`，非用户加密） | 三种代际（原始 / DDAR / DDA2） |
| 17 | **PackIt**（Mac） | `.pit` | `50 4D 61`(`"PMa"`) **且** `bytes[3] ∈ {67 'g', 34 '4', 35 '5', 36 '6'}` | `XADPackItParser` | `XADPackItParser.m:33-43`，hdr=4 | 归档+压缩；XOR/DES 混淆（`XADPackItXORHandle` / `XADPackItDESHandle`，同文件） | — |
| 18 | **Now Compress**（Now Software / 早期 Mac） | `.nc`（**推断，未能确定**） | `00 02`@0 **且** `BE32@8 ∈ [1,65535]`（文件数）**且** `bytes[24] ∈ [1,31]`（名长）**且** `bytes[25..25+名长)` 全 ≥32（可打印）**且** `sum(bytes[24..129]) == BE32@130`（校验和） | `XADNowCompressParser` | `XADNowCompressParser.m:35-58`，hdr=24 | 归档+压缩；无加密 | 头部求和校验是强判据。扩展名未见源码记载 |

### 1.3 组③ Less common formats（#19–#41）

| # | 格式（别名） | 扩展名（推断） | 识别特征 | 解析器类 | 源码位置 | 压缩/加密 | 备注 |
|---|---|---|---|---|---|---|---|
| 19 | **PPMd**（7-Zip PPMd 裸流） | `.pmd` `.ppmd` | `84 AC AF 8F`@0 **或** 字节反序 `8F AF AC 84` | `XADPPMdParser` | `XADPPMdParser.m:31-42`，hdr=16 | 纯压缩流；无加密 | 支持 Variant G/H/I（`PPMd/Variant*.c`） |
| 20 | **XAR**（eXtensible ARchive，macOS `.pkg` 内层） | `.xar` `.pkg` | `78 61 72 21`(`"xar!"`) | `XADXARParser` | `XADXARParser.m:51-58`，hdr=4 | 归档+压缩（zlib/bzip2/lzma 任选）；**无加密**（`<encryption>` 未实现） | 读 XML TOC + 扩展属性（`:158`） |
| 21 | **Compress (.Z)**（LZW，compress(1)） | `.Z` `.taz` | `1F 9D` | `XADCompressParser` | `XADCompressParser.m:29-35`，hdr=3 | 纯压缩流；无加密 | 与 `1F 9D` 混淆的格式极少，是安全 magic |
| 22 | **RPM**（Red Hat / Fedora 包） | `.rpm` | `ED AB EE DB` | `XADRPMParser` | `XADRPMParser.m:27-40`，hdr=96 | **封装**：剥离 RPM 头，交出内层 cpio（gz/bz2/xz/lzma 之一） | 处理 `sigtype` ∈ {0,1,5}；不支持的 type 走 `reportInterestingFileWithReason:`（`:44` 附近）并抛 `NotSupported`。默认内层 `cpio` + 压缩后缀 `gz`（`:60-61`） |
| 23 | **XZ**（xz-utils / LZMA2 容器） | `.xz` `.txz` | `FD 37 7A 58 5A 00` | `XADXZParser` | `XADXZParser.m:28-38`，hdr=6 | 纯压缩流；无加密 | **源码注释里保留着被禁用的旧 LZMA magic 分支** `ff 4C 5A 4D 41 00`（`:34`），说明曾有 `\xffLZMA\0` 变体 |
| 24 | **SWF**（Flash / Shockwave） | `.swf` | `46 57 53`(`"FWS"`) **或** `43 57 53`(`"CWS"`) **且** `bytes[3] < 32`（版本上限启发式，`XADSWFParser.m:37`） | `XADSWFParser` | `XADSWFParser.m:32-45`，hdr=6 | **容器**（非压缩包）：Zlib 解压后扫 tag，抽出内嵌位图/JPEG（`XADSWFTagParser.m`） | 注释保留：`// Assume the SWF format doesn't reach version 32 any time soon.`。属"资源提取"用途 |
| 25 | **PDF** | `.pdf` | `25 50 44 46 2D`(`"%PDF-"`) **且** 总长 ≥ 53 | `XADPDFParser` | `XADPDFParser.m:41-55`，hdr=53 | **容器**：遍历 PDF 对象，抽 image stream，**现场重装 JPEG/TIFF 头**（`CreateTIFFHeaderWithEntries` / `CreateNewJPEGHeaderWithColourProfile`）；**加密：标准安全处理器 RC4/AES**（`PDF/PDFEncryptionHandler.m`、`PDF/PDFEncryptionUtils.m`） | 弱识别（只查 `%PDF-`）。xref 表 vs xref 流双路 + `/Prev` 链回溯（`PDF/PDFParser.m:210-241`）。异常色彩空间走 `reportInterestingFileWithReason:`（`:159-160`、`:262-264`） |
| 26 | **ALZip** | `.alz` | `41 4C 5A`(`"ALZ"`) **且** `bytes[3]==01` **且** `bytes[7]==00` | `XADALZipParser` | `XADALZipParser.m:62-70`，hdr=8 | 归档+压缩；无加密 | 韩国 ALZip；`.egg` 是另一支（**未支持**） |
| 27 | **CAB**（Microsoft Cabinet） | `.cab` | `4D 53 43 46`(`"MSCF"`) | `XADCABParser` | `XADCABParser.m:62-69`，hdr=4 | 归档+压缩（MSZIP/LZX/Quantum）；**无加密**（MacPaw 版未实现 CAB 弱加密） | 结构与 `MatchCABSignature`（`:347-360`）用于 SFX 场景 |
| 28 | **CFBF / OLE2 复合文档**（doc/xls/ppt/msi 的容器） | `.doc` `.xls` `.ppt` `.msi` `.msg` `.vsd` `.msix` | `D0 CF 11 E0 A1 B1 1A E1` **且** `bytes[28]==FE && bytes[29]==FF`（版本号，额外保险） | `XADCFBFParser` | `XADCFBFParser.m:51-60`，hdr=512 | **容器**（无压缩，但按"归档"处理：把每个 stream 当条目）；无加密（加密在 OLE 层，未实现） | 严格解析 MS-CFB：DIFAT → FAT → MiniFAT（`:1-40+`）。源码注释明确说明**不强制 spec 的 sector shift 值以兼容非规范文件**，但拒绝 `idspersec<=1` 防除零 |
| 29 | **CAB SFX**（自解压 CAB） | `.exe` | `4D 5A`(`MZ`)@0 **且** 总长 ≥ 20000 **且** 从偏移 8 起逐字节扫 `4D 53 43 46`(`"MSCF"`) 并通过 `MatchCABSignature` 结构校验（len>offs 且 cabinet 落在文件内） | `XADCABSFXParser` | `XADCABParser.m:375-393`，hdr=65536 | 归档+压缩（SFX） | 记录 `CABSFXOffset`，`-parse` 时 `seek` 过去再 `[super parse]`（`:397-403`） |
| 30 | **LZH / LHA / LArc / PMarc** | `.lzh` `.lha` `.lzs` `.larc` `.pma` | `bytes[2] == 2D`('-') **且** `bytes[3..4] ∈ {6C 68("lh"), 6C 7A("lz"), 70 6D("pm")}` **且** `bytes[6] == 2D`('-') **且** `bytes[5]` 合法：lh→`{0,1,2,3,4,5,6,7,d}`、lz→`{0,4,5}`、pm→`{0,1,2}` | `XADLZHParser` | `XADLZHParser.m:34-60`，hdr=7 | 归档+压缩；**无加密**（老 LHA 的 `-p` 弱口令未实现） | `bytes[5]=='d'` = 目录条目。同文件埋 `XADMightBeMacBinaryKey`（`:240`） |
| 31 | **LZH SFX（Amiga LhA）** | `.lha` `.exe` | `BE32@44 == 53 46 58 21`（`"SFX!"`） | `XADLZHAmigaSFXParser` | `XADLZHSFXParsers.m:29-35`，hdr=48 | 归档+压缩（SFX） | 若 `lha150r` 则对数据套固定 XOR 密钥 `BOA\x0F`（`XADLZHSFXParsers.m:57-63`），属"混淆"非加密 |
| 32 | **LZH SFX（Commodore 64）** | `.lha` `.prg` | `bytes[0]==01 && bytes[2]==28 && bytes[3]==1C` **且** `bytes[0xd30]=='1'` **且** `bytes[0xd44..0xd46]=="LHA"` **且** `bytes[0xe8b..0xe8f]=="-lh-"` | `XADLZHCommodore64SFXParser` | `XADLZHSFXParsers.m:76-84`，hdr=0xe90 | 归档+压缩（SFX） | 4 处固定偏移交叉验证，误报率低但**要求文件 ≥ 0xe90 字节** |
| 33 | **LZH SFX（通用，4 组变体）** | `.exe` `.lzh` | 四选一（`XADLZHSFXParsers.m:105-120`）：①`bytes[36]=='L' && bytes[38]=='-' && bytes[39]==0x27 && BE32@40 == 73 20 53 46` ②`BE32@32 == 4C 5A 53 53`(`"LZSS"`) **且** `BE32@36 == 20 73 65 6C`(`" sel"`) ③`BE32@6 == 53 46 58 20`(`"SFX "`) **且** `BE32@10 == 6F 66 20 4C`(`"of L"`) **且** `BE32@14 == 48 61 72 63`(`"Harc"`) ④`bytes[37]=='L' && bytes[38]=='H' && BE32@76 == 6E 61 6D 65`(`"name"`) **且** `BE32@80 == 20 74 6F 20`(`" to "`) | `XADLZHSFXParser` | `XADLZHSFXParsers.m:105-122`，hdr=84 | 归档+压缩（SFX） | 这四条都源自 SFX 宿主程序的**字符串残留**（"LZSS sel..." / "SFX ... of LHarc"），不是格式 magic |
| 34 | **Zoo**（zoo 2.x） | `.zoo` | `DC A7 C4 FD`@**0x14**（非 0 偏移 magic） | `XADZooParser` | `XADZooParser.m:30-38`，hdr=0x22 | 归档+压缩；**无加密**（老 zoo 弱口令未实现） | 同文件有 `XADZooMethod1Handle` |
| 35 | **LZX**（Amiga LZX） | `.lzx` | `4C 5A 58`(`"LZX"`，仅 3 字节，**弱**) | `XADLZXParser` | `XADLZXParser.m:32-38`，hdr=10 | 纯压缩流；无加密 | 与 Microsoft LZX（CAB/WIM 内部）**无关**，本解析器只处理 Amiga LZX |
| 36 | **PowerPacker**（Amiga） | `.pp` `.pp20` | `50 50 32 30`(`"PP20"`) | `XADPowerPackerParser` | `XADPowerPackerParser.m:31-37`，hdr=4 | 纯压缩流；无加密 | — |
| 37 | **NDS**（Nintendo DS ROM 镜像） | `.nds` `.dsi` | `bytes[0x12]==0` **且** `bytes[0x13] ≤ 7` **且** `bytes[0x15..0x1d]` 全 0 **且** `CRC16(0xffff, [0xc0,0x15c), 表 a001) == LE16@0x15c` **且** `CRC16(0xffff, [0,0x15e), 表 a001) == LE16@0x15e`（`XADNDSParser.m:32-47`） | `XADNDSParser` | `XADNDSParser.m:32-47`，hdr=0x200 | **镜像**（非压缩）：抽 ROM 内文件系统 + 把 tiled 图标转 PNG（`ConvertTiledIconToPNG`） | **双重 CRC16 是极强判据**，几乎不可能误报 |
| 38 | **NSA**（NScripter archive） | `.nsa`（必须 `arc*.nsa`） | 【文件名判据】**只看文件名** `^arc[0-9]*\.nsa$`（ICASE），**内容一行不看**（原 `bytes` 检查被注释掉，`XADNSAParser.m:39-41`） | `XADNSAParser` | `XADNSAParser.m:34-42`，hdr=6 | 归档+压缩（内部 LZSS，`XADNSALZSSHandle`）；无加密 | 2017 增补，用于日本 AVG 游戏资源提取。抽 SPB 图转 BMP（`:26-28`） |
| 39 | **SAR** | `.sar`（必须 `arc*.sar`） | 【文件名判据】**只看文件名** `^arc[0-9]*\.sar$`（ICASE） | `XADSARParser` | `XADSARParser.m:28-36`，hdr=6 | 归档（**未能确定**内部算法：本文件无压缩 handle，可能仅目录 + 原始数据） | 同上，NScripter 体系 |
| 40 | **Ar**（Unix ar / deb） | `.a` `.deb` `.lib` | `21 3C 61 72 63 68 3E 0A`(`"!<arch>\n"`) | `XADArParser` | `XADArParser.m:27-35`，hdr=6 | 纯归档（不压缩）；无加密 | `.deb` 是 ar + tar 双层，靠递归嗅探自动下钻 |
| 41 | **WARC**（Web ARChive） | `.warc` `.warc.gz` | `57 41 52 43 2F 31 2E 30 0D 0A`(`"WARC/1.0\r\n"`) **或** `"WARC/1.1\r\n"` | `XADWARCParser` | `XADWARCParser.m:28-38`，hdr=10 | 归档（HTTP 记录容器，不压缩）；无加密 | 有测试 `XADMasterTests/WARC/XADWARCParserTests.m` |

### 1.4 组④ Detectors that require lots of work（#42–#52）—— 需要大量 IO / 全量扫描

| # | 格式（别名） | 扩展名（推断） | 识别特征 | 解析器类 | 源码位置 | 压缩/加密 | 备注 |
|---|---|---|---|---|---|---|---|
| 42 | **WinZip Self-Extractor** | `.exe` | `4D 5A`(`MZ`)@0 **且** 前 20480 B 内扫到字面串 `57 69 6E 5A 69 70 28 52 29 20 53 65 6C 66 2D 45 78 74 72 61 63 74 6F 72`（`"WinZip(R) Self-Extractor"`，24B） | `XADWinZipSFXParser` | `XADZipSFXParsers.m:59-73`，hdr=20480 | 归档+压缩（SFX） | **不进浮动签名组**（外层 `MZ` 已足够唯一）。识别的是"宿主程序里的版权串"，不是内层包 |
| 43 | **ZipIt SEA**（Mac，ZipIt 自解压） | `.sea` `.zip` | `BE32@0 == 4A 6F 79 21`(`"Joy!"`) | `XADZipItSEAParser` | `XADZipSFXParsers.m:85-95`，hdr=4 | 归档+压缩（SFX） | 4 字节 magic，简单可靠 |
| 44 | **Zip SFX**（自解压 ZIP） | `.exe` | `4D 5A`(`MZ`)@0 **且** 扫描 `50 4B 03 04` **且** `bytes[i+4] ∈ [10,40)`（version-needed 低字节）**且** `bytes[i+9] == 0`（压缩方法高字节） | `XADZipSFXParser` | `XADZipSFXParsers.m:28-47`，hdr=0x10000 | 归档+压缩（SFX） | ✅ **浮动签名组**。写 `XADSignatureOffset`。两个额外约束（版本区间 + 方法高字节为 0）用于**排除误命中** |
| 45 | **Embedded RAR**（拼接在后部的 RAR） | `.exe` `.sfx` `.rar` | 前 0x80000 内**线性扫** `52 61 72 21 1A 07 00`（`FindSignature`，`XADRARParser.m:99-106`） | `XADEmbeddedRARParser` | `XADRARParser.m:835-853`，hdr=0x80000 | 归档+压缩 | ✅ **浮动签名组**。写 `RAREmbedOffset` + `XADSignatureOffset`。`volumesForHandle:` 依据 RAR 头 flags bit0(分卷) + bit4(新命名) 再列目录找卷（`:855-878`） |
| 46 | **Embedded RAR 5**（拼接在后部的 RAR5） | `.exe` `.sfx` `.rar` | 前 **1 MiB**（实际受 `maxheader` 限到 512 KiB）内扫描 `52 61 72 21 1A 07 01 00` | `XADEmbeddedRAR5Parser` | `XADRAR5Parser.m:865-872`，**继承父类 hdr=8 / `recog L77`** | 归档+压缩 | ✅ **浮动签名组**。**注意：它没有覆写 `recognizeFileWithHandle:`**，用的是 `XADRAR5Parser` 的那份（即 RAR5 与 RAR5-SFX 的识别逻辑完全相同，只在 `formatName`/仲裁上区分） |
| 47 | **7-Zip SFX** | `.exe` | `4D 5A`(`MZ`)@0 **且** 从 0 起按 **512 字节步长**扫 `37 7A BC AF 27 1C 00` | `XAD7ZipSFXParser` | `XAD7ZipParser.m:1032-1050`，hdr=0x40000 | 归档+压缩（SFX） | 源码注释 `// TODO: Is this enough?`（`:1028` 附近）——作者自己不确定 0x40000 够不够 |
| 48 | **NSIS 安装包**（Nullsoft） | `.exe` | 从 0 起按 **512 字节步长** 依次试三种 16 字节签名（`XADNSISParser.m:46-69`）：① **Older**：`EC BE AD DE` + `4E 75 6C 6C 53 6F 66 74 49 6E 73 74`(`"NullSoftInst"`)，或 `ED BE AD DE` + 同串 ② **Old**：`ptr+4` 起 `EF BE AD DE` + `"NullSoftInst"` 且 `LE32@0 & 2 == 0`（排除卸载器）③ **New**：`ptr+4` 起 `EF BE AD DE` + `4E 75 6C 6C 73 6F 66 74 49 6E 73 74`(`"NullsoftInst"`，小写 s) 且 `LE32@0 & 2 == 0` | `XADNSISParser` | `XADNSISParser.m:98-125`，hdr=0x10000 | **安装包封装**（内含 LZMA/bzip2 数据，`XADNSISBzip2Handle.m`）；无加密 | 记 `NSISOlderOffset` / `NSISOldOffset` / `NSISNewOffset`。另有 `LooksLikeLZMA`（`5D 00 00 .. 00`，`:71-74`）辅助判 payload |
| 49 | **Gzip SFX**（shell 脚本 + gzip） | `.sh` `.run` `.bin` | `23 21`(`"#!"`)@0 **且** 前 5000 B 内扫 `1F 8B 08` **或** `1F 9E 08` | `XADGzipSFXParser` | `XADGzipParser.m:139-160`，hdr=5000 | 纯压缩流（SFX 外壳）；无加密 | 记 `GzipSFXOffset`；`-parse` 时 `seek` 过去再 `[super parse]`（`:162-168`） |
| 50 | **Compact Pro** | `.cpt` `.sea` | `bytes[0]==1` **且** `BE32@4` = 目录偏移，**遍历整张目录表重算 CRC32（表 `edb88320`）并比对**（`XADCompactProParser.m:32-72`）。若 `fileSize == CSHandleMaxLength`（不可 seek 的流）**降级为只看扩展名** `\.(cpt\|sea)`（`:38`） | `XADCompactProParser` | `XADCompactProParser.m:32-72`，hdr=8 | 归档+压缩；无加密 | **识别期真的解完整张目录表**，全套在 `@try` 里（`:69` `@catch(id e) {}`）。典型的"昂贵但几乎零误报" |
| 51 | **ARJ** | `.arj` `.a01` | **全量扫描** `60 EA`，要求 `LE16@(i+2) ∈ [32,2600]`（头长）**且** `CRC32(0xffffffff, header[i+4 .. i+4+size], 表 edb88320) == ~LE32@(i+4+size)`（`XADARJParser.m:38-60`） | `XADARJParser` | `XADARJParser.m:38-60`，hdr=0x8000 | 归档+压缩；**加密：`XADXORHandle` + password（`:256`）** | `60 EA` 只有 2 字节，**靠反向 CRC32（取反比较）兜住误报**。`XADMasterTests/` 下**无** ARJ 专用测试文件 |
| 52 | **Zip（多卷单片）** | `.zip`（必须含 `zip`） | 【文件名 + 可 seek + 尾部判据】必须 `CSFileHandle`（`:111`）**且** 名字匹配 `\.zip`（ICASE，`:113`）**且** 从文件尾 0x10011 字节窗口内**反向扫** `50 4B 05 06`（EOCD，`:115-135`） | `XADZipMultiPartParser` | `XADZipSFXParsers.m:107-135`，hdr=8 | 归档+压缩 | 注册顺序在 `XADZipParser`(#1) **之后**，因此**只在 #1 失败时**（开头没有 `PK`）才轮到它 —— 这正是"头部被子文件截断的多卷 zip"的救回路径。卷扫描用**两种命名正则**、`z01` 优先（`:137-154` + `XADArchiveParser.m:842-890`） |

### 1.5 组⑤ Over-eager detectors（#53–#61）—— 签名弱、刻意排最后

| # | 格式（别名） | 扩展名（推断） | 识别特征 | 解析器类 | 源码位置 | 压缩/加密 | 备注 |
|---|---|---|---|---|---|---|---|
| 53 | **ARC**（SEA ARC 5.21+） | `.arc` | `bytes[0] == 1A` **且** 文件名合法（`IsRegularARCName`：`bytes[2]!=0` 且 `[2..0xf)` 遇 0 前全 ≥32）**且** 布局自洽（`IsARCHeader`，`XADARCParser.m:44-100`）；**明确排除 loader 名** `" unpacker "`（`48 20 75 6E 70 61 63 6B 65 72 20`，`IsLoaderARCName`@`:32-35`） | `XADARCParser` | `XADARCParser.m:100-107`，hdr=0x1d | 归档+压缩；**加密：`XADXORHandle` + password（`:240`）** | 方法 1 分支按 `size<16MB` + 下一字节必为 `1A` 校验（`:64-76`）；压缩分支校验 compsize/uncompsize（`:78-100`） |
| 54 | **ARC SFX** | `.com` `.exe` | 三种宿主布局：① `@0` 即合法 ARC 头（`.COM`，如 ARC520.COM）② `@3` 即合法 ARC 头（前置 3 字节 jump）③ `MZ`@0 + 从其内部扫 ARC 头 | `XADARCSFXParser` | `XADARCParser.m:345-400` | 归档+压缩（SFX） | 记 `ARCSFXOffset`；偏移按 `datasize + 0x19 / 0x1d`（±3）推算 |
| 55 | **Squeeze**（CP/M `SQ`） | `.sq` `.qqq` | `76 FF` **且** `bytes[4] != 0` **且** 从 `bytes[4]` 起逐字节找 `0`（名字终止），途中所有字节必须 ≥32 | `XADSqueezeParser` | `XADSqueezeParser.m:94-115`，hdr=1024 | 纯压缩流；无加密 | `76 FF` 是 CP/M 时代弱签名 |
| 56 | **Crunch**（CP/M `CR`) | `.cr` `.z`（**推断**） | `76 FE` 或 `76 FD` **且** `bytes[2] != 0` **且** 找到 0 终止后**紧跟的两个字节 ∈ [0x10,0x2f]`（ASCII 日期数字） | `XADCrunchParser` | `XADCrunchParser.m:121-146`，hdr=1024 | 纯压缩流；无加密 | 注意源码里 `//if(bytes[i]<32) return NO;` 被注释掉 —— 容错放宽 |
| 57 | **LBR**（CP/M `.LBR` / ARK） | `.lbr` `.ark` | 128B 头字段约束：`bytes[0]==0`、`bytes[1..11]` 全 `' '`(0x20)、`bytes[12..13]==0`、`bytes[26..31]==0`、`LE16@14`(sectors)`!=0`；若 `LE16@16`(crc) 非 0 且 `sectors*128 ≤ length`，再校验**反向 CRC16（多项式 `0x1021`）** | `XADLBRParser` | `XADLBRParser.m:33-57`，hdr=128 | 归档+压缩；无加密 | — |
| 58 | **LZMA_Alone**（`.lzma` 裸流） | `.lzma` `.tlz` | **无 magic**，纯参数不变量（`XADLZMAAloneParser.m:28-46`）：`bytes[0] < 9*5*5`(225) **且** `LE32@1`(dictsize) 是 `2^k` 或 `3·2^k`（用乘 `0xaaaaaaab` 除 3 判）**且** `LE64@5`(uncompressedSize) `!= 0` **且** `bytes[11..12]` 全 `00` 或全 `FF` | `XADLZMAAloneParser` | `XADLZMAAloneParser.m:28-46`，hdr=13 | 纯压缩流；无加密 | **源码原注释**：`// Geez, put some magic bytes in your file formats, people!`。识别器里 `if(...objectForKey:@".lzma")` 的**扩展名分支被注释掉了**（`:47-50`），所以确实纯靠内容 |
| 59 | **Cpio**（odc/newc/crc + 二进制） | `.cpio` | `30 37 30 37 30 37`(`"070707"`，odc) **或** `"070701"`(newc) **或** `"070702"`(crc) **或** `71 C7` / `C7 71`（二进制 cpio 的字节序标记） | `XADCpioParser` | `XADCpioParser.m:30-46`，hdr=6 | 纯归档（不压缩）；无加密 | RPM（#22）解出 cpio 后靠递归嗅探自动接上 |
| 60 | **Split file**（`.001/.002` 分片） | `.001` `.002` … | 【文件名 + 文件系统判据】必须 `CSFileHandle`（`:34`）**且** 名字匹配 `^(.*)\.([0-9]{3})$`（ICASE）**且** **磁盘上另一个分片真的存在**：`001` 则找 `002`，否则找 `001`（`:37-44`）。**内容一行不看** | `XADSplitFileParser` | `XADSplitFileParser.m:30-44`，hdr=0 | 无（纯切分，非压缩）；无加密 | 阳性条件就是 `XADPlatform fileExistsAtPath:`。**"文件系统本身也是判据"** |
| 61 | **ISO 9660**（+ High Sierra 混合） | `.iso` `.bin` `.cue` `.img` | 从 `0x8000` 起扫描：**ISO 9660** `block[0]==01` **且** `block[1..5]=="CD001"` **且** `block[6]==01`；**High Sierra** `block[8]==01` **且** `block[9..14]=="CDROM1"`。命中后在"下一块"里再找一次（`j ∈ [2048,2448)`），**用间距反推扇区大小**（2048 或 2352 原始 CD），并写 `ISO9660ImageBlockSize` / `ISO9660ImageBlockOffset = i - j*16` / `ISO9660ImageIsHighSierra` | `XADISO9660Parser` | `XADISO9660Parser.m:30-105`，hdr=**0x80000** | **镜像**（扇区文件系统，按归档抽文件）；无加密 | 一行 `i - j*16` 同时解决**前缀偏移 + 扇区大小 + 扇区 0 定位**（卷描述符固定在扇区 16）。这是"光盘镜像前面被拼接了别的数据"能识别的关键 |

### 1.6 组⑥ LibXAD（#62）

| # | 格式 | 扩展名 | 识别特征 | 解析器类 | 源码位置 | 备注 |
|---|---|---|---|---|---|---|
| 62 | **LibXAD 兜底**（30 个外部 client，见 §2） | 任意（`.adf` 特殊） | ① **旁路**：`[[name pathExtension] lowercaseString] == "adf"` 直接 `YES` ② 否则调 `xadRecogFile(xmb, [data length], [data bytes], XAD_INHOOK, &inhook, TAG_DONE)`，找到 client 则把 `client->xc_ArchiverName` 写进属性 `LibXADFormatName` | `XADLibXADParser` | `XADLibXADParser.m:42-68` | 排在整个注册表**最末尾**，只在所有内置解析器失败后才跑。源码注释：`// Kludge to recognize ADF disk images, since the filesystem parsers don't provide recognition functions`。`.adf` 旁路的存在意味着**一个叫 `x.adf` 的任意文件也会被当成归档**（唯一的后缀强判据） |

### 1.7 归并说明（92 → 约 77 个独立格式族）

以下条目是"同族别名 / SFX / 内嵌 / 多卷"变体，归并后独立格式数减少 15：

| 族 | 注册表条目 | 归并 |
|---|---|---|
| ZIP | #1 `XADZipParser`、#42 `WinZipSFX`、#43 `ZipItSEA`、#44 `ZipSFX`、#52 `ZipMultiPart` | 5 → 1 |
| RAR 4 | #2 `XADRARParser`、#45 `EmbeddedRAR` | 2 → 1 |
| RAR 5 | #3 `XADRAR5Parser`、#46 `EmbeddedRAR5` | 2 → 1 |
| 7-Zip | #4 `XAD7ZipParser`、#47 `7ZipSFX` | 2 → 1 |
| Gzip | #5 `XADGzipParser`、#49 `GzipSFX` | 2 → 1 |
| LZH | #30 `LZH`、#31 AmigaSFX、#32 C64SFX、#33 通用SFX | 4 → 1 |
| CAB | #27 `CAB`、#29 `CABSFX` | 2 → 1 |
| ARC | #53 `ARC`、#54 `ARCSFX` | 2 → 1 |
| StuffIt | #8 `StuffIt`、#9 `StuffIt5`、#10 `StuffIt5Exe`、#11 `StuffItSplit`、#12 `StuffItX` | 5 → 3（StuffIt 1–4 / StuffIt 5 / StuffIt X） |
| 合计 | 62 条 | **→ 47 个格式族**（+ libxad 30 = **约 77**） |

---

## 2. libxad 外部客户端（30 个，`XADLibXADParser` 背后）

编译列表出自 `libxad/clients.c:51-69`；client 名出自各文件末的 `XADCLIENT(...)` 结构体（名字为 `xc_ArchiverName`）。
`libxad/clients/` 目录里另有 **9 个未编译**的 client 文件（`DCS.c` / `MDC.c` / `PackDev.c` / `PackDisk.c` / `SuperDuper3.c` / `Tar.c` / `xDisk.c` / `xMash.c`，以及仅 `XAD_GPLCLIENTS` 下编译的 `HA.c` / `TR-DOS.c`——后两个**文件根本不存在**），原因是注释写明 `// uses XPK only, not supported`（`clients.c:54-68`）。

### 2.1 文件归档器 / 整盘压缩器

| # | Client 名 | 类 | 文件 | 说明 |
|---|---|---|---|---|
| 1 | **Ace MS-EXE** | 文件归档（SFX） | `libxad/clients/Ace.c:865` | ACE 自解压 EXE |
| 2 | **Ace** | 文件归档 | `Ace.c:880` | ACE 归档（`.ace`） |
| 3 | **Crunch-LZHUF** | 文件归档 | `AMPK.c:2432` | Amiga 上的 Crunch（LZHUF） |
| 4 | **Warp** | 磁盘归档 | `AMPK.c:2448` | Amiga 整盘 |
| 5 | **Arc CBM SFX** | 文件归档（SFX） | `AMPK.c:2464` | Commodore ARC 自解压 |
| 6 | **Arc CBM** | 文件归档 | `AMPK.c:2481` | Commodore ARC |
| 7 | **CompDisk** | 磁盘归档 | `AMPK.c:2498` | Amiga 整盘 |
| 8 | **LhWarp** | 磁盘归档 | `AMPK.c:2514` | Amiga LhA + Warp |
| 9 | **Amiga Plus Unpack** | 文件归档 | `AMPK.c:2531` | |
| 10 | **AmiPack** | 文件归档 | `AMPK.c:2548` | |
| 11 | **CrunchDisk** | 磁盘归档 | `CrunchDisk.c` | Amiga 整盘 |
| 12 | **SDS Software SFX** | 文件归档（SFX） | `DMS.c:1718` | |
| 13 | **DMS SFX** | 磁盘/文件归档（SFX） | `DMS.c:1734` | DiskMasher 自解压 |
| 14 | **DMS** | 磁盘归档 | `DMS.c:1760` | **DiskMasher System**（Amiga `.dms`，最经典的 Amiga 磁盘镜像压缩） |
| 15 | **S-Pack SFX** | 文件归档（SFX） | `IFF-CDAF.c:712` | |
| 16 | **S-Pack** | 文件归档 | `IFF-CDAF.c:728` | |
| 17 | **Shrink** | 文件归档 | `IFF-CDAF.c:744` | |
| 18 | **XPK Archive** | 文件归档 | `IFF-CDAF.c:760` | **XPK**（Amiga 上著名的可插拔压缩库容器） |
| 19 | **Savage Compressor** | 磁盘归档 | `LhA.c:612` | ⚠️ **见下方注 3** |
| 20 | **LhF** | 文件归档 | `LhF.c:573` | Amiga LhF |
| 21 | **MXM-Simple Archive SFX** | 文件归档（SFX） | `MXM-SimpleArc.c` | |
| 22 | **LhSFX** | 文件归档（SFX） | `Zoom.c:1397` | Amiga LhA 自解压 |
| 23 | **S-Omni** | 文件归档 | `Zoom.c:1413` | |
| 24 | **PCompress PACK** | 文件归档 | `Zoom.c:1429` | |
| 25 | **LhPak SFX** | 文件归档（SFX） | `Zoom.c:1445` | |
| 26 | **Zoom 5** | 磁盘归档 | `Zoom.c:1461` | Amiga 整盘 |
| 27 | **Zoom** | 磁盘归档 | `Zoom.c:1485` | Amiga 整盘 |

### 2.2 文件系统解析器（磁盘镜像，8 个）

| # | Client 名 | 文件 | 说明 |
|---|---|---|---|
| 28 | **Amiga Standard FS** | `FS_Amiga.c:604`（`XADCID_FSAMIGA`） | 解析 Amiga FFS/OFS 磁盘镜像（`.adf` `.hdf`），注意 `XADCF_FILESYSTEM` 标志、**无 recog 函数**（`NULL`）—— 这正是 `.adf` 需要后缀旁路的原因 |
| 29 | **Microsoft FAT FS** | `FS_FAT.c:557`（`XADCID_FSFAT`） | 解析 FAT12/16 磁盘镜像，注释给出了完整 boot block 布局（`FS_FAT.c:36-45`），同样**无 recog 函数** |
| 30 | **SanityOS FS** | `FS_SOS.c:134`（`XADCID_FSSANITYOS`） | SanityOS（Amiga 表亲）磁盘，块 0 偏移 16 处有 `'SOS1'`（`FS_SOS.c:37-43`），**无 recog 函数** |

> **注 3（重要缺陷）**：`LhA.c` 虽然文件名叫 LhA、版本串是 `"LhA 1.13 (21.02.2004)"`（`LhA.c:34`），但文件末**只注册了一个 client：`Savage Compressor`**（`LhA.c:612`）。`XADCID_LHA`（`libxad/include/xadmaster.h:915`）在整个仓库里**只有定义、没有任何 client 使用**（全文 grep 仅命中头文件那一行）。
> 也就是说：**这个 vendored libxad fork 里并不存在名为 LhA 的 client** —— LHA 文件由 XADMaster 自己的 `XADLZHParser`（#30）负责。同理 `libxad/clients/Tar.c` 存在但**未编译**。

---

## 3. 支持但**不属于"压缩包"**的格式（按性质分类）

这是判断 unbox 需要哪几个"后端"的关键。XADMaster 把 7 类非压缩包格式复用了同一套嗅探/条目模型：

| 类别 | 格式 | 解析器 / client | 它到底在做什么 |
|---|---|---|---|
| **① 裸压缩流**（单文件，无文件名/无目录概念） | Gzip(#5)、Bzip2(#6)、Compress `.Z`(#21)、XZ(#23)、LZMA_Alone(#58)、PPMd(#19)、LZX-Amiga(#35)、PowerPacker(#36)、Squeeze(#55)、Crunch(#56) | 各自 Parser | 只产出一个无名条目；`XADIsArchiveKey` 由解析器自己置位，供上层决定是否递归 |
| **② 纯归档**（不压缩，只打包） | Tar(#7)、Ar(#40)、Cpio(#59)、WARC(#41) | 各自 Parser | 直接切片，无解压算法 |
| **③ 分片 / 多卷**（既非格式也非压缩） | Split file(#60)、Zip 多卷(#52)、StuffIt split(#11)、RAR 多卷、EmbeddedRAR 多卷 | `XADSplitFileParser` / `XADZipMultiPartParser` / `XADStuffItSplitParser` | **靠文件名正则 + 目录列举 + 真实存在的兄弟文件**聚合成 `CSMultiFileHandle`（`XADArchiveParser.m:842-890`） |
| **④ 可执行文件封装（SFX）** | ZipSFX(#44)、WinZipSFX(#42)、ZipItSEA(#43)、ARCSFX(#54)、CABSFX(#29)、7zSFX(#47)、GzipSFX(#49)、LhSFX×3(#31–#33)、StuffIt5Exe(#10)、NSIS(#48)、libxad 的 Ace MS-EXE / Arc CBM SFX / SDS SFX / DMS SFX / S-Pack SFX / LhSFX / LhPak SFX / MXM-SimpleArc | 各自 SFX Parser | 识别宿主程序特征 → 记录内层偏移 → `seek` 后交给真正的格式解析器。**这是 unbox「伪装包」场景最值钱的一组** |
| **⑤ 磁盘 / 光盘镜像** | ISO 9660 + High Sierra(#61)、NDS(#37) | `XADISO9660Parser` / `XADNDSParser` | 扇区级文件系统解析；ISO 还会动态反推扇区大小（2048 / 2352） |
| **⑥ 文件系统（写在裸设备/镜像里）** | Amiga Standard FS、Microsoft FAT FS、SanityOS FS（libxad #28–#30） | libxad `FS_*` client | **注意：这三个 client 的 `xc_Recog` 是 `NULL`**，即**没有内容识别能力**，只能靠 `XADLibXADParser` 的 `.adf` 后缀旁路进入（`XADLibXADParser.m:47-48`） |
| **⑦ 容器文档 / 宿主格式**（**不是压缩包，只是能从中提取资源**） | PDF(#25)、SWF(#24)、CFBF/OLE2(#28)、RPM(#22) | `XADPDFParser` / `XADSWFParser` / `XADCFBFParser` / `XADRPMParser` | PDF/SWF/CFBF：遍历内部对象，**把图像流重装成独立的 JPEG/TIFF/BMP 输出**（`XADPDFParser.m:60-360`、`XADSWFParser` + `XADSWFTagParser`、`XADNDSParser` 的 `ConvertTiledIconToPNG`）。RPM 是"剥壳交 cpio"的纯封装 |

> **对 unbox 的直接含义**：如果要复刻 unar 的格式覆盖面，后端至少需要 **压缩算法层**（deflate/bzip2/lzma/lzma2/ppmd/lzw/lzss/lh 系/arj/rar/ace/xpk-dms-zoom 等 Amiga 系）、**归档层**（zip/rar/7z/lzh/cpio/ar/tar/stuffit/xar/cab/arj/arc/alz/zoo/lbr/lbr...）、**镜像/文件系统层**（iso9660 含 2352 模式、NDS、FAT、FFS）三套，且**SFX/伪装的识别不能只靠 magic**（见 §4）。

---

## 4. 识别顺序里容易被误判的格式

### 4.1 源码自己承认的"过度热心探测器"（组⑤，故意排最后）

注册表注释 `// Over-eager detectors`（`XADArchiveParser.m:228`）点名 9 个：`ARC`、`ARCSFX`、`Squeeze`、`Crunch`、`LBR`、`LZMA_Alone`、`Cpio`、`SplitFile`、`ISO9660`。

| 风险点 | 为什么危险 | 源码里的缓解手段 |
|---|---|---|
| **`XADLZMAAloneParser`（#58）** | **零 magic**，全靠参数不变量。任何开头 13 字节恰好满足"props<225 + dict 是 2^k 或 3·2^k + size≠0 + 高 2 字节全 0/全 FF"的文件都会被吃下。源码注释直接吐槽文件格式作者 | `bytes[0] >= 225` 上界、dict 的 2^k / 3·2^k 整除判据（乘 `0xaaaaaaab`）、`LE64@5 != 0`、`bytes[11..12]` 全 0 或全 FF，四重交集 |
| **`XADARCParser`（#53）/ `XADARCSFXParser`（#54）** | `1A` 单字节 + 文件名可打印，约束很松；`.COM` 型 SFX 更是靠 `datasize` 反推偏移 | 排除 loader 名 `" unpacker "`、`size<16MB`、下一字节必为 `1A`、压缩分支校验 compsize/uncompsize |
| **`XADSqueezeParser`（#55）/ `XADCrunchParser`（#56）** | `76 FF` / `76 FE`、`76 FD` 仅 2 字节；CP/M 时代格式，现代数据极易撞 | Squeeze：从 `bytes[4]` 起到第一个 0 之间所有字节必须 ≥32。Crunch：0 终止后两字节必须 ∈ [0x10,0x2f] |
| **`XADLBRParser`（#57）** | 128 字节头几乎全是"零/空格约束"，理论上可被构造 | `sectors != 0` + 反向 CRC16（多项式 `0x1021`）双重 |
| **`XADCpioParser`（#59）** | `71 C7` / `C7 71` 两字节太短 | `070707/070701/070702` 六字节 ASCII 才是主判据 |
| **`XADSplitFileParser`（#60）** | **完全不看内容**，只看 `^(.*)\.([0-9]{3})$` + 磁盘上兄弟文件存在 | 无（这是设计选择）。危险在于**任何 `xxx.001` 文件都会被当成归档** |
| **`XADISO9660Parser`（#61）** | 从 `0x8000` 起扫描，任何含 `CD001` 的大文件都可能命中（hdr 高达 512 KiB） | 要求 `block[0]==01 && block[6]==01` 与 `"CD001"` 同时成立；High Sierra 走单独的 `"CDROM1"` 分支 |
| **`XADARJParser`（#51）** | `60 EA` 只有 2 字节，且要**全量扫描** | `size ∈ [32,2600]` + `CRC32(header) == ~stored` 强校验 |
| **`XADCompactProParser`（#50）** | `bytes[0]==1` 一字节 | **遍历整张目录表重算 CRC32**；在不可 seek 的流上退化为只看 `\.(cpt\|sea)` |

### 4.2 只靠文件名 / 后缀的格式（内容完全不看）

| 格式 | 判据 | 后果 |
|---|---|---|
| `XADTarParser`（#7） | `^.*\.tar$` ICASE → 直接 YES | 一个叫 `fake.tar` 的任意文件被当作 tar |
| `XADNSAParser`（#38） | `^arc[0-9]*\.nsa$` ICASE | 内容检查被注释掉（`XADNSAParser.m:39-41`） |
| `XADSARParser`（#39） | `^arc[0-9]*\.sar$` ICASE | 同上 |
| `XADSplitFileParser`（#60） | `^(.*)\.([0-9]{3})$` + 兄弟文件存在 | 见上 |
| `XADZipMultiPartParser`（#52） | 必须含 `.zip` + 必须可 seek | 会抢先于正常 `XADZipParser` 吗？不会——`XADZipParser`(#1) 在其之前；但**一个损坏的 `.zip`** 若开头没有 `PK` 就可能被 #52 从尾部 EOCD 救回（事后看这是好事） |
| `XADCompactProParser`（#50）降级分支 | `\.(cpt\|sea)` ICASE（仅在流不可 seek 时） | 不可 seek 输入下失去内容验证 |
| `XADLibXADParser`（#62）旁路 | `pathExtension == "adf"` → 直接 YES | **任意** `x.adf` 都被当归档 |

### 4.3 先手遮蔽（前者吃掉后者）的现实风险排序

因为主循环是"**第一个返回 YES 的赢**"（`XADArchiveParser.m:288-292`），下表列出**最可能发生遮蔽**的位置：

| 抢先者 | 被遮蔽者 | 触发条件 |
|---|---|---|
| `XADZipParser`(#1) | 一切后面声明的 zip 变体 | 命中 `PK\x03\x04@0` 即赢；但由于它同时是"正确"解析器，实际无害 |
| `XADRARParser`(#2) / `XADRAR5Parser`(#3) | `XADEmbeddedRAR*`(#45/#46) | RAR 在**偏移 0** 时，`XADRARParser` 先赢（合理：偏移 0 的 RAR 优先当普通 RAR）；只有偏移 ≠ 0 时浮动组才出场 |
| `XADGzipParser`(#5) | `XADGzipSFXParser`(#49) | `#!` 开头**同时**是 gzip 的场景不存在（gzip magic 是 `1F 8B`），所以低风险 |
| `XADTarParser`(#7) | `XADISO9660Parser`(#61)、`XADLZMAAloneParser`(#58) 等 | **`.tar` 后缀直接放行**，会跳过所有昂贵探测器。一个既叫 `.tar` 又是 ISO 的文件会被判成 tar |
| `XADBinHexParser`(#13) | 任何后面格式 | 前 8192B 内只要出现 `"(This file must be converted with BinHex"` 字样**且**随后 CRC 自洽即赢。**恰好内嵌了一段合法 BinHex 文本的自解压包会被判成 BinHex** |
| `XADMacBinaryParser`(#14) | 任何 128 字节头像 MacBinary 的格式 | 有 4 级 CRC 约束，风险低 |
| `XADLZHSFXParser` 组(#31–#33) | 后面的格式 | #33 那条 `BE32@6 == "SFX "` + `BE32@10 == "of L"` + `BE32@14 == "Harc"` 也可能在**别的**自解压程序里出现（同一作者的工具链） |
| `XADCABSFXParser`(#29) | `XADLZHSFXParser`(#33) 等 | 它在列表里比 LZH SFX 组**晚**，所以无影响；但 `#29` 会抢先于 `#33`… 不，`#29 < #33`，确认会抢占，只要同时满足 MZ + 长度≥20000 + 能从偏移 8 找到合法 MSCF |
| **浮动签名组内部**（#44/#45/#46） | 同组互相 | 官方仲裁 = "**发现的签名偏移最小者胜**"，缺省 `0x100000`（`XADArchiveParser.m:146-152`）。测试钉死了四个用例：Zip@5000 vs RAR@10000 → **Zip**；RAR@5000 vs Zip@10000 → **RAR**（`XADMasterTests/XADSFXDetectionParserTests.m`） |
| `XADSplitFileParser`(#60) | 无（在很后面） | 但它自己**完全不看内容**，所以对同名序列文件是"总是赢" |

### 4.4 结构性缺陷（源码可证的"已知问题"）

| 问题 | 位置 | 说明 |
|---|---|---|
| **每次嗅探都读 512 KiB** | `XADArchiveParser.m:251-257` + `:354` | `maxheader = max(requiredHeaderSize)`，被 `XADEmbeddedRARParser`(#45) 和 `XADISO9660Parser`(#61) 顶到 `0x80000`。**小文件也付这个代价** |
| **`XADEmbeddedRAR5Parser` 实际只扫 512 KiB** | `XADRAR5Parser.m:83-90` | 它期望扫 1 MiB（`RAR5MaximumSFXHeader = 1<<20`），但 `recognize` 拿到的 `data` 已被 `maxheader`(512 KiB) 截断，且它**没有覆写 `requiredHeaderSize`**（继承父类的 8，对 `maxheader` 无贡献）。**结果是 512 KiB–1 MiB 区间的内嵌 RAR5 签名在嗅探期丢失**（`-parse` 期的 `readUntilSignature` 仍能读到 1 MiB，但那是在识别成功之后） |
| **`7zSFX` 扫描范围作者自己也存疑** | `XAD7ZipParser.m:1029` | `requiredHeaderSize = 0x40000`，注释 `// TODO: Is this enough?` |
| **`XADRARParser` 的 TODO** | `XADRARParser.m:122` | `if(length<7) return NO; // TODO: fix to use correct min size` |
| **`XADRAR5Parser` 同样的 TODO** | `XADRAR5Parser.m:87` | `if(length<8) return RAR5SignatureNotFound; // TODO: fix to use correct min size` |
| **`.adf` 后缀旁路** | `XADLibXADParser.m:47-48` | 因为 libxad 的 `FS_Amiga` client `xc_Recog == NULL`（无识别函数），只能靠后缀。**这直接违反了"不看后缀"的设计原则**，是唯一的例外 |
| **不存在 LhA client** | `libxad/clients/LhA.c:612` | 见 §2 注 3 |
| **CAB / Zoo / XAR / LZH 无加密支持** | `XADCABParser.m` / `XADZooParser.m` / `XADXARParser.m` / `XADLZHParser.m` | 全文 grep 无任何 crypt/password 引用（唯一例外：`XADLZHSFXParsers.m:57-63` 对 Amiga LhA 用**固定** XOR 密钥 `BOA\x0F`） |
| **Mac 专用 / 过时格式占比高** | 组② 全 11 个 + 组⑤ 的 Squeeze/Crunch/LBR | StuffIt 系、BinHex、MacBinary、AppleSingle、DiskDoubler、PackIt、Now Compress、Compact Pro 全部是 macOS 9 时代或更早；CP/M 时代的 Squeeze/Crunch/LBR 除考古外几乎无实用价值 |

---

## 5. 一页速查（源码位置）

```
注册表（62 条，顺序即优先级）        XADArchiveParser.m:167-241
浮动签名组（3 个）                   XADArchiveParser.m:244-249
maxheader 计算                      XADArchiveParser.m:251-257   → 0x80000 (由 EmbeddedRAR + ISO9660 决定)
主嗅探循环                          XADArchiveParser.m:260-295
浮动签名仲裁（最小偏移胜）            XADArchiveParser.m:297-327 + 146-152
异常吞噬                            XADArchiveParser.m:329-339
六个分组注释                        Common / Mac / Less common / Expensive / Over-eager / LibXAD
--- 各格式识别函数（行号）---
Zip                XADZipParser.m:46          RAR              XADRARParser.m:117
RAR5               XADRAR5Parser.m:77         7-Zip            XAD7ZipParser.m:96
Gzip               XADGzipParser.m:33         Bzip2            XADBzip2Parser.m:28
Tar                XADTarParser.m:77          StuffIt          XADStuffItParser.m:41
StuffIt5           XADStuffIt5Parser.m:33     StuffIt5Exe      XADStuffIt5Parser.m:434
StuffItSplit       XADStuffItSplitParser.m:30 StuffItX         XADStuffItXParser.m:282
BinHex             XADBinHexParser.m:30        MacBinary        XADMacBinaryParser.m:30 (→XADMacArchiveParser.m:34)
AppleSingle        XADAppleSingleParser.m:32   DiskDoubler      XADDiskDoublerParser.m:49
PackIt             XADPackItParser.m:33        NowCompress      XADNowCompressParser.m:35
PPMd               XADPPMdParser.m:31          XAR              XADXARParser.m:51
Compress           XADCompressParser.m:29      RPM              XADRPMParser.m:27
XZ                 XADXZParser.m:28            SWF              XADSWFParser.m:32
PDF                XADPDFParser.m:41           ALZip            XADALZipParser.m:62
CAB                XADCABParser.m:62           CFBF             XADCFBFParser.m:51
CABSFX             XADCABParser.m:375          LZH              XADLZHParser.m:34
LZH-AmigaSFX       XADLZHSFXParsers.m:29       LZH-C64SFX       XADLZHSFXParsers.m:76
LZH-SFX            XADLZHSFXParsers.m:105      Zoo              XADZooParser.m:30
LZX                XADLZXParser.m:32           PowerPacker      XADPowerPackerParser.m:31
NDS                XADNDSParser.m:32           NSA              XADNSAParser.m:34
SAR                XADSARParser.m:28           Ar               XADArParser.m:27
WARC               XADWARCParser.m:28          WinZipSFX        XADZipSFXParsers.m:59
ZipItSEA           XADZipSFXParsers.m:85       ZipSFX           XADZipSFXParsers.m:28
EmbeddedRAR        XADRARParser.m:835          EmbeddedRAR5     XADRAR5Parser.m:865（逻辑在 :77）
7zSFX              XAD7ZipParser.m:1032        NSIS             XADNSISParser.m:98
GzipSFX            XADGzipParser.m:139         CompactPro       XADCompactProParser.m:32
ARJ                XADARJParser.m:38           ZipMultiPart     XADZipSFXParsers.m:107
ARC                XADARCParser.m:100          ARCSFX           XADARCParser.m:345
Squeeze            XADSqueezeParser.m:94       Crunch           XADCrunchParser.m:121
LBR                XADLBRParser.m:33           LZMA_Alone       XADLZMAAloneParser.m:28
Cpio               XADCpioParser.m:30          SplitFile        XADSplitFileParser.m:30
ISO9660            XADISO9660Parser.m:62       LibXAD           XADLibXADParser.m:42
--- libxad ---
已编译 client 列表                  libxad/clients.c:51-69
未编译（XPK-only，不支持）           clients.c:54-68（DCS/MDC/PackDev/PackDisk/SuperDuper3/xDisk/xMash）
FS client 无识别函数                 FS_Amiga.c:612 / FS_FAT.c:565 / FS_SOS.c:142（NULL）
```

---

## 6. 未能确定项（诚实声明）

| 项 | 状态 |
|---|---|
| `XADSARParser`（#39）的内部数据布局与是否压缩 | **未能确定**：本文件只有文件名判定 + `:28-36` 之后的内容，未见压缩 handle 引用（`XADNSAParser` 有 `XADNSALZSSHandle`，SAR 没有对应物）。需读 `XADSARParser.m` 全文 200+ 行才能定论 |
| `XADNowCompressParser`（#18）的扩展名 | **未能确定**：源码无扩展名记载，`00 02` 头也无公开文档化的后缀共识（部分资料记 `.nc`，无源码佐证） |
| 各格式的"官方"扩展名映射表 | **未能确定**：XADMaster 源码中**不存在**扩展名注册表（`grep pathExtension` 只命中 4 处，且都是用于给输出条目命名而非识别）。本文扩展名列是行业通用值 |
| `XADCrunchParser`（#56）的标准扩展名 | **未能确定**（源码无，CP/M 时代无统一后缀） |
| libxad 各 client 的 `xc_Recog` 具体判据 | 仅提取了 client 名与 ID；30 个 client 的识别函数体未逐一展开（每个都是独立的 C 文件，工作量等同于再写一份报告） |
| `XADNDSParser` 的 `bytes[0x13] ≤ 7` 语义 | **推断**为"设备类型/容量代码"（DS 卡带容量的 log2 编码），源码未加注释 |
| `XADSWFParser` / `XADPDFParser` 的重装头算法细节 | 已定位到函数名与行号（`CreateTIFFHeaderWithEntries`、`CreateNewJPEGHeaderWithColourProfile`），但未逐行核对位域参数 |
| `libxad/clients/HA.c` / `TR-DOS.c` | 源码注释引用了它们（`clients.c:47-48`，仅 `XAD_GPLCLIENTS` 下编译）但**文件不在仓库中** |

---

*报告结束。所有行号基于当前仓库快照（`/home/user/hermes/yshjy/refs/XADMaster/`），未经修改。*
