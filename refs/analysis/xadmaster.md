# XADMaster（unar / The Unarchiver 引擎）多格式识别与嗅探机制 —— 深度技术分析

> 分析对象：`/home/user/hermes/yshjy/refs/XADMaster/`（Objective-C + C/C++，394 个顶层文件，~50 个格式解析器）
> 版本特征：含 MacPaw Way Ltd. 2017+ 的 SFX 检测重构（`XADSignatureOffset` / 浮动签名组 / `XADSFXDetectionParserTests`）
> 本文所有结论均来自实际源码阅读，附 `文件:行号` 定位。

---

## 0. 概要

XADMaster 的架构是经典的三层分离，**格式识别**与**解压算法**彻底解耦：

```
XADArchive            ← 门面（对用户/CLI）；持有 entries 数组 + unarchiver
  └─ XADArchiveParser ← 抽象基类 + 【全局解析器注册表】 + 【嗅探调度器】
       ├─ XADZipParser / XADRARParser / XAD7ZipParser / ... （~50 个子类）
       └─ CSHandle 家族  ← 可 seek 的统一字节流抽象（文件/内存/子区间/拼接/流式/多卷）
XADUnarchiver / XADSimpleUnarchiver ← 落盘、路径冲突、权限、resource fork 策略
XAD*Handle            ← 纯解压算法（Deflate/LZMA/PPMd/StuffIt/XOR/RC4/AES...）
```

三条互不冲突的入口，全部汇入同一个嗅探器：

| 入口 | 调用点 | 说明 |
|---|---|---|
| 磁盘文件 | `XADArchiveParser.m:406` `archiveParserForPath:` | 额外做**多卷扫描** + **读 macOS resource fork** |
| 内存/任意句柄 | `XADArchiveParser.m:354` `archiveParserForHandle:resourceFork:name:` | 最纯粹的入口 |
| 归档内某个实体 | `XADArchiveParser.m:483` `archiveParserForEntryWithDictionary:resourceForkDictionary:archiveParser:wantChecksum:` | 支持**嵌套归档 / 归档套归档**，并继承父级密码与文件名编码 |

**一句话总结设计哲学：**
> 一张有序的解析器表 + 每个解析器自己实现的 `recognizeFileWithHandle:` 布尔函数 + **廉价的头 magic 检查排在前面、昂贵的全文件扫描排在后面** + **一组"浮动签名"解析器用"签名最小偏移优先"仲裁** + **识别期异常一律吞掉当作"不匹配"**。

---

## 1. 格式识别分流机制（核心）

### 1.1 注册表：一切都从这张有序表开始

`XADArchiveParser.m:156-258` `+[XADArchiveParser initialize]`

```objc
static NSArray *parserclasses;                      // 主注册表（有序！顺序即优先级）
static NSArray *parsersWithFloatingSignaturesClasses; // 浮动签名子集
static int maxheader = 0;                            // 全表 requiredHeaderSize 的最大值
```

表的顺序在源码里被**显式注释分组**，这是整个设计的骨架：

```objc
parserclasses = @[
  // ── Common formats ─────────────────────────────────
  XADZipParser, XADRARParser, XADRAR5Parser, XAD7ZipParser,
  XADGzipParser, XADBzip2Parser, XADTarParser,

  // ── Mac formats ────────────────────────────────────
  XADStuffItParser, XADStuffIt5Parser, XADStuffIt5ExeParser, XADStuffItSplitParser,
  XADStuffItXParser, XADBinHexParser, XADMacBinaryParser, XADAppleSingleParser,
  XADDiskDoublerParser, XADPackItParser, XADNowCompressParser,

  // ── Less common formats ────────────────────────────
  XADPPMdParser, XADXARParser, XADCompressParser, XADRPMParser, XADXZParser,
  XADSWFParser, XADPDFParser, XADALZipParser, XADCABParser, XADCFBFParser,
  XADCABSFXParser, XADLZHParser, XADLZHAmigaSFXParser, XADLZHCommodore64SFXParser,
  XADLZHSFXParser, XADZooParser, XADLZXParser, XADPowerPackerParser, XADNDSParser,
  XADNSAParser, XADSARParser, XADArParser, XADWARCParser,

  // ── Detectors that require lots of work ─────────────  ← 需要大量 IO / 全量扫描
  XADWinZipSFXParser, XADZipItSEAParser, XADZipSFXParser, XADEmbeddedRARParser,
  XADEmbeddedRAR5Parser, XAD7ZipSFXParser, XADNSISParser, XADGzipSFXParser,
  XADCompactProParser, XADARJParser, XADZipMultiPartParser,

  // ── Over-eager detectors ────────────────────────────  ← 容易误报，排最后
  XADARCParser, XADARCSFXParser, XADSqueezeParser, XADCrunchParser, XADLBRParser,
  XADLZMAAloneParser, XADCpioParser, XADSplitFileParser, XADISO9660Parser,

  // ── LibXAD ─────────────────────────────────────────  ← 外部 Amiga 生态兜底
  XADLibXADParser,
];

parsersWithFloatingSignaturesClasses = @[
  XADZipSFXParser, XADEmbeddedRARParser, XADEmbeddedRAR5Parser
];
// 注释原文：These classes can detect themselves incorrectly if one archive is placed in another
```

`maxheader` 在同一函数里由所有类的 `+requiredHeaderSize` 求最大值算出（`XADArchiveParser.m:251-257`）。
实测最大值 = **0x80000 = 512 KiB**（`XADEmbeddedRARParser` 与 `XADISO9660Parser`），也就是**任何一次嗅探都会先把前 512KB 读进内存**交给所有候选解析器。

### 1.2 主循环（伪代码）

`XADArchiveParser.m:260-295` `+archiveParserClassForHandle:firstBytes:resourceFork:name:propertiesToAdd:`

```
function sniff(handle, header /* 前 512KB */, name, out props) -> Option<Class>:
    floating_done = false

    for cls in parserclasses:                      # 顺序即优先级
        handle.seek(0)
        props.clear()                              # 每个候选都在干净的属性袋里跑

        if cls in parsersWithFloatingSignatures:   # 浮动签名：整组单独评估一次
            if not floating_done:
                floating_done = true
                c = arbitrate_floating_group()     # 见 1.3
                if c:
                    handle.seek(0)
                    return c
            continue

        if try_recognize(cls):                     # 见 1.4
            handle.seek(0)
            return cls                             # 第一个返回 YES 的赢

    return None                                    # 全部失败 → XADDataFormatError
```

关键点：
- **`props.clear()`**：候选解析器可以往属性袋里塞"我发现的偏移量"（如 `RAREmbedOffset`），失败则被清空，不会污染下一个候选。
- **`handle.seek(0)` 在每次调用前后都做**：解析器可以任意乱读乱 seek，框架兜底复位。
- **首次匹配即胜出**，没有打分/投票。极少数情况靠"把更容易误报的放最后"来兜。

### 1.3 浮动签名组的仲裁（"归档被拼接在别的文件之后"的正式解法）

`XADArchiveParser.m:297-327` `+archiveParserFromParsersWithFloatingSignature:...`
`XADArchiveParser.m:146-152` `CompareParserSignaturesLocations`

```
function arbitrate_floating_group(handle, header, name, out props) -> Option<Class>:
    valid = []
    for cls in {ZipSFX, EmbeddedRAR, EmbeddedRAR5}:
        handle.seek(0)
        p = {}                                    # 独立属性袋
        if try_recognize(cls, into: p):           # 这一步会写入 XADSignatureOffset
            p["XADParserClass"] = cls
            valid.append(p)

    if valid.is_empty(): return None

    sort valid by (p["XADFoundSignatureOffset"] ?? 0x100000)   # 默认 1MB
    props.merge(valid[0])                          # 只有赢家的属性被保留
    return valid[0]["XADParserClass"]
```

**语义：谁发现的内嵌归档签名在文件里靠前，谁就是真正的归档。**
`XADSignatureOffset` 的常量名是字面量 `@"XADFoundSignatureOffset"`（`XADArchiveParser.m:143`）。

这条规则被单元测试直接钉死 —— `XADMasterTests/XADSFXDetectionParserTests.m`：

| 测试 | 构造 | 期望 |
|---|---|---|
| `testRARSFXArchiveDetected` | 偏移 1000 处放 `Rar!\x1a\x07\x00` | → `XADEmbeddedRARParser` |
| `testZipSFXArchiveDetected` | MZ + 偏移 2000 处放 `PK\x03\x04` | → `XADZipSFXParser` |
| `testZipRarSignatureDetectedAsZip` | Zip 签名@5000，RAR 签名@10000 | → **Zip** |
| `testRarZipSignatureDetectedAsRar` | RAR 签名@5000，Zip 签名@10000 | → **RAR** |

### 1.4 异常吞噬式嗅探

`XADArchiveParser.m:329-339`

```objc
+ (BOOL)isValidParserClass:(Class)parserClass forHandle:(CSHandle *)handle
                firstBytes:(NSData *)header name:(NSString *)name
           propertiesToAdd:(NSMutableDictionary *)props
{
    @try {
        if ([parserClass recognizeFileWithHandle:handle firstBytes:header name:name propertiesToAdd:props])
        { [handle seekToFileOffset:0]; return YES; }
    } @catch (id e) {
    } // ignore parsers that throw errors on recognition or init
    return NO;
}
```

**任何解析器在识别阶段抛出的异常（越界读、CRC 不符、`@throw @"Invalid PDF structure"`）都被静默视为"我不是"。**
这带来一个非常实用的性质：识别器可以写得"贪心而脆弱"（例如 BinHex、Compact Pro 会真的去解一小段并校验 CRC），失败路径完全靠异常处理，不必到处写边界判断。

### 1.5 路径入口独有的两件事

`XADArchiveParser.m:406-460` `+archiveParserForPath:`

1. **读 macOS resource fork**：`XADPlatform handleForReadingResourceForkAtPath:` → `XADResourceFork`。
   在 Linux 平台上这是空实现（`XADPlatformLinux.m:227` 直接 `return nil`），所以 Linux 下相当于没有 —— 移植到 Rust 时可以整块忽略，但要保留"entry 可能只有 resource fork"的数据模型。
2. **多卷扫描**：识别出 Class 后立刻调 `+[parserclass volumesForHandle:firstBytes:name:]`：

```
volumes = cls.volumesForHandle(handle, header, filename)
if volumes.count > 1:
    handle = CSMultiFileHandle(volumes)          # 多文件拼成一个虚拟字节流
    return new cls with handle
elif volumes is empty array:
    props["XADVolumeScanningFailed"] = YES       # 空数组 = 扫描失败，退回单文件模式
# nil / 异常 → 静默退回单文件模式
```

卷扫描本身是**文件系统目录列举 + 正则 + 排序**（`XADArchiveParser.m:842-890`）：

```objc
+scanForVolumesWithFilename:regex:firstFileExtension:
    // 列出同目录所有文件 → 正则过滤 → 排序
static XADVolumeSort(id e1, id e2, void *firstext)
    // 命中 firstFileExtension 的排最前（NSCaseInsensitive | NSBackwardsSearch | NSAnchored）
```

`firstFileExtension` 的典型用法是 `@"z01"`：保证 `.z01` 排在 `.zip` 前面（多卷 ZIP 的第一片是 `.z01`）。

### 1.6 嵌套归档：识别器是递归的

`XADArchiveParser.m:470-512`

```objc
+archiveParserForEntryWithDictionary:...archiveParser:parser wantChecksum:checksum
{
    handle = [parser handleForEntryWithDictionary:entry];   // 取子实体流
    subparser = [XADArchiveParser archiveParserForHandle:handle name:filename];  // ← 递归嗅探
    if([parser hasPassword]) [subparser setPassword:[parser password]];           // 密码下传
    if([[parser stringSource] hasFixedEncoding]) [subparser setEncodingName:...]; // 文件名编码下传
    return subparser;
}
```

`XADArchive.m:45-61` `+recursiveArchiveForFile:` 更激进 —— **只要归档里只有 1 个 entry，就不断往下钻**：

```objc
while([archive numberOfEntries]==1) {
    sub = [[XADArchive alloc] initWithArchive:archive entry:0];
    if(sub) archive = sub; else break;
}
```

这是 `unar` 能自动剥掉"tar.gz 外面还套了一层"的原因，也是对抗"改后缀"的关键：**完全不看文件名，只递归识别内容**。

---

## 2. 格式签名表

> 「判定条件」列全部摘录自各 `+recognizeFileWithHandle:` 实现。
> `hdr` = `+requiredHeaderSize`（静态返回值；多行的已实测合并）。

### 2.1 常规格式（按注册表顺序）

| # | 格式 | 解析器类 | 判定条件（magic / 启发式） | hdr |
|---|---|---|---|---|
| 1 | **Zip** | `XADZipParser` | `PK\x03\x04`@0 **或** `PK\x05\x06`@0 **或** `PK\x03\x04`@**4** | 8 |
| 2 | **RAR 1.5–4.x** | `XADRARParser` | `Rar!\x1a\x07\x00`@0 **或** 远古签名 `52 45 7E 5E`@0 | 7 |
| 3 | **RAR 5** | `XADRAR5Parser` | `Rar!\x1a\x07\x01\x00`，**在前 1MiB 内任意偏移** | 8 |
| 4 | **7-Zip** | `XAD7ZipParser` | `37 7A BC AF 27 1C 00`（7 字节） | 32 |
| 5 | **Gzip** | `XADGzipParser` | `1F 8B 08` **或** `1F 9E 08` **或** `1F A1`（BSD compress 变体） | 3 |
| 6 | **Bzip2** | `XADBzip2Parser` | `"BZh"[1-9]` + 6 字节块 magic ∈ {`31 41 59 26 53 59`, `17 72 45 38 50 90`} | 10 |
| 7 | **Tar** | `XADTarParser` | 名匹配 `^.*\.tar$`（ICASE）**或** 512B 头解析结果 ≠ `TAR_FORMAT_V7` | 512 |
| 8 | **StuffIt** | `XADStuffItParser` | `bytes[10..13] == 72 4C 61 75`("rlau") 且 (`0x53495421`=`"SIT!"` **或** `"STi n/0-9"` / `"ST 0-9 0-9"` 安装包变体) | 22 |
| 9 | **StuffIt 5** | `XADStuffIt5Parser` | 头部字面串 `"StuffIt (c)1997-\xFF\xFF\xFF\xFF Aladdin Systems, Inc., http://www.aladdinsys.com/StuffIt/\r\n"` | 100 |
| 10 | **StuffIt 5 EXE** | `XADStuffIt5ExeParser` | `MZ` @0 **且** `CSUInt32BE(bytes+4100) == 0x4203E853` | 8192 |
| 11 | **StuffIt split** | `XADStuffItSplitParser` | `B0 56 00` + `bytes[4]∈[1,63]` 且文件名可打印 | 100 |
| 12 | **StuffIt X** | `XADStuffItXParser` | `"StuffIt!"` **或** `"StuffIt?"` | 10 |
| 13 | **BinHex** | `XADBinHexParser` | 前 8192B **扫描** 字面串 `"(This file must be converted with BinHex"`，然后跳到 `:` 起始，真解一小段并校验 **CRC16** | 8192 |
| 14 | **MacBinary** | `XADMacBinaryParser` | 委托 `XADMacArchiveParser +macBinaryVersionForHeader:`（见 2.3） | 128 |
| 15 | **AppleSingle/Double** | `XADAppleSingleParser` | `00051600`/`00051607`/`00160500`/`07160500` @0 **且** `00020000`/`00000200` @4 | 8 |
| 16 | **DiskDoubler** | `XADDiskDoublerParser` | `AB CD 00 54` + CRC16@82 **或** `"DDAR"` + CRC16@76 **或** `"DDA2"` + CRC16@60 | 124 |
| 17 | **PackIt** | `XADPackItParser` | `"PMa"` + `[g456]` | 4 |
| 18 | **NowCompress** | `XADNowCompressParser` | `00 02` @0 + 文件数∈[1,65535] + 名长∈[1,31] 全可打印 + **字节 [24,130) 求和校验** == BE32@130 | 24 |
| 19 | **PPMd** | `XADPPMdParser` | `84 AC AF 8F` @0 **或** 反向 `8F AF AC 84` | 16 |
| 20 | **XAR** | `XADXARParser` | `"xar!"` | 4 |
| 21 | **Compress (.Z)** | `XADCompressParser` | `1F 9D` | 3 |
| 22 | **RPM** | `XADRPMParser` | `ED AB EE DB` | 96 |
| 23 | **XZ** | `XADXZParser` | `FD 37 7A 58 5A 00`（源码注释里保留了被禁用的旧 LZMA-magic 分支） | 6 |
| 24 | **SWF** | `XADSWFParser` | `"FWS"` **或** `"CWS"`，且 `bytes[3] < 32`（版本上限启发式） | 6 |
| 25 | **PDF** | `XADPDFParser` | `"%PDF-"`，且长度 ≥ 53 | 53 |
| 26 | **ALZip** | `XADALZipParser` | `"ALZ"` + `01` + `bytes[7]==0` | 8 |
| 27 | **CAB** | `XADCABParser` | `"MSCF"` | 4 |
| 28 | **CFB / OLE2**（doc/xls/msi） | `XADCFBFParser` | `D0 CF 11 E0 A1 B1 1A E1` + `bytes[28..29]==FE FF`（版本号，额外保险） | 512 |
| 29 | **LZH / LArc / PMarc** | `XADLZHParser` | `bytes[2]=='-'`，`bytes[3..4]` ∈ {`lh`,`lz`,`pm`}，`bytes[6]=='-'`，`bytes[5]` ∈ {`0..7`,`d`} | 7 |
| 30 | **Zoo** | `XADZooParser` | **偏移 0x14 处** == `DC A7 C4 FD`（非 0 偏移 magic） | 0x22 |
| 31 | **LZX** | `XADLZXParser` | `"LZX"`（仅 3 字节，弱） | 10 |
| 32 | **PowerPacker** | `XADPowerPackerParser` | `"PP20"` | 4 |
| 33 | **NDS** | `XADNDSParser` | 字段约束 + **两段 CRC16**：CRC16(0xffff, [0xc0,0x15c)) == BE16@0x15c 且 CRC16(0xffff, [0,0x15e)) == BE16@0x15e | 0x200 |
| 34 | **NSA** | `XADNSAParser` | **只看文件名** `^arc[0-9]*\.nsa$`（ICASE），内容不检查 | 6 |
| 35 | **SAR** | `XADSARParser` | **只看文件名** `^arc[0-9]*\.sar$`（ICASE），内容不检查 | 6 |
| 36 | **Ar** | `XADArParser` | `"!<arch>\n"` | 6 |
| 37 | **WARC** | `XADWARCParser` | `"WARC/1.0\r\n"` 或 `"WARC/1.1\r\n"` | 10 |
| 38 | **NSIS 安装包** | `XADNSISParser` | **512 字节步长扫描**三种 NSIS 签名，命中则记下 `NSISOlderOffset`/`NSISOldOffset`/`NSISNewOffset` | 0x10000 |
| 39 | **Compact Pro** | `XADCompactProParser` | `bytes[0]==1` + 目录偏移 `BE32@4` + **遍历目录表重算 CRC32 并比对** | 8 |
| 40 | **ARJ** | `XADARJParser` | **全量扫描** `60 EA` + 头长 ∈[32,2600] + `CRC32(头)==~BE32@(i+4+size)` | 0x8000 |
| 41 | **ARC** | `XADARCParser` | `0x1A`@0 + 合法 ARC 文件名 + 布局自洽（含 loader 名判定） | 0x1d |
| 42 | **Squeeze** | `XADSqueezeParser` | `76 FF` + 文件名首字节非 0、后续全 ≥32，遇 0 终止 | 1024 |
| 43 | **Crunch** | `XADCrunchParser` | `76 FE`/`76 FD` + 文件名后跟两个字节 ∈[0x10,0x2f]（日期数字） | 1024 |
| 44 | **LBR** | `XADLBRParser` | 128B 头字段零/空格约束 + sectors≠0 + **反向 CRC16（多项式 0x1021）** | 128 |
| 45 | **LZMA_Alone** | `XADLZMAAloneParser` | **无 magic**：props<225 且 dict 为 2^k 或 3·2^k 且 uncompressedSize≠0 且高两字节 ∈{0x0000,0xFFFF} | 13 |
| 46 | **Cpio** | `XADCpioParser` | `"070707"`/`"070701"`/`"070702"` **或** `71 C7` / `C7 71`（结尾字节序标记） | 6 |
| 47 | **Split file** | `XADSplitFileParser` | **只看文件名** `^(.*)\.([0-9]{3})$` **且**磁盘上 `.001`/`.002` 中另一个也存在 | 0 |
| 48 | **ISO 9660** | `XADISO9660Parser` | 从 0x8000 扫描主卷描述符，再动态探块大小（见 4.5） | 0x80000 |
| 49 | **LibXAD** | `XADLibXADParser` | 委托外部 libxad `xadRecogFile()`；**`.adf` 后缀兜底返回 YES** | 动态 |

### 2.2 自解压 / 内嵌（SFX / Embedded）—— 这一组才是"伪装包"的正解

| 解析器类 | 外层特征 | 内层签名搜索 | 记录偏移属性 | 浮动组 |
|---|---|---|---|---|
| `XADZipSFXParser` | `MZ`@0 | 全 header 扫 `PK\x03\x04`，**且** `bytes[i+4]∈[10,40)`（version-needed 低位）**且** `bytes[i+9]==0`（压缩方法高位） | `XADSignatureOffset` | ✅ |
| `XADEmbeddedRARParser` | 无要求 | 前 **0x80000** 扫 `Rar!\x1a\x07\x00` | `RAREmbedOffset` + `XADSignatureOffset` | ✅ |
| `XADEmbeddedRAR5Parser` | 无要求 | 前 **1MiB** 扫 `Rar!\x1a\x07\x01\x00` | `XADSignatureOffset` | ✅ |
| `XADWinZipSFXParser` | `MZ`@0 | 前 20480B 扫字面串 `"WinZip(R) Self-Extractor"`（24B） | — | ❌ |
| `XADZipItSEAParser` | — | `"Joy!"`（BE32）@0 | — | ❌ |
| `XADZipMultiPartParser` | 必须 **`.zip` 文件名** + **可 seek 的 `CSFileHandle`** | 从尾部 0x10011 B **反向**扫 `PK\x05\x06` | — | ❌ |
| `XADARCSFXParser` | 三种 `.COM`/`.EXE` 布局 | 计算第一条目数据长度推出归档起点 | `ARCSFXOffset` | ❌ |
| `XAD7ZipSFXParser` | `MZ`@0 | **512 字节步长**扫描 7z 签名 | `7zSFXOffset` | ❌ |
| `XADGzipSFXParser` | `"#!"`@0 | 前 **5000** B 扫 `1F 8B 08` | `GzipSFXOffset` | ❌ |
| `XADCABSFXParser` | `MZ`@0 且长度≥20000 | 从**偏移 8 起**逐字节扫 `"MSCF"` + `MatchCABSignature` 结构校验 | `CABSFXOffset` | ❌ |
| `XADLZHAmigaSFXParser` | — | `BE32@(11*4) == 0x53465821`（`"SFX!"`） | — | ❌ |
| `XADLZHCommodore64SFXParser` | — | 固定偏移复合特征（`@0==01`、`@0xd44=='LHA'`、`@0xe8b=='-lh-'`） | — | ❌ |
| `XADLZHSFXParser` | — | 4 组固定偏移字面串（`"LH"`、`"SF s"`、`"LZSS sel"`、`"SFX "` + `"of L"` + `"Harc"` …） | — | ❌ |

**关键发现：所有 SFX 解析器在 `-parse` 里都做同一件事 —— 先 seek 到自己记录的偏移，再 `[super parse]`。**

```objc
// XADGzipParser.m:162-168 / XADCABParser.m:397-403 / XAD7ZipParser.m:1053-1059 / XADRARParser.m:882-888
-(void)parse {
    off_t offs = [[[self properties] objectForKey:@"GzipSFXOffset"] longLongValue];
    [[self handle] seekToFileOffset:offs];
    [super parse];
}
```

`XADRAR5Parser` 略有不同，它不信任"识别期算出的偏移"，在 `-parse` 时**重新搜索一次**（`XADRAR5Parser.m:144-155` `readUntilSignature`），失败就抛 `RAR5SignatureCannotBeFound`。

### 2.3 MacBinary 的两级"怀疑"机制（很值得抄的容错模式）

`XADMacArchiveParser.m:34-76` `+macBinaryVersionForHeader:`

```
1. 零填充字节约束：bytes[0]==0, [74]==0, [82]==0, [108..115] 全 0
2. 合法名字：bytes[1] ∈ [1,63] 且 name 区不含 0
3. 校验和 A：XADCalculateCRC(0, bytes, 124, crc_reverse_0x1021) == XADUnReverseCRC16(BE16@124)
   校验和 B（兼容某些工具）：XADCalculateCRC(65535, bytes,124, crc_a001) ^ 65535 == BE16@124
4. 签名：BE32@102 == 'mBIN' → 返回 3 (MacBinary III)，否则 2 (MacBinary II)
5. 都不满足 → 退化为"启发式 v1"：额外零约束 + 两个 fork size < 0x80000000 + 两个日期非 0
```

而**调用方**分两级"怀疑度"（`XADMacArchiveParser.m:360-380`）：

```objc
NSNumber *isbinobj = dict[XADIsMacBinaryKey];   // 已知是 MacBinary（例如文件名叫 .bin）
NSNumber *checkobj = dict[XADMightBeMacBinaryKey]; // 只"怀疑"（例如 ZIP 里条目名以 ".bin" 结尾）
if(!isbin && !check) return NO;
if(!isbin && [handle isKindOfClass:[CSStreamHandle class]]) return NO;  // 不可 seek 的流不验证
header = [fh readDataOfLengthAtMost:128];
if([header length]!=128) return NO;
if(!isbin && [XADMacArchiveParser macBinaryVersionForHeader:header]==0) return NO;
```

"怀疑"标记由其他解析器顺手打上 —— `XADZipParser.m:1036-1041`（条目名以 `.bin` 结尾）、`XADLZHParser.m:240`：

```objc
if(namelength>4) if(memcmp(namebytes+namelength-4,".bin",4)==0)
    [dict setObject:[NSNumber numberWithBool:YES] forKey:XADMightBeMacBinaryKey];
```

**这是"便宜线索 → 昂贵验证"的教科书实现**：扫描 ZIP 条目名几乎零成本，就先埋一个 flag；真正读 128 字节头 + 算 CRC 才在需要时做。

---

## 3. 关键源码位置表

### 3.1 识别 / 调度骨架

| 位置 | 类 / 函数 | 作用 |
|---|---|---|
| `XADArchiveParser.m:143-144` | `XADSignatureOffset`(=`XADFoundSignatureOffset`) / `XADParserClass` | 浮动签名偏移、仲裁用的类键 |
| `XADArchiveParser.m:146-152` | `CompareParserSignaturesLocations` | 按偏移升序比较，缺省 `0x100000` |
| `XADArchiveParser.m:156-258` | `+initialize` / `parserclasses` / `maxheader` | **全局注册表 + 表顺序 + maxheader 计算** |
| `XADArchiveParser.m:260-295` | `+archiveParserClassForHandle:firstBytes:resourceFork:name:propertiesToAdd:` | **主嗅探循环** |
| `XADArchiveParser.m:297-327` | `+archiveParserFromParsersWithFloatingSignature:...` | **浮动签名仲裁（最小偏移优先）** |
| `XADArchiveParser.m:329-339` | `+isValidParserClass:...` | **异常吞噬**的识别包装 |
| `XADArchiveParser.m:354-358` | `+archiveParserForHandle:resourceFork:name:` | 读 `maxheader` 字节头后进入嗅探 |
| `XADArchiveParser.m:406-460` | `+archiveParserForPath:` | 路径入口：resource fork + 多卷扫描 |
| `XADArchiveParser.m:470-512` | `+archiveParserForEntryWithDictionary:...` | **递归子归档** + 密码/编码下传 |
| `XADArchiveParser.m:842-890` | `XADVolumeSort` / `+scanForVolumesWithFilename:regex:firstFileExtension:` | 卷扫描 = 列目录 + 正则 + 排序 |
| `XADArchiveParser.m:905-924` | `-handleAtDataOffsetForDictionary:` | **`XADSkipOffsetKey` vs `XADDataOffsetKey` 两条取数路径** |
| `XADArchiveParser.m:1235-1250` | `-encodedPassword` / `-encodedCStringPassword` | 密码按 parser 的编码转换 |
| `XADArchiveParser.m:1254-1263` | `-reportInterestingFileWithReason:` | **"异常/可疑文件"上报钩子**（fuzzing 埋点） |
| `XADArchiveParser.m:1268-1280` | `+requiredHeaderSize` / `+recognizeFileWithHandle:firstBytes:name:` / `+volumesForHandle:...` | 子类必须实现的三个识别期接口（默认 `0` / `NO` / `nil`） |
| `XADArchiveParser.h:237-251` | `// Subclasses implement these:` | 子类契约的权威定义 |
| `XADArchive.m:101,126,154` | `archiveParserForPath:` / `ForHandle:` / `ForEntryWithDictionary:` | 门面的三个嗅探触发点 |
| `XADArchive.m:45-61` | `+recursiveArchiveForFile:` | **单条目自动递归下钻** |
| `XADArchive.m:790-793` | `extractEntries:...` | 子归档解压失败 → **退回当普通文件解** |

### 3.2 ZIP

| 位置 | 函数 | 作用 |
|---|---|---|
| `XADZipParser.m:44-58` | `+requiredHeaderSize` / `+recognizeFileWithHandle:` | 3 条 magic（含 `@4` 的怪例） |
| `XADZipParser.m:60-97` | `+volumesForHandle:...` | `.z01` 多卷 / `.zip.001` 分卷 / `.1.zip` 三种命名 |
| `XADZipParser.m:118-191` | `-findCentralDirectoryRecordOffset:zip64Offset:` | **分块反向扫 EOCD + Zip64 locator 探测** |
| `XADZipParser.m:194-233` | `-parseWithSeparateMacForks` | 无 EOCD / >4GB 无 zip64 的降级决策 |
| `XADZipParser.m:239-372` | `-parseWithCentralDirectoryAtOffset:zip64Offset:` | 中央目录解析（含 Zip64 覆写、条目溢出补丁） |
| `XADZipParser.m:307-310` | numentries 溢出补丁 | **>65535 条目未用 Zip64 的容错** |
| `XADZipParser.m:313-363` | local header 校验 | `localid==0x504B0506` 的 "kludge for strange archives"；失败置 `XADIsCorruptedKey` |
| `XADZipParser.m:374-...` | `-readCentralDirectoryRecord` | 单条中央目录记录解析 |
| `XADZipParser.m:460-586` | `-parseWithoutCentralDirectory` | **无 EOCD 时的线性本地头扫描 + 重同步** |
| `XADZipParser.m:590-617` / `619-654` | `MatchZipDataDescriptor` / `MatchZip64DataDescriptor` | **数据描述符（flag 0x08）搜索匹配器** |
| `XADZipParser.m:656-682` | `-findEndOfStreamMarkerWithZip64Flag:...` | 用上面的匹配器找流末尾 |
| `XADZipParser.m:686-701` | `MatchZipEntry` / `-findNextEntry` | **损坏后重同步**：扫下一个 `PK` 头 |
| `XADZipParser.m:703-...` | `-parseZipExtraOrNilWithLength:...` | extra 字段容错解析（异常→置 corrupt + 日志） |
| `XADZipParser.m:1130-1191` | `-rawHandleForEntryWithDictionary:wantChecksum:` | 加密分发（ZipCrypto / WinZipAES）+ 校验包装 |
| `XADZipParser.m:1212-1257` | `-decompressionHandleWithHandle:method:flags:size:` | **压缩方法 → Handle 的路由表**（switch 在 :1214-1255） |

### 3.3 其他格式（重点标注"非零偏移"逻辑）

| 位置 | 作用 |
|---|---|
| `XADZipSFXParsers.m:26-47` | ZipSFX：MZ + 扫 PK + 版本/方法合理性；写 `XADSignatureOffset` |
| `XADZipSFXParsers.m:103-135` | ZipMultiPart：**从尾部反向扫 EOCD** |
| `XADZipSFXParsers.m:137-154` | ZipMultiPart 的卷扫描（两种命名正则，`z01` 优先） |
| `XADRARParser.m:88-97` | `IsRARSignature` / `IsAncientRARSignature` |
| `XADRARParser.m:99-107` | `FindSignature`（线性扫描） |
| `XADRARParser.m:112-124` | RAR 识别（**只认偏移 0**） |
| `XADRARParser.m:830-905` | `XADEmbeddedRARParser`：0x80000 扫描 + `RAREmbedOffset` + `volumesForHandle` 多卷 |
| `XADRAR5Parser.m:77-99` | `signatureLocationInData:` **前 1MiB 全扫** |
| `XADRAR5Parser.m:144-155` | `readUntilSignature`：parse 期**重新定位**签名 |
| `XAD7ZipParser.m:1027-1060` | 7zSFX：MZ + **512 步长**扫描 + `7zSFXOffset` |
| `XADGzipParser.m:137-172` | GzipSFX：`#!` + 5000B 内扫 + `GzipSFXOffset` |
| `XADCABParser.m:373-403` | CABSFX：MZ + 逐字节扫 MSCF + `CABSFXOffset` |
| `XADARCParser.m:343-...` | ARCSFX：三种宿主布局 + `ARCSFXOffset` |
| `XADNSISParser.m:96-125` | NSIS：512 步长扫三种签名 + `NSIS*Offset` |
| `XADISO9660Parser.m:62-105` | ISO9660：0x8000 起扫描 + **动态推导扇区大小** |
| `XADBinHexParser.m:30-62` | BinHex：扫文本头 + 真解一段 + CRC16 |
| `XADMacArchiveParser.m:34-76` | `macBinaryVersionForHeader:` 四级判定 |
| `XADMacArchiveParser.m:360-430` | 两级"怀疑"（`XADIsMacBinaryKey` / `XADMightBeMacBinaryKey`）+ 拆数据/资源叉 |
| `XADLZMAAloneParser.m:28-46` | **无 magic 的纯启发式识别**（源码吐槽注释在此） |
| `XADPDFParser.m:41-55` | PDF 识别（弱：只查 `%PDF-`） |
| `XADPDFParser.m:60-...` | PDF 当容器用：抽 image stream 并**重装 JPEG/TIFF 头** |
| `PDF/PDFParser.m:192-202` | `proceedAssumingCharacter:errorMessage:` **强消费字符的容错解析原语** |
| `PDF/PDFParser.m:210-235` | 尾部找 `startxref …\n%%EOF` + 沿 `/Prev` 链回溯 |
| `PDF/PDFParser.m:237-241` | xref 表（老）与 xref 流（PDF 1.5+）双路分发 |
| `XADSplitFileParser.m:30-...` | 分卷识别 = **文件存在性探测** |
| `XADLibXADParser.m:35-60` | 外部 libxad 兜底 + `.adf` 后缀特判 |
| `XADArchiveParserDescriptions.m:99,172` | `XADIsArchiveKey` 等属性的可读化描述表 |

---

## 4. 「数据不在文件开头」的完整处理链条

这是问得最准的一点：XADMaster 有**至少五套独立机制**应对，且分层使用。

### 4.1 机制 A：浮动签名 + `XADSignatureOffset`（通用、可仲裁）

只覆盖 Zip/RAR/RAR5 三种"最可能被包成 SFX"的格式。工作流：

```
recognize 阶段：在 header 缓冲里线性扫描内层签名
              → 命中则 props["XADFoundSignatureOffset"] = i
              → 该解析器进入浮动组仲裁（最小偏移胜）
parse   阶段：seek(i) 后当作正常归档解析
```

性质：**偏移只是"发现位置"，不修改 handle 内容**；上层看到的仍然是一个从 0 开始的字节流。

### 4.2 机制 B：每个 SFX 解析器自己的私有偏移属性

`ARCSFXOffset` / `7zSFXOffset` / `GzipSFXOffset` / `CABSFXOffset` / `RAREmbedOffset` / `NSISOlderOffset` / `NSISOldOffset` / `NSISNewOffset`。

它们**不参与仲裁**（因为外层特征已经足够唯一：`MZ`、`#!`、`B0 56 00`…），只是把"内层在哪"这个信息从"识别期"传到"解析期"。`-parse` 开头统一 `seek` 过去。

`ISO9660ImageBlockSize` / `ISO9660ImageBlockOffset` / `ISO9660ImageIsHighSierra` 同属这一类。

> **教训：偏移这件事，识别器与解析器之间的契约需要显式承载。** XADMaster 用"属性袋"实现，代价是字符串键 + 弱类型。

### 4.3 机制 C：RAR5 的自搜索（不依赖识别期结果）

`XADRAR5Parser.m:83-99` 识别期扫一次；`XADRAR5Parser.m:144-155` 解析期**再扫一次**：

```objc
- (void)readUntilSignature {
    CSHandle *h = [[self handle] subHandleToEndOfFileFrom:0];
    NSData *data = [h readDataOfLengthAtMost:RAR5MaximumSFXHeader];   // 1 MiB
    off_t loc = [XADRAR5Parser signatureLocationInData:data];
    if (loc == RAR5SignatureNotFound) [NSException raise:RAR5SignatureCannotBeFound ...];
    [self.handle skipBytes:loc];
}
```

「`RAR5MaximumSFXHeader = 1 << 20`」这个常量把所有魔数集中到一处，注释直白：*"for SFXX, RAR Signature can be found not at start, but anywhere in the data"*。

### 4.4 机制 D：ZIP 的尾部搜索（尾部偏移的典范）

`XADZipParser.m:118-191`。EOCD 理论上在文件最后 22+65535 字节内，但 XADMaster **不信任这个上限**，做成"分块从尾往前扫直到文件开头"：

```
findCentralDirectoryRecordOffset:
    seekToEndOfFile(); end = offset
    chunk_size  = 0x10000          # 64 KiB
    preserved   = min(chunk_start, 20)   # 保留 20B 重叠
    chunk_end   = end

    loop:
        numbytes = min(chunk_size, chunk_end)
        chunk_start = chunk_end - numbytes
        buf = read(chunk_start, numbytes)

        # 在块内【从后往前】找 EOCD
        pos = numbytes - 4
        while pos >= preserved:
            if buf[pos..pos+3] == "PK\x05\x06": break
            pos -= 1

        if found(pos >= preserved):
            if centrOffset: *centrOffset = chunk_start + pos
            # Zip64 EOCD locator 就在 EOCD 前 20 字节
            if pos >= 20 and buf[pos-20..pos-17] == "PK\x06\x07":
                *zip64offs = chunk_start + pos - 20
            return

        # 关键：下一块要重叠，避免 PK67/PK06 被切在块边界
        chunk_end -= (chunk_size - preserved*2)
        if chunk_end <= 0: break
```

注意三个细节：
1. **20 字节重叠**（`preservedBytes`）—— 因为 Zip64 locator 是 20 字节，恰好卡在块边界会被漏掉。源码里有 ASCII 图解注释（`XADZipParser.m:176-183`）。
2. **`pos>=preservedBytes` 而非 `pos>=0`** —— 保证 "PK\x05\x06" 四个字节完整落在本块内。
3. **找不到 EOCD 不放弃** → `parseWithoutCentralDirectory`（见 4.7）。

### 4.5 机制 E：ISO9660 动态推导扇区大小（最巧的一个）

`XADISO9660Parser.m:62-105`：

```objc
// 从 0x8000 起扫描主卷描述符
for(int i=0x8000; i<length-2048-6; i++) {
    if(IsISO9660PrimaryVolumeDescriptor(bytes,length,i)) {
        // 在下一个"块"里再找一次 "CD001"，用间距反推块大小！
        for(int j=2048; j<2448; j++) {
            if(i+j+6>length) break;
            if(memcmp(&bytes[i+j+1], "CD001", 5)==0) {
                props["ISO9660ImageBlockSize"]   = j;          // 2048 或 2352
                props["ISO9660ImageBlockOffset"] = i - j*16;   // 扇区 0 的字节位置
                return YES;
            }
        }
    }
    // High Sierra 变体：找 "CDROM"
    if(IsHighSierraPrimaryVolumeDescriptor(bytes,length,i)) {
        for(int j=2048; j<2448; j++)
            if(memcmp(&bytes[i+j+9], "CDROM", 5)==0) {
                props["ISO9660ImageIsHighSierra"] = YES;
                props["ISO9660ImageBlockSize"]    = j;
                props["ISO9660ImageBlockOffset"]  = i - j*16;
                return YES;
            }
    }
}
```

**一次循环同时解决了：镜像前缀偏移（`i`）、扇区大小（`j: 2048 或 2352 原始 CD）和"扇区 0 在哪"（`i - j*16`）。**
`i - j*16` 的含义：卷描述符固定位于第 16 个扇区，所以扇区 0 起点 = 命中位置 − 16×扇区大小。这也是"光盘镜像前面被拼接了别的数据"（如 `.bin/.cue`、混合模式光盘）能正确识别的原因。

### 4.6 机制 F：`XADDataOffsetKey` 与 `XADSkipOffsetKey` 的双通道

`XADArchiveParser.m:905-924`：

```objc
-(CSHandle *)handleAtDataOffsetForDictionary:(NSDictionary *)dict
{
    NSNumber *skipoffs = dict[XADSkipOffsetKey];
    if(skipoffs) {                                  // 通道 1：跳过若干字节后开始
        [skiphandle seekToFileOffset:skipoffs];
        NSNumber *length = dict[XADSkipLengthKey];
        if(length) return [skiphandle nonCopiedSubHandleOfLength:length];
        else       return skiphandle;
    } else {                                        // 通道 2：绝对的偏移+长度
        [sourcehandle seekToFileOffset:dict[XADDataOffsetKey]];
        NSNumber *length = dict[XADDataLengthKey];
        if(length) return [sourcehandle nonCopiedSubHandleOfLength:length];
        else       return sourcehandle;
    }
}
```

- **`XADDataOffset/Length`**：实体数据在**原始文件**里的绝对位置（外部归档格式常用）。
- **`XADSkipOffset/Length`**：数据在一个**中间流**里的位置（`XADSkipHandle` 可以注册多段"虚假区域"，把不连续的块拼成逻辑连续流）—— 典型用途是 `.dmg`/资源叉/稀疏文件里"洞"的处理。

Rust 版对应设计：条目数据定位至少要支持 (a) 绝对偏移+长度 (b) 相对偏移+长度 (c) 多段拼接。

### 4.7 机制 G：无中心目录的线性重扫 + 重同步

`XADZipParser.m:460-586` `-parseWithoutCentralDirectory`：从头逐条读本地文件头，识别 4 种 `PK` 头：

```objc
switch(id) {
  case 0x504B0304: /* local file header */ ... break;
  case 0x504B0102: stopParsing = YES; break;                 // 中央目录开始了，停
  case 0x504B0708: /* multi */    [self findNextEntry]; break; // 神秘的跳过
  case 0x504B3030: /* strange */  [self findNextEntry]; break;
  default:
      [self setObject:@YES forPropertyKey:XADIsCorruptedKey]; // 标脏
      [self findNextEntry];                                   // 重同步
}
```

重同步函数本身极简（`XADZipParser.m:686-701`）：

```objc
static int MatchZipEntry(const uint8_t *bytes,int available,off_t offset,void *state)
{
    if(available<6) return NO;
    if(bytes[0]!='P'||bytes[1]!='K'||bytes[5]!=0) return NO;   // 第 6 字节必须为 0（额外保险）
    if(bytes[2]==1&&bytes[3]==2) return YES;   // 中央目录
    if(bytes[2]==3&&bytes[3]==4) return YES;   // 本地头
    if(bytes[2]==5&&bytes[3]==6) return YES;   // EOCD
    return NO;
}
-(void)findNextEntry { [[self handle] scanUsingMatchingFunction:MatchZipEntry maximumLength:4]; }
```

`scanUsingMatchingFunction:maximumLength:` 来自 `Scanning.h`（`Scanning.m:45-60`），是一个**带"滑动窗口 + 提前放弃"的字节匹配器**：`maximumLength` 是"最多往前找多远"。同一个原语也用于数据描述符搜索（`XADZipParser.m:656-682`）和 `PK\x03\x04` 等。

---

## 5. ZIP 解析容错度小结

XADMaster 的 ZIP 实现比大多数库"脏"得多，容错清单：

| 容错点 | 位置 | 行为 |
|---|---|---|
| EOCD 位置超 64KB | `XADZipParser.m:131-190` | 分块（64KiB）反向扫到文件头 |
| Zip64 locator 跨块边界 | 同上 `:185` | 20 字节重叠读取 |
| EOCD 完全丢失 | `:204-209` | 退回线性本地头扫描 |
| >4GB 但没写 Zip64 | `:220-226` | 直接放弃中央目录，走线性扫描 |
| 条目数 >65535 未用 Zip64 | `:307-310` | `centraloffset+centralsize-next > 65536*46` 则 `numentries += 65536`（可多次） |
| 本地头签名异常 | `:316` | 接受 `0x504B0506` 作为本地头（"kludge for strange archives"） |
| 本地头找不到 | `:360-363` | 置 `XADIsCorruptedKey` 继续别的条目 |
| `PK\x07\x08` / `PK\x30\x30` | `:556-562` | 静默跳过并重同步 |
| 未知数据块 | `:564-569` | 置脏 + `findNextEntry` 重同步 |
| extra 字段解析失败 | `:703-720` | `parseZipExtraOrNilWithLength:` 吞异常，置脏，返回 nil |
| deflate 长度不可信 | `:1207-1257` | `CSZlibHandle deflateHandleWithHandle:` **故意不传 length**，注释说明"有些压缩器不给 >4GB 文件写 zip64，size 可能是错的" |
| 条目名无 UTF-8 flag | `:1008-1014` | 走 `XADStringSource` 的编码猜测（mac zip 常见 CP437/MacRoman） |
| 目录条目无尾斜杠 | `:1016-1030` | 用"下一条目名字是否以本条 + '/' 开头"反推目录 |
| MS-DOS 系统却用反斜杠 | `:1002-1011` | IZArc 兼容 kludge，允许 DOS 路径分隔符 |
| data descriptor | `:515-528` + `:590-654` | flag 0x08 时按流内容搜 PK\x07\x08 |

**`XADIsCorruptedKey` 的语义**：它不是"解析失败"，而是"**我跳出了一些东西但仍然产出了可用的条目列表**"。`XADArchiveParser.m:1016-1017` 在 `addEntryWithDictionary:` 里把任何条目的脏标记上浮为归档级属性。

---

## 6. 加密与密码处理

### 6.1 密码的索取是"拉取式"，不是"注入式"

`XADArchiveParser.m:647-660`：

```objc
-(NSString *)password {
    if(!password) {
        [delegate archiveParserNeedsPassword:self];   // 回调上层去要
        if(!password) return @"";
    }
    return password;
}
```

`XADArchiveParser.m:662-673` `setPassword:` 还会**作废 solid 流缓存**（因为 solid 流的早期条目数据是用旧密码解的）：

```objc
-(void)setPassword:(NSString *)newpassword {
    password = newpassword;
    [currsolidobj release]; currsolidobj = nil;       // 必须重解
    [currsolidhandle release]; currsolidhandle = nil;
}
```

### 6.2 密码编码链

- `passwordencodingname` 可独立于文件名编码（`XADArchiveParser.m:695-708`）。
- `encodedPassword` / `encodedCStringPassword`（`:1235-1250`）把 NSString 按目标编码转成字节。
- 在递归子归档时，**父级密码 + 编码一起下传**（`:507-509`）。
- `-caresAboutPasswordEncoding` 标志用于"我确实需要密码字节"的场景。

### 6.3 各格式的密码校验

| 格式 | 位置 | 机制 |
|---|---|---|
| **ZipCrypto** | `XADZipParser.m:1176-1185` + `XADZipCryptHandle.m:41-70` | 计算 `testByte`：若 `flags&0x08`（有 data descriptor）→ 用 `localDate>>8` 的高字节；否则用 `CRC32>>24`。解出 12 字节头后比对第 12 字节。 |
| **WinZip AES** | `XADZipParser.m:1155-1175` + `XADWinZipAESHandle.m:47-92` | 严格校验 `version∈{1,2}`、`vendor==0x4541`、`keySize∈[1,3]`→{16,24,32}B；**PBKDF2-HMAC-SHA1，1000 轮**，salt = keybytes/2 字节；version 2 额外校验 HMAC。 |
| **RAR 1.3/1.5/2.0** | `XADRAR13CryptHandle` / `XADRAR15CryptHandle` / `XADRAR20CryptHandle` | 各版本自研流密码 |
| **RAR 5 / 7z** | `XADRARAESHandle` / `XAD7ZipAESHandle` | AES-256-CBC + PBKDF2 |
| **StuffIt** | `XADStuffItDESHandle` / `XADStuffItXCyanideHandle` | DES / 自研 |
| **PDF** | `PDF/PDFEncryptionHandler.m` / `PDFEncryptionUtils.m` | 标准安全处理器（RC4/AES） |
| 通用 | `XADRC4Handle`, `XADXORHandle`, `XADXORSumHandle` | 复用积木 |

### 6.4 密码尝试机制：**没有爆破**

- `unar` CLI 只提供 `-p/--password` 与 `-E/--password-encoding`（`unar.m:84-100`）。
- 失败路径：`XADSimpleUnarchiver.m:715-727` → `unar.m:347-362`，交互式则 `AskForPassword()`，非交互直接 `exit(2)`。
- **XADMaster 本身不实现字典/暴力破解**；尝试循环由调用方（The Unarchiver GUI）在外层做，引擎只负责"给出一个明确的 XADPasswordError / check byte 不匹配"。

> 对 unbox 的启示：引擎侧应把"密码是否正确的判定"做成**廉价且早失败**（ZipCrypto 的 check byte、AES 的 verifier），把"试密码"的循环留给上层。

---

## 7. 意外发现（重点）

### ★1. 「归档被拼接在别的文件后面」有正式的一等公民机制，且用**最低偏移**仲裁

不是特例代码，而是：
- 独立的类属性 `parsersWithFloatingSignaturesClasses`；
- 标准化的偏移契约 `XADSignatureOffset`（字面量 `XADFoundSignatureOffset`）；
- 显式的仲裁函数 `CompareParserSignaturesLocations`，缺省值 `0x100000`；
- 专用单元测试 `XADSFXDetectionParserTests.m` 钉死"低偏移赢"。

**这套东西可以直接照搬成 Rust 的 `SignatureOffset` 仲裁器** —— 它是"一个文件里有两层包"的唯一正确解法。

### ★2. 注册表的**顺序本身就是文档**，注释分组是设计意图

```
Common → Mac → Less common → Detectors that require lots of work → Over-eager detectors → LibXAD
```

"Over-eager detectors"（过度热心探测器：ARC/Squeeze/Crunch/LBR/LZMA_Alone/Cpio/SplitFile/ISO9660）被**刻意排在最后**，因为它们的签名弱、靠启发式，放前面会吃掉真格式。这是一个非常清醒的工程决策。

### ★3. `requiredHeaderSize` 派生 `maxheader`，一次读 512KB 分发给所有候选

`XADArchiveParser.m:251-257`。好处：
- 候选解析器拿到的 `firstBytes` 是**同一块内存**（零成本共享，不用各自 IO）；
- 同时给一个**可 seek 的 handle**，需要更多字节的解析器可以自己读；
- `maxheader` 自动随新增格式增长，不需要手改常量。

代价：**任何文件嗅探都要读 512KB**（对小文件有浪费）。Rust 版可以改成"分级预读"：先读 4KB 过便宜签名，只有进入昂贵探测组才补齐到 512KB/1MB。

### ★4. 识别期异常吞噬 = 允许写"贪心而脆弱"的识别器

`+isValidParserClass:` 的 `@try/@catch`。这让 Compact Pro 敢在识别阶段**解完整张目录表算 CRC32**、BinHex 敢**真解一段并校验 CRC16**、ISO9660 敢**两次嵌套扫描**——失败了就是异常，一行 catch 全兜。

**这条对 Rust 尤其珍贵**：Rust 没有异常，最接近的等价物是"`recognize` 返回 `Result<bool>` 且 `Err` 一律视作 false"。也就是说 —— **让识别函数返回 `Result<bool, Error>`，调用方 `.unwrap_or(false)`**，就能得到同样的"随便失败"自由度。

### ★5. 有格式**完全没有 magic**，只能靠参数合法性反推

`XADLZMAAloneParser.m:28-46`，源码原注释：

> `// Geez, put some magic bytes in your file formats, people!`

它的判定是四重约束的交集：

```objc
if(bytes[0] >= 9*5*5) return NO;                 // props 字节上界
dictsize = LE32(&bytes[1]);
div3 = dictsize * 0xaaaaaaab;                    // 除以 3 的魔数乘法
if((dictsize&(dictsize-1))!=0 && (div3&(div3-1))!=0) return NO;  // 必须是 2^k 或 3·2^k
if(LE64(&bytes[5]) == 0) return NO;              // 未压缩大小不能被声明为 0
if(!(bytes[11]==0&&bytes[12]==0) && !(bytes[11]==0xff&&bytes[12]==0xff)) return NO;  // 高 2 字节必须全 0 或全 FF
```

**"没有常量就找不变量"** —— 这个思路对 unbox 极其关键：面对"MP4 里塞 deflate 流"这种事，deflate 头（`0x78 0x9C` 之类）本身是可伪造的，但 **zlib 头的 CMF/FLG 必须满足 `(CMF*256+FLG) % 31 == 0` 且 CM=8** —— 这就是同类的"数学不变量"校验。XADMaster 还有 `MatchZipDataDescriptor` 用**CRC32 字段 == 已知 CRC32** 作为强判据（极难误报）。

### ★6. `reportInterestingFileWithReason:` —— 专门给"可疑/异常"打报告的钩子

`XADArchiveParser.m:1254-1263`，全库埋点：

| 触发点 | 原因字符串 |
|---|---|
| `XADZipParser.m:992-993` | `"Reduce %d compression"`（古老的 Reduce 算法，几乎只有伪造/老文件才有） |
| `XADZipParser.m:1244` | `"Unsupported compression method %d"` |
| `XADPDFParser.m:262-264` | `"CalRGB image"` / `"CalGray image"` / `"Lab image"`（异常色彩空间） |
| `XADPDFParser.m:159-160`（注释保留） | `"CMYK JPEG"` / `"JPEG with ICC profile"` |

钩子的实现是 `varargs` + `[NSString stringWithFormat:@"%@: %@", [self formatName], fullreason]`，且**不影响解析流程**。

**这正是 unbox 需要的"反取证/异常上报通道"**：识别出"这个 MP4 里有一坨 Reduce 压缩数据"或"这个 JPEG 的尾部偏移有 ZIP 签名"这种**强烈可疑信号**，而不只是返回一个格式名。

### ★7. `XADMightBeMacBinaryKey`：两级怀疑的"便宜线索 → 昂贵验证"

见 2.3。核心是允许其他解析器**零成本埋线索**（`条目名以 .bin 结尾`），真正昂贵的验证（读 128B + 算 CRC16）推迟到"有人问"的时候。而且**在不可 seek 的流上直接不验证**（`XADMacArchiveParser.m:374`）—— 因为验证要回读。

### ★8. ISO9660 用"两个描述符的间距"反推扇区大小

见 4.5。一行 `ISO9660ImageBlockOffset = i - j*16` 同时解决前缀偏移 + 扇区大小 + 扇区 0 定位。**"用结构间的相对距离反推全局参数"** 是一个威力极大的技巧：面对"我不知道 MP4 的 moov 偏移，但我看到了两个 `moov`/`udta` 原子"这类问题时，间距关系就是答案。

### ★9. 有些格式**只看文件名**（并且这是有意为之）

- `XADNSAParser.m:34-40` / `XADSARParser.m:28-33`：只匹配 `^arc[0-9]*\.(nsa|sar)$`，**内容一行都不看**（源码里原来的 `bytes` 检查被注释掉了）。
- `XADTarParser.m:77-84`：`\.tar$` 直接放行。
- `XADSplitFileParser.m:30-44`：只看 `^(.*)\.([0-9]{3})$` **且** 在磁盘上确认另一个分片存在。
- `XADZipMultiPartParser.m:107-135`：强制要求文件名含 `.zip` **且** 必须能 seek。

**这提醒我们：纯内容判定不是万能的，也不是必须的。** `unbox` 需要的是"**后缀不能作为证据，但可以作为提示**"——把后缀降级为 tie-breaker / 二次验证触发器，而不是主判据。XADMaster 里 `XADCABSFXParser`、`XADSplitFileParser` 都在用"文件名 + 内容"联合判定。

### ★10. 不可 seek 的流一律当 solid 处理

`XADArchiveParser.m:577-590`：

```objc
-(void)setHandle:(CSHandle *)newhandle {
    ...
    CSHandle *testhandle = newhandle;
    if([testhandle isKindOfClass:[CSSubHandle class]]) testhandle = [(CSSubHandle *)testhandle parentHandle];
    if([testhandle isKindOfClass:[CSStreamHandle class]]) forcesolid = YES;  // 流式 → solid
    else forcesolid = NO;
}
```

因为"读不了就只能顺序读一遍"，于是整包必须是 solid（所有条目共享一次解压流）。`addEntryWithDictionary:` 会自动补 `XADSolidObjectKey`（`:1086`）并维护 `FirstSolid/NextSolid` 链（`:1088-1120`）。

### ★11. PDF 被当成"容器"，并且会**重装外部格式的头**

`XADPDFParser.m:60-360`：遍历 PDF 对象字典，抽 `isImage` 的 `PDFStream`，然后：
- 从页面树（`Kids`/`Page`）反推每个图片属于哪一页（`page` 计数 → 条目排序）；
- 根据 `Filter` / `DecodeParms` / `Predictor` 决定输出为 **JPEG** 还是 **TIFF**；
- 用 `CreateTIFFHeaderWithEntries(...)` 和 `CreateNewJPEGHeaderWithColourProfile(...)` **现场构造一个独立的 TIFF/JPEG 头**，把 PDF 内部的裸像素数据和 ICC profile 重新包装成可独立打开的文件。

**"提取出来的东西不是原样输出，而是重装成宿主格式"** —— 这也是 unbox 可以直接借鉴的方向：从 MP4 里掏出 H.264 裸流时，装回 `.h264` Annex-B 或 `.mp4` 头，比吐一坨裸字节对用户有用得多。

### ★12. PDF 的 `proceedAssumingCharacter:` —— 极简的容错解析原语

`PDF/PDFParser.m:192-196`：

```objc
-(void)proceedAssumingCharacter:(uint8_t)c errorMessage:(NSString *)error {
    if(currchar != c) [self _raiseParserException:error];
    [self proceed];
}
```

然后 `"xref"` 就是四次调用、`"trailer"` 就是七次调用（`:247-264`）。它把"**我期望这里是 X，不是就报错**"变成一个可复用的一行原语，比手写 `if(memcmp(...))` 更好读、更容易给统一错误信息。Rust 里可以用一个小的 `expect_bytes(&[..])?` combinator 复刻。

PDF 另外两个容错点：**`/Prev` 链回溯**（`:225-232`，增量更新的 PDF 会有一串旧 xref），**xref 表 vs xref 流双路**（`:237-241`，PDF 1.5+ 用对象流）。

### ★13. `-recursiveArchiveForFile:` 无脑下钻直到"不是单条目归档"

`XADArchive.m:45-61`。**不设深度上限**，靠"归档里只有 1 个条目"这个条件自然终止。这直接赋予了 unar "改后缀也照样解"的能力。

配合 `extractEntries:` 的降级逻辑（`XADArchive.m:790-793`）：

```objc
if(sub && [self entryIsArchive:i]) {
    @try { res = [self extractArchiveEntry:i to:destination]; } @catch(id e) { res = NO; }
    if(!res && lasterror == XADDataFormatError)   // 当作归档解失败、且原因是"不认识这个格式"
        res = [self extractEntry:i to:destination deferDirectories:YES];  // 退回当普通文件解
}
```

**"尝试解归档 → 失败就退回当普通文件"** 是"伪装包"场景下最实用的双通路策略。

### ★14. 多卷扫描是"主动列目录"，而不是"等用户给卷列表"

`XADArchiveParser.m:856-890`：拿到文件名 → `stringByDeletingLastPathComponent` → `XADPlatform contentsOfDirectoryAtPath:` → 正则过滤 → 排序。

两种排序策略：
- `XADVolumeSort` 支持 `firstFileExtension`（`z01` 排最前，保证 `.z01` 在 `.zip` 前）；
- 否则用 `NSCaseInsensitive` 比较。

**同一机制同时服务"多卷"和"分卷"**（`.zip.001` 系列靠列目录发现 `.002`…）。

### ★15. 分卷识别的阳性条件是"**另一个文件存在**"

`XADSplitFileParser.m:30-44`：

```objc
matches = name 匹配 "^(.*)\.([0-9]{3})$";
if(!matches) return NO;
otherext = ([matches[2] isEqual:@"001"]) ? @"002" : @"001";
othername = matches[1] + "." + otherext;
return [XADPlatform fileExistsAtPath:othername];   // ← 就靠这一句
```

**"文件系统本身也是判据"** —— 这是纯内容嗅探器做不到的信息源。

### ★16. LibXAD 作最后的兜底，并留了 `.adf` 后缀旁路

`XADLibXADParser.m:35-60`：调用外部 `xadOpenLibrary(12)` / `xadRecogFile()`，把整个文件（`[data length]`, `[data bytes]`）交出去。**排在整个注册表最末尾**，只在所有内置解析器都失败后运行。

源码注释很诚实：`// Kludge to recognize ADF disk images, since the filesystem parsers don't provide recognition functions`。

**架构启示：识别管道可以留一个"外部插件"插槽放在最末位**，代价（可能慢、可能误报）只在前面全失败时才付。

### ★17. `XADSkipHandle`：把"不连续的块"伪装成连续流

`XADSkipHandle.h` 里的 `XADSkipRegion { off_t actual, skip; }` 数组 + `addSkipFrom:length:` / `addSkipFrom:to:` / `actualOffsetForSkipOffset:` / `skipOffsetForActualOffset:`。

也就是说，XADMaster 里"文件里有洞"不是靠特判，而是**在 handle 层注册虚假区域**，让所有上层解析器对"洞"完全无感。配合 4.6 的 `XADSkipOffsetKey` 通道使用。

Rust 版可以直接设计成 `struct SparseReader { regions: Vec<Region> }` 实现 `Read + Seek`，这样"跳过前置垃圾数据"和"拼接多卷"是同一个抽象。

### ★18. `XADArchiveParserDescriptions` —— 属性可读化与"元数据自描述"

`XADArchiveParserDescriptions.m` 提供 `descriptionOfValueInDictionary:key:`，按 key 的**正则模式**决定如何展示：

```objc
if([key matchedByPattern:@"CRC32$"])      → "0x%08x"
else if([key matchedByPattern:@"CRC16$"]) → "0x%04x"
else if([key matchedByPattern:@"Is[A-Z0-9]"]) → human readable boolean
else if(key == XADFileSizeKey || XADCompressedSizeKey) → human readable size
else if(key == XADPosixPermissionsKey) → rwxr-xr-x
```

`lsar.m` 靠它输出 `unar -l` 的人类可读列表。同时 `XADArchiveParserDescriptions.m:99,172` 有一张"属性 → 中文/英文名 + 是否对用户可见（202 之类的优先级数字）"表。

**用模式匹配来驱动格式化，避免为每个 key 写一个分支** —— 值得学的元编程小技巧。

### ★19. `XADIsCorruptedKey` 是"脏标记"而非"失败"

见第 5 节。它的语义是"**我跳过了无法理解的东西，但条目列表仍可用**"。`XADArchiveParser.m:1016-1017` 把它从条目上浮到归档级。UI 层可以据此提示"这个包可能被篡改过"。

**对 unbox 极其对味**：一个"MP4 尾部藏了 ZIP"的文件，完全可以既返回"MP4"又返回"corrupted/trailing-data"标记，而不是二选一。

### ★20. 内置"格式探测已识别的归档是否还套了一层"的判定字段 `XADIsArchiveKey`

由各解析器在 `parse` 阶段自己设置（`XADGzipParser.m:106`、`XADBzip2Parser.m:61`、`XADCompressParser.m:54`、`XADXZParser.m:54`、`XADLZMAAloneParser.m:78`、`XADRPM.Parser`、`XADMacBinaryParser.m:57`…），含义是"**我这一个条目本身是个归档，值得再递归进去**"。

**关键点：这个标志是单条目粒度的，不是文件粒度的。** 所以"MP4 的第 3 个 track 里其实是个 zip、其余 track 是正常视频"这种情况，XADMaster 的数据模型是能表达的。

---

## 8. 对 unbox（Rust）的启示

### 8.1 推荐的分层结构

```rust
/// 与 XADArchiveParser +recognizeFileWithHandle: 对应
pub trait FormatProbe {
    /// 便宜的预读需求；框架据此决定"分级预读"策略
    fn required_header(&self) -> usize;               // ← requiredHeaderSize
    /// 返回 Result，Err 一律视作"不匹配"（等价于 XADMaster 的 @try/@catch）
    fn recognize(&self, h: &mut dyn ReadSeek, header: &[u8], name: Option<&str>,
                 out: &mut ProbeFindings) -> Result<bool, ProbeError>;
    fn priority(&self) -> u8;                         // ← 注册表顺序，显式化
    fn vol_scan(&self, ...) -> Option<Vec<PathBuf>> { None }  // ← volumesForHandle:
}

/// 识别期产出的"线索袋"：偏移、内层格式提示、可疑度
#[derive(Default)]
pub struct ProbeFindings {
    pub signature_offset: Option<u64>,   // ← XADSignatureOffset
    pub private: BTreeMap<&'static str, i64>, // ← RAREmbedOffset / CABSFXOffset / …
    pub suspicious: Vec<&'static str>,   // ← reportInterestingFileWithReason:
    pub might_be: SmallVec<[HintKind; 2]>, // ← XADMightBeMacBinaryKey
    pub corrupted: bool,                 // ← XADIsCorruptedKey
    pub is_archive: bool,                // ← XADIsArchiveKey
}
```

### 8.2 嗅探主流程（建议）

```
fn sniff(handle, name) -> Result<Classified>:
    header4k  = read(0, 4096)                       # 阶段 1：便宜
    for probe in PROBES_BY_PRIORITY where cheap:    #   顺序即优先级
        if probe.recognize(header4k)? : return ...  #   Err → continue

    header_big = read(0, MAX_HEADER)                # 阶段 2：补齐到 512KB/1MB
    for probe in PROBES_BY_PRIORITY where !cheap:
        ...
        if probe.floating:                          # 阶段 3：浮动签名组
            collect all matches w/ signature_offset
            return min_by_key(signature_offset)     #   低偏移赢
    return Err(XADDataFormatError)
```

要点（逐条对应源码）：
1. **`Err` 视作 `false`**，不要 `?` 直接上抛（对应 `+isValidParserClass:` 的 catch）。
2. **每个候选跑在干净的 `ProbeFindings` 里**，只在胜出时并入外部（对应 `props.removeAllObjects`）。
3. **每次候选前后 `seek(0)`**。
4. **顺序显式排序**：便宜签名（magic 常量）→ Mac/罕见格式 → 昂贵探测器 → 过度热心探测器 → 外部插件。
   Rust 版可以用 `Vec<Box<dyn FormatProbe>>` 常量数组，或把顺序写进 `priority: u8` 并由测试断言。
5. **`maxheader` 由各 probe 的 `required_header()` 取 max**，不要硬编码 512KB。
6. **分级预读**（改进）：小文件不用付 512KB 的代价。

### 8.3 针对"MP4/MKV/PNG/JPG/PDF 里藏压缩数据"的专项建议

XADMaster 里**没有**"在 MP4/PNG/JPG 里找 ZIP"的现成实现（它只覆盖 Zip/RAR/RAR5 的 SFX），但它提供的积木刚好够用：

| 需求 | 抄 XADMaster 的哪块 |
|---|---|
| 在任意偏移找内层包 | `XADEmbeddedRARParser`（全 header 线性扫 + 记录 offset） |
| 多个内层签名时选哪个 | `CompareParserSignaturesLocations`（**最低偏移优先**） |
| 尾部藏东西（ZIP/JPG 的 EOI 之后、MP4 的 `moov` 之后） | `XADZipMultiPartParser` 的**从尾部反向扫** |
| 前缀有垃圾 | `GzipSFXOffset` / `7zSFXOffset` 的"seek 过去再 parse" |
| 需要区分"内层是真包"还是"巧合" | `MatchZipDataDescriptor`（**用 CRC32 反向验证**）+ `XADZipSFXParser` 的 version/method 合理性区间检查 |
| 没有 magic 的流（deflate/zlib/LZMA） | `XADLZMAAloneParser` 的**参数不变量** + zlib 的 `(CMF*256+FLG)%31==0` |
| 报告可疑但不断定 | `reportInterestingFileWithReason:` → `ProbeFindings.suspicious` |
| 容器里"重装宿主格式"输出 | `XADPDFParser` 的 `CreateTIFFHeaderWithEntries` 思路（H.264 → Annex-B / mp4） |
| 文件名只作提示 | `XADSplitFileParser`（"另一个分片存在"）+ `XADCABSFXParser`（后缀 + 内容联合） |

### 8.4 立即可落地的 5 条硬规则

1. **`recognize()` 返回 `Result<bool, E>`，调用方 `.unwrap_or(false)`。** 给识别器最大的"随便失败"自由。
2. **偏移是一等公民**：`ProbeFindings.signature_offset: Option<u64>`；`parse()` 之前统一 `seek()`。识别器与解析器之间只用这一条契约。
3. **浮动签名组 + 最小偏移仲裁**，并写单测钉死（照抄 `XADSFXDetectionParserTests` 的四个用例：低偏移赢 / 单独命中 / 互斥顺序无关）。
4. **`corrupted` 与 `suspicious` 独立于"识别成功"**：允许 `Ok(Format::Mp4) + findings.corrupted = true + findings.suspicious = ["trailing-zip@0x1f000"]`。
5. **在"过度热心"的探测器和便宜 magic 之间显式分层**，并用一个常量数组 + 注释分组固化顺序（照抄 XADMaster 的 5 组注释）。

---

## 9. 附：一页速查

```
嗅探入口      XADArchiveParser.m:260 / :354 / :406 / :483
注册表 + 顺序  XADArchiveParser.m:156-258   （5 组注释：Common/Mac/Less common/Expensive/Over-eager）
主循环        XADArchiveParser.m:260-295
异常吞噬       XADArchiveParser.m:329-339
浮动签名仲裁   XADArchiveParser.m:297-327 + :146-152（缺省 0x100000，低偏移赢）
头部预读       maxheader = max(requiredHeaderSize) = 0x80000 (512KB)
偏移契约       XADSignatureOffset = @"XADFoundSignatureOffset"   XADArchiveParser.m:143
数据定位       XADDataOffset/Length（绝对） vs XADSkipOffset/Length（相对+多段）  :905-924
ZIP EOCD       XADZipParser.m:118-191（64KB 分块反向扫 + 20B 重叠 + Zip64 locator）
ZIP 无 CD 降级  XADZipParser.m:460-586 + :686-701（线性扫 + PK 重同步）
ZIP 脏标记     XADIsCorruptedKey，:360-363 / :564-569
MacBinary 怀疑  XADMightBeMacBinaryKey（ZIP :1040 / LZH :240 埋点；MacArchive :360-380 验证）
可疑上报       reportInterestingFileWithReason:  :1254-1263
PDF 容器化     XADPDFParser.m:60-360（抽 image stream + 重装 JPEG/TIFF 头）
密码           XADArchiveParser.m:647-673 / :1235-1250；无内置爆破（unar.m:347-362）
递归下钻       XADArchive.m:45-61（单条目不断下钻）；:790-793（解失败退回普通文件）
```

---

*报告结束。所有行号基于当前仓库快照，未经修改。*
