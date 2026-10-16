# 7-Zip 源码分析：归档扫描/识别机制（`-t#` Scan 模式）

分析对象：`/home/user/hermes/yshjy/refs/7zip/`
版本：`git log` = `9128b80 26.04`（7-Zip 26.04）
核心文件：`CPP/7zip/UI/Common/OpenArchive.cpp`（3690 行，全部扫描逻辑都在这里，**没有独立的 Parser.cpp**）

---

## 0. 一句话结论

`-t#` 不是"某个归档格式"，而是一种**打开模式**（`COpenType`）。它把"打开归档"降级成"在整份文件里做前向签名扫描，为每一段候选归档创建一个虚拟归档 `NArchive::NParser::CHandler`"，最终把文件切成 `未知垃圾 / 内嵌归档 / 未知垃圾 / …` 的**分段列表**。它不依赖容器自身的目录表（CD/索引），因此对"目录表偏移异常"的伪压缩包也能识别。

---

## 1. `-t#` 在源码里的对应（问题 1）

| 层 | 位置 | 内容 |
|---|---|---|
| CLI 解析 | `ArchiveCommandLine.cpp:298` | `{ "t", SWFRM_STRING_SINGL(1) }` — `-t` 取一个字符串 |
| CLI 存值 | `ArchiveCommandLine.cpp:1573-1574` | `options.ArcType = parser[kArchiveType].PostStrings[0]`，即 `"#"` |
| 进入打开 | `UI/Console/Main.cpp:1048-1049` | `ParseOpenTypes(*codecs, options.ArcType, types)` |
| **类型判定** | **`OpenArchive.cpp:3618-3623`** | `else if (name[0] == '#') { ... type.CanReturnArc = false; type.CanReturnParser = true; }` |
| 承载结构体 | `OpenArchive.h:61` `struct COpenType` | 字段 `CanReturnArc` / `CanReturnParser` / `EachPos` / `Recursive` / `MaxStartOffset` |
| **虚拟归档类** | **`OpenArchive.cpp:117` `namespace NArchive { namespace NParser`** | 内含 `struct CParseItem`（一段）与 `class CHandler`（把多段当成一个归档来列举/提取） |

关键：`'#'` 命中 `ParseType` 里"不是已知格式名"的分支，于是：

```cpp
else if (name[0] == '#')            // OpenArchive.cpp:3618
{
  if (name[1] != 0) return false;   // 必须正好是 "#"，#:a 之外不能带前缀
  type.CanReturnArc    = false;     // 不把整份文件当成一个归档返回
  type.CanReturnParser = true;      // 用"扫描解析器"模式
}
```

`CanReturnParser=true` 这一位就是"扫描模式"的开关。它有三处作用：
1. `OpenArchive.cpp:2092` / `2578` / `2745`：错误分支里不再提前 `return S_FALSE`，允许"先当成非归档、继续扫描"。
2. `OpenArchive.cpp:2576`：**取消 8MB 起始偏移限制**（见第 5 节，这是核心）。
3. `OpenArchive.cpp:2981`：扫描结束后，即使没找到完整归档，也把 `NParser::CHandler` 作为 `Archive` 返回（`FormatIndex = -1`，`IsParseArc = true`）。

### 子参数（鲜为人知）

`ParseType` 按 `:` 切参数，`ParseTypeParams`（`OpenArchive.cpp:3564`）支持：

| 写法 | 字段 | 含义 |
|---|---|---|
| `-t#` | — | 默认扫描；未知段会合并 |
| `-t#e` | `EachPos=true` | 每个字节位置都试（不去重、不跳过），最慢最全 |
| `-t#a` | `CanReturnArc=true` | 允许把整份文件当成一个归档直接返回 |
| `-t#:s<size>` | `MaxStartOffset` | 限制起始扫描范围，如 `-t#:s64m`；单位 b/k/m/g/t |
| `-t#r` | `Recursive=true` | 递归 |
| `-t#.7z` | 多类型 | 用 `.` 组合多个类型（`ParseOpenTypes` 按 `.` 切） |

多个类型也可写成 `-t#:e:r` 之类，参数用 `:` 串联。

---

## 2. 扫描分段算法（问题 2）

### 2.1 总体流程（`CArc::OpenStream2`，`OpenArchive.cpp:1610` 起）

```
OpenStream2(op):
  # 1) 先按"正常方式"把所有已知格式在候选起始位置试一遍
  #    （按扩展名/签名排序，orderIndices，见 2118 "Check all possible START archives"）
  for each format in orderIndices:
      archive = CreateArc(format)
      OpenArchiveSpec(archive, needPhySize=!CanReturnArc, stream, &searchLimit=0, cb)
      if 成功: 记录 CParseItem{Offset, Size=PhySize}; handlerSpec->AddItem(pi)
              pos = pi.Offset + pi.Size        # 跳过已识别的段
  # 这里对 -t# 而言 CanReturnArc=false，因此 needPhySize=true，
  # 会把每个命中段的物理长度 PhySize 抠出来，这就是分段长度来源。

  # 2) 主扫描循环：用哈希表在整份文件里前向找所有签名
  build hash table (见 2.3)
  pos = 0
  for (;;):                              # "Main Scan Loop" 2464
      加载 4MB 缓冲；跳过 [pos, pos+kBeforeSize) 里"不可能命中"的字节
      for each 候选格式 index 命中当前 pos 的签名:
          open = CreateArc(index)
          limitedStream = LimitedStream(stream, startArcPos=pos-SignatureOffset, rem)
          OpenArchiveSpec(open, needPhySize=true, limitedStream, searchLimit=?)
          if 成功:
              pi = {Offset=startArcPos, Size=PhySize}
              handlerSpec->AddItem(pi)          # 2573 / AddItem 253
              pos = pi.Offset + pi.Size          # 前进到该段之后
      if 没命中: pos++
  # 3) 收尾：把剩余尾部补成"未知段"
  handlerSpec->AddUnknownItem(fileSize)           # 2984
  if CanReturnParser:
      Archive = handlerSpec (NParser::CHandler); FormatIndex = -1; IsParseArc = true
```

### 2.2 判定"这里开始是一段归档 / 这段多长"

- **起点判定 = 纯签名扫描（magic bytes），不做解析**。每个格式在注册表里带 `Signatures`（一组 `(字节串, SignatureOffset)`）。扫描器把它们编进一张哈希表（见 2.3），只要当前位置的 2 字节（`kNumHashBytes=2`，即 `HASH_VAL(buf)=GetUi16(buf)`）在表里，就逐个 `TestSignature(buf, sig, sig.Size())` 精确比对。

- **长度判定 = 让被命中的那个 handler 自己报 `PhySize`**。`OpenArchiveSpec(needPhySize=true)`（`OpenArchive.cpp:1573`）：
  1. 调 `archive->Open(...)`；
  2. 读归档属性 `kpidPhySize`；
  3. **如果 handler 没在 Open 阶段给出 PhySize**（典型：bzip2 / gzip / 部分 xz），就补一刀 `archive->Extract(NULL, -1, testMode=true, cb)` —— 让它"空跑一遍全解压/全扫描"从而算出真实物理长度（源码注释：`It's for bzip2/gz and some xz archives`）。
  4. 若 `kpidPhySizeCantBeDetected=true`，则不试。

  所以分段的边界 = `[签名位置, 签名位置 + handler 报告的 PhySize)`。这就是 8.7GB 伪装 MP4 输出 `119248031 / 8592555306 / 14354` 的来历：
  - `[0, 119248031)` → 未知段（MP4 外壳/垃圾），命名 `1`
  - `[119248031, 119248031+8592555306)` → ZIP 段，命名 `2.zip`
  - `[8592555306+119248031, EOF)` 共 `14354` → 尾部未知段，命名 `3`

- **未知段**：由 `CHandler::AddUnknownItem(next)`（`OpenArchive.cpp:224`）在每次 `AddItem` 之前、以及最后（`fileSize`）自动填补空隙，保证分段无缝覆盖 `[0, FileSize)`。

### 2.3 哈希表与"逐字节扫描"的加速（问题 6 重点）

`OpenArchive.cpp:2340-2430`：

```
kNumHashBytes = 2 (非 WinCE)        # 1069-1075
HASH_VAL(buf) = GetUi16(buf)        # 取 2 字节
kBeforeSize = 1<<16 (64KB)          # 2362
kAfterSize  = 1<<20 (1MB)           # 2363
kBufSize    = 1<<22 (4MB)           # 2364, 必须 > kBefore+kAfter
kNumVals    = 1<<16                 # 各签名首 2 字节 -> 候选链表
```

- 为所有格式的所有签名建立 **首 2 字节 → 格式索引链表** 的散列（`hash[65536]` + `prevs[256]` 链）。所以主力格式签名必须 ≥2 字节，否则归入 `difficultFormats`（每位置都试）。
- 扫描时先做 1 字节宽度的"粗筛"：`for (; buf<bufLimit && hash[HASH_VAL(buf)]==0xFF; buf++);`（`2596`），一步跳过整片哈希表为空的字节——这是能在 8.7GB 上飞快扫过的关键。
- `difficultFormats`（`2424`）：**无有效签名 / 签名 <2 字节 / 带 `kStartOpen` / 旧接口** 的格式（如 TAR 的 `ustar` 在偏移 257、Base64、IHex、LZMA 等）。它们在每个位置都会被直接尝试一次，不靠哈希。

---

## 3. 为什么能输出 `1 / 2.zip / 3` 这样的分段名（问题 3）

`NParser::CHandler` 把"每一段"当自己的一条目（item），并在 `GetProperty`（`OpenArchive.cpp:318-360`）里这样造名字：

```cpp
case kpidPath:
{
  char sz[32]; ConvertUInt32ToString(index + 1, sz);   // 1-based 序号
  UString s(sz);                                       // "1"
  if (!item.Name.IsEmpty())      { s.Add_Dot(); s += item.Name; }       // ".name"
  if (!item.Extension.IsEmpty()) { s.Add_Dot(); s += item.Extension; }  // ".zip"
  prop = s;
}
```

- `Extension` = `ai.GetMainExt()`（`ReadParseItemProps`，`1387`），即该格式的主扩展名（zip / 7z / rar …）。
- `Name` = handler 通过 `kpidName` 归档属性报的名字（一般为空；tar 内嵌名等才会非空，且此时清空 Extension）。
- 未知段两者都空 → 只剩序号：`1`、`3`。
- 找出的 ZIP 段 → 序号 2 + `.zip` = `2.zip`。

**多段/嵌套**：`_items` 是一个 `CObjectVector<CParseItem>`（`OpenArchive.cpp:179`），主循环每命中一个内嵌归档就 `AddItem`，`AddUnknownItem` 自动补空隙。因此一个文件里塞多个 zip/7z/rar 会得到多个带扩展名的段；`GetStream(index)`（`436`）用 `CreateLimitedInStream(_stream, Offset, Size)` 把每段切出来——`7z x -t#` 时就是按此把每段原样 dump 成文件。**注意：`-t#` 本身只枚举"段"，不会自动再进 zip 里列成员**；要看 zip 内容需对导出的 `2.zip` 再跑一次（或用 `-t#r`）。

---

## 4. 关键源码位置表

| 文件:行 | 类/函数 | 作用 |
|---|---|---|
| `UI/Common/OpenArchive.cpp:45` | `kMaxCheckStartPosition = 1<<23` | **非扫描模式**下默认起始搜索上限 = 8MB（关键差异点） |
| `UI/Common/OpenArchive.cpp:61`(OpenArchive.h) | `struct COpenType` | 打开模式：`CanReturnParser/CanReturnArc/EachPos/MaxStartOffset` |
| `UI/Common/OpenArchive.cpp:117` | `namespace NArchive::NParser` | 虚拟归档命名空间 |
| `UI/Common/OpenArchive.cpp:119` | `struct CParseItem` | 一段：`Offset/Size/Name/Extension/ArcType/IsNotArcType/LenIsUnknown` |
| `UI/Common/OpenArchive.cpp:198` | `CHandler::FindInsertPos` | 按 Offset 二分找插入位 |
| `UI/Common/OpenArchive.cpp:224` | `CHandler::AddUnknownItem` | 用未知段填满空隙 |
| `UI/Common/OpenArchive.cpp:253` | `CHandler::AddItem` | 先补未知段再插入已知段 |
| `UI/Common/OpenArchive.cpp:318` | `CHandler::GetProperty` | **分段命名** `index+1[.name][.ext]` |
| `UI/Common/OpenArchive.cpp:402` | `CHandler::Extract` | 用 CCopyCoder 把每段原样按 `[Offset,Size)` 拷出 |
| `UI/Common/OpenArchive.cpp:436` | `CHandler::GetStream` | `CreateLimitedInStream(Offset,Size)` |
| `UI/Common/OpenArchive.cpp:1241` | `CArc::ReadBasicProps` | 读 `kpidPhySize/kpidOffset/kpidErrorFlags`，算 `ThereIsTail/TailSize` |
| `UI/Common/OpenArchive.cpp:1387` | `ReadParseItemProps` | 把 handler 的 `kpidName/kpidExtension/kpidMTime/kpidIsNotArcType` 灌进 CParseItem |
| `UI/Common/OpenArchive.cpp:1573` | `OpenArchiveSpec(needPhySize)` | 取 PhySize；拿不到就 `Extract(testMode)` 跑一遍 |
| `UI/Common/OpenArchive.cpp:1666` | `maxStartOffset` | `= MaxStartOffset_Defined ? MaxStartOffset : 8MB` |
| `UI/Common/OpenArchive.cpp:2092` | `if (formatIndex>=0 && !CanReturnParser)` | 扫描模式下不因"指定类型开失败"而提前放弃 |
| `UI/Common/OpenArchive.cpp:2118` | `Check all possible START archives` | 第一遍：按扩展名/签名顺序试各格式 |
| `UI/Common/OpenArchive.cpp:2340-2430` | PARSER 哈希构建 | `hash[65536]` + `prevs[256]`；`difficultFormats` |
| `UI/Common/OpenArchive.cpp:2464` | `Main Scan Loop` | 全文件前向扫描主循环 |
| `UI/Common/OpenArchive.cpp:2528` | `if (bytesInBuf-posInBuf < kAfterSize)` | 环形缓冲管理（保留前 64KB/后 1MB 重叠） |
| `UI/Common/OpenArchive.cpp:2576` | `if (!mode.CanReturnParser) {pos>maxStartOffset break}` | **扫描模式取消 8MB 限制，一直扫到 EOF** |
| `UI/Common/OpenArchive.cpp:2596` | `hash[...]==0xFF` 快跳 | 哈希粗筛加速 |
| `UI/Common/OpenArchive.cpp:2608` | `Open Loop for Current Pos` | 逐个候选格式试开当前 pos |
| `UI/Common/OpenArchive.cpp:2846` | `pos = pi.Offset + pi.Size` | 命中后跳过整段 |
| `UI/Common/OpenArchive.cpp:2981` | `if (mode.CanReturnParser)` | 收尾返回 NParser::CHandler（`FormatIndex=-1`） |
| `UI/Common/OpenArchive.cpp:3618` | `ParseType` `'#'` 分支 | **`-t#` 的判定入口** |
| `UI/Common/OpenArchive.cpp:3564` | `ParseTypeParams` | `e/a/r/s<size>` 子参数 |
| `Common/RegisterArc.h` | `REGISTER_ARC_*` 宏 | 各格式签名注册（见第 5 节） |
| `Archive/Common/FindSignature.h/.cpp` | `FindSignatureInStream` | 单签名前向查找（RAR/NSIS 等 handler 内部用） |
| `Archive/IArchive.h:59-80` | `NArcInfoFlags` | `kFindSignature`=1<<3、`kStartOpen`=1<<6、`kMultiSignature`=1<<4、`kPreArc`=1<<9 … |
| `Archive/Zip/ZipIn.cpp:618` | `CInArchive::FindMarker` | ZIP 定位；`searchLimit==0` 时走**弱判定**（只要求流首是 PK…） |
| `Archive/Zip/ZipIn.cpp:2530/2731` | `ReadHeaders` / LOCALS-CD-MODE | CD 失败后**退回顺序读 Local header** |
| `Archive/Zip/ZipIn.cpp:2076` | `CInArchive::ReadLocals` | 顺序解析所有 Local File Header |

---

## 5. 各格式签名表（问题 4）

**签名表在哪**：每个 handler 用 `CPP/7zip/Common/RegisterArc.h` 的宏（`REGISTER_ARC_I/IO/IO_DECREMENT_SIG/…`）把 `(name, ext, id, Signatures[], SignatureOffset, flags, isArcFunc)` 注册进全局 `CArcInfo` 表；`CCodecs`（`UI/Common/LoadCodecs.h`）汇总成 `Formats[]`，扫描器读其中的 `Signatures`。

宏语义：`SignatureOffset` = 签名相对归档起点的偏移（决定了 `startArcPos = pos - SignatureOffset`）；`kMultiSignature` = 一个格式多组签名；`kStartOpen`/`kFindSignature` = 影响是否进 `difficultFormats`。

### 5.1 你点名的格式（已核对源码）

| 格式 | 签名（hex） | 偏移 | 注册处 | flags 要点 |
|---|---|---|---|---|
| **ZIP** | `50 4B 03 04`(Local) / `50 4B 05 06`(Ecd) / `50 4B 06 06`(Ecd64) / `50 4B 07 08 50 4B`(Span) / `50 4B 30 30 50 4B`(NoSpan) | 0 | `Zip/ZipRegister.cpp:11-19` | `kFindSignature\|kMultiSignature\|kUseGlobalOffset` |
| **7z** | `37 7A BC AF 27 1C`（源码存 `{'7'+1,'z',0xBC,0xAF,0x27,0x1C}`，注册时 `DECREMENT_SIG` 把首字节减 1 → `36`→`37`） | 0 | `7z/7zRegister.cpp:12-14` | `kFindSignature` |
| **RAR (v1.5-4)** | `52 61 72 21 1A 07 00` (`Rar!\x1a\x07\x00`) | 0 | `Rar/RarHandler.cpp:48-49,1768` | `kFindSignature` |
| **RAR5** | `52 61 72 21 1A 07 01 00` (`Rar!\x1a\x07\x01\x00`) | 0 | `Rar/Rar5Handler.cpp:51-52,3399` | `kFindSignature` |
| **TAR** | `'u','s','t','a','r'`（UStar 魔数） | **257** (`kUstarMagic_Offset`) | `Tar/TarRegister.cpp:12-14` | `kStartOpen`（→ difficult） |
| **GZIP** | `1F 8B 08`（`1F`,`8B`,CM=8） | 0 | `GzHandler.cpp:38-40,1190` | `kKeepName`；有 `IsArc_Gz` |
| **XZ** | `FD 37 7A 58 5A 00`（`XZ_SIG`，定义在 `C/Xz.c:11`，声明 `C/Xz.h:63-66`） | 0 | `XzHandler.cpp:1445` | `kKeepName` |
| **BZIP2** | `42 5A 68` (`BZh`) | 0 | `Bz2Handler.cpp:472-474` | `kKeepName`；有 `IsArc_BZip2` |
| **ZSTD** | `28 B5 2F FD` | 0 | `ZstdHandler.cpp:429,1132/1139` | `kKeepName`；两个注册（`zstd` / `zstd2`） |

### 5.2 其余可提取格式（附带偏移/flags）

| 格式 | 签名 | 偏移 | 位置 |
|---|---|---|---|
| CAB | `4D 53 43 46 00 00 00 00` (`MSCF`) | 0 | `Cab/CabHeader.cpp:11`, `Cab/CabRegister.cpp:12` |
| ARJ | `60 EA` | 0 | `ArjHandler.cpp:186-187,968` |
| LZH/LHA | `2D 6C 68` (`-lh`) | **2** | `LzhHandler.cpp:710,712` |
| Z (compress) | `1F 9D` | 0 | `ZHandler.cpp:216-218` |
| PPMd | `8F AF AC 84` | 0 | `PpmdHandler.cpp:405-407` |
| ISO9660 | `43 44 30 30 31` (`CD001`) | `kStartPos+1`=**0x8001** | `Iso/IsoRegister.cpp:12-14`（`kStartPos=0x8000`, `IsoHeader.h:30`） |
| UDF | `00 42 45 41 30 31 01 00`(`BEA01`) + `01 43 44 30 30 31`(`CD001`)（多签名 8B/6B） | `kIsoStartPos`=**0x8000** | `Udf/UdfHandler.cpp:409-422` |
| NSIS | `EF BE AD DE 4E 75 6C 6C 73 6F 66 74 49 6E 73 74`（`\xEF\xBE\xAD\xDENullsoftInst`，16B） | **4** | `Nsis/NsisRegister.cpp:12`；`NsisIn.h:30` (`kFindSignature`)|
| XAR | `78 61 72 21` (`xar!`) | 0 | `XarHandler.cpp:1309` |
| RPM | `ED AB EE DB` | 0 | `RpmHandler.cpp:766-768` |
| DMG | `6B 6F 6C 79 00 00 00 04 00 00 02 00` (`koly`) | 0 | `DmgHandler.cpp:805,2328` |
| HFS/HFSX | `48 2B`/`48 58` | `kHeaderPadSize`(0x400) | `HfsHandler.cpp:2624` |
| NTFS | `4E 54 46 53 20 20 20 20 00` | 3 | `NtfsHandler.cpp:83,2935` |
| EXT2/3/4 | `53 EF` | **0x438** | `ExtHandler.cpp:2851,2853` |
| FAT | `55 AA` | **0x1FE** | `FatHandler.cpp:1316,1318` |
| APFS | `4E 58 53 42 00` (`NXSB`) | `kSignatureOffset` | `ApfsHandler.cpp:309,4463` |
| GPT | `45 46 49 20 50 41 52 54` (`EFI PART`) | **1<<9** | `GptHandler.cpp:466` |
| QCOW | `51 46 49 FB 00 00 00` (`QFI`) | 0 | `QcowHandler.cpp:32,706` |
| VDI | `7F 10 DA BE` | **0x40** | `VdiHandler.cpp:29,431` |
| VMDK | `4B 44 4D 56` (`KDMV`) | 0 | `VmdkHandler.cpp:39,1523` |
| Sparse | `3A FF 26 ED 01 00` | 0 | `SparseHandler.cpp:91,551` |
| CramFS | (magic) | **16** | `CramfsHandler.cpp:777` |
| SquashFS | `68 73 71 73`(`hsqs`)/`73 71 73 68`(`sqsh`)/`73 68 73 71`/`71 73 68 73`（4 组） | 0 | `SquashfsHandler.cpp:2360-2366` |
| LVM | `4C 41 42 45 4C 4F 4E 45` (`LABELONE`) | `kSectorSize`=**512** | `LvmHandler.cpp:31-34,1101` |
| Mach-O | multi | 0 | `MachoHandler.cpp:736` (`kPreArc`) |
| PE/EXE | `4D 5A` (`MZ`) | 0 | `PeHandler.cpp:2995-2997` (`kPreArc`) |
| ELF | `7F 45 4C 46` | 0 | `ElfHandler.cpp:1275-1277` (`kPreArc`) |
| FLV | `46 4C 56 01` | 0 | `FlvHandler.cpp:512-514` |
| SWF | `46 57 53` / `43 57 53` | 0 | `SwfHandler.cpp:987,989` |
| Compound/OLE | `D0 CF 11 E0 A1 B1 1A E1` | 0 | `ComHandler.cpp:1442` |
| CHM | `49 54 53 46 03 00 00 00 60 00 00 00` (`ITSF`) | 0 | `Chm/ChmHandler.cpp:764-766` |
| WIM | `4D 53 57 49 4D` (`MSWIM`) | 0 | `Wim/WimRegister.cpp:12` |
| CPIO | multi (`070701`/`070702`/`070707`) | 0 | `CpioHandler.cpp:1103` (`IsArc_Cpio`) |
| Base64 | **无签名**（按扩展名 + `IsArc_Base64`） | — | `Base64Handler.cpp:480` (`kByExtOnlyOpen`) |
| LZMA / LZMA86 | **无签名**（`IsArc_Lzma`） | — | `LzmaHandler.cpp:585,597` |
| IHex | **无签名**（`IsArc_Ihex`） | — | `IhexHandler.cpp:487` |
| MBR | **无签名** | — | `MbrHandler.cpp:642` (`kPureStartOpen`) |
| Split(001) | **无签名** | — | `SplitHandler.cpp:347` |
| UEFI Capsule/FFS | multi | 0 / `kFfsGuidOffset` | `UefiHandler.cpp:1853,1873` |

合计 60+ 个 handler 注册点（`grep -rn REGISTER_ARC CPP/7zip/Archive/` 得 ~64 处，含 zstd 双注册与注释行）。

---

## 6. ZIP「目录表偏移异常」为何 `-t#` 能过（问题 5）

**非扫描模式（普通 `7z x fake.mp4`）为什么失败：**

1. 文件无 zip 扩展名 → 按格式顺序试。
2. ZIP handler 的 `Open` 调用 `FindMarker(searchLimit)`，而 `searchLimit` = `maxStartOffset` = `kMaxCheckStartPosition` = **8MB**（`OpenArchive.cpp:45,1666`）。
3. `FindMarker` 带非零 limit 时是**强判定**：从起点往前扫 8MB 范围找 marker，找不到就 `return S_FALSE`（`ZipIn.cpp:3327-3345` `else if (!IsMultiVol) return res;`）。
4. 内嵌 ZIP 在 **119MB** 处，远超 8MB → 找不到 marker → 报"不是归档"。（即便找到 ECD，若 `cdOffset` 字段被篡改指向别处，CD-MODE 也会失败。）

**`-t#` 扫描模式为什么成功：**

1. `CanReturnParser=true` → `OpenArchive.cpp:2576` 的 `if (!mode.CanReturnParser)` 分支被跳过 → **扫描主循环一直扫到 EOF，没有任何 8MB 限制**。
2. 扫描器在 119248031 处命中 `PK\x03\x04`。
3. 为这个位置准备流并传给 handler：ZIP 带 `kUseGlobalOffset`，所以 limited 流覆盖 `[0,fileSize)` 再 `Seek(startArcPos)`（`OpenArchive.cpp:2697-2703`）；不带该 flag 的格式则建 `[startArcPos, rem)` 的 limited 流。两种情况下传给 handler 的 `searchLimit` 都 = **0**（`maxCheckStartPosition`）。于 handler 而言"当前流位置正好是 Local File Header"。
4. `FindMarker` 遇到 `*searchLimit==0` 时走**弱判定**（`ZipIn.cpp:629-655`）：只要流首 4 字节是 `kLocalFileHeader`/`kEcd`/`kEcd64` 之一即 `return S_OK`，并置 `MarkerIsFound=MarkerIsSafe=true`。
5. `ReadHeaders` 进入 marker 分支；marker = `kLocalFileHeader` → 先试 CD-MODE；因 CD 偏移异常而 `res != S_OK` → **退回 LOCALS-CD-MODE**（`ZipIn.cpp:2731` 起）：`ArcInfo.Base=0`，`ReadSignature()` 后调用 `ReadLocals(items)`（`2076`）**顺序解析所有 Local File Header**，跳过每条的 packed data，一直读到 `CentralFileHeader`。
6. 成功 → 报出 `PhySize`（`ZipIn.cpp:3222` 注释"we can't clear buf variables, we need them to calculate PhySize"）→ 得到段长度 8592555306。

**一句话**：`-t#` 用**自己的全文件签名前向扫描**绕过了 ZIP handler 的"从起点/末尾找 marker + 依赖 CD 偏移"路径；把 handler 强行摆到 Local header 上，再让 ZIP 的 **LOCALS-CD-MODE** 纯靠 Local header 顺序重建目录。同理，任何"靠 CD/索引定位、偏移被改坏"的容器（7z 的 NextHeader、RAR 的目录块等）在扫描模式下都更有可能用顺序解析路径成功。

---

## 7. 对我们的启示：unbox 的 Rust 扫描引擎该怎么设计

### 7.1 架构分层（照搬 7z 的思路）

```
1. SignatureRegistry   —— 静态表：Vec<Format>{ id, name, ext, signatures: Vec<(offset, bytes)>, flags }
2. SignatureIndex      —— 构建哈希：前 2 字节(u16 LE) -> Vec<(formatIdx, sigIdx)>
                          自实现 u16->SmallVec，或用 rustc-hash/ahash
3. Scanner             —— 前向扫描状态机 + 环形缓冲（保留前 overlap / 后 lookahead）
4. SegmentResolver     —— 对每个命中 pos：
                          a) 用「格式解析器」尝试在 startArcPos = pos - sig.offset 打开
                          b) 要求解析器返回 physical_len（PhySize）
                          c) 拿不到就用「流式预算」全跑一遍（等价 7z 的 Extract(testMode)）
                          d) 产出 Segment{ offset, len, format: Option<FormatId> }
5. SegmentList         —— 按 offset 排序 + 未知段填空，命名 index+1[.name][.ext]
```

### 7.2 关键设计点（踩坑清单）

- **前向哈希粗筛**必做。纯逐字节 memcmp 在 8.7GB 上不可接受。至少做 2 字节 u16 索引；条件允许上 3 字节或 SIMD `memchr`-风格首字节粗筛（对应 7z 的 `hash[HASH_VAL(buf)]==0xFF`）。这是性能命门。
- **环形缓冲 / 窗口**：7z 用 64KB 前置 + 1MB 后置重叠（`kBeforeSize/kAfterSize`），因为 ZIP 的 CD、7z 的 header 可能向前回读。Rust 版务必保留"签名位置之前的回看窗口"，否则带 `SignatureOffset>0`（TAR=257、EXT=0x438）或需要回读的格式会漏。
- **长度判定是本项目最硬的需求**。7z 靠 handler 报 `PhySize`（必要时全解压）。Rust 版两种策略：
  1. **结构解析**（首选）：ZIP 顺序读 Local header（跳过 compressed size）直到 Central Dir；7z 读 header + NextHeader 链；RAR 读块链。给出精确 end。
  2. **签名锚定 + 兜底启发式**（次选）：以"下一个已识别归档的签名"或"文件尾"为界，标 `LenIsUnknown`。这正是 7z `pi.LenIsUnknown` 的用法。
- **搜索模式分级**：模仿 `-t#` 的弱/强判定。扫描器交给解析器的流**从签名处开始**且**信任流首**（弱判定）能让"偏移被破坏"的容器走顺序解析路径——这正是我们要复刻 `-t#` 效果的核心。不要一上来就要求 CD/索引自洽。
- **跳过已识别段**：命中并定长后 `pos += len`，避免在同一次扫描里重复发现同一归档的内部签名（ZIP 内部还有很多 `PK\x03\x04`，若不跳过会炸出成百上千个假段）。
- **未知段填空**：Rust 版最终产出一个**无缝覆盖 [0, filesize)** 的 `Vec<Segment>`，便于"丢壳取核"。
- **difficult 格式**：TAR（签名在 257）、无签名格式（LZMA/Base64/IHex）需要"每位置尝试"或用 extra 启发式，别只靠哈希表；但可放在最后、加预算上限，避免拖慢主扫描。
- **候选格式的调用顺序**：优先"扩展名匹配 + 长签名 + `kFindSignature`"，再 `difficultFormats`。7z 用 `sortedFormats.Insert(0, ...)` 把签名命中的排前面。
- **伪 MP4 的现实**：MP4 是 box 结构，无固定全文魔数，所以外壳会自然落进"未知段"。我们的引擎要有意识地把 `ftyp/moov/mdat` 等 MP4 box 识别成"已知非归档外壳"以提升可读性（7z 里靠 `kpidIsNotArcType`/`kPreArc` 的概念）。

### 7.3 可复用的签名常量

第 5 节的表可直接落成 Rust 的 `const SIGNATURES: &[(&str, &[u8], u32)]`。建议起步就覆盖：ZIP(4 组)、7z、RAR、RAR5、TAR(off 257)、GZIP、XZ、BZ2、ZSTD、CAB、ARJ、LZH(off 2)、Z、ISO(off 0x8001)、EXT(off 0x438)、FAT(off 0x1FE)。移动端为了体积可只留常见 8~10 种。

---

## 8. 意外发现（问题 6 汇总）

1. **`-t#` 没有独立文件**：整个"扫描模式"就藏在 `OpenArchive.cpp` 里——`NParser::CHandler` 是个**内联在 UI/Common 的虚拟归档**，不属于 `Archive/` 下任何格式。
2. **`-t#` 是"打开模式"而非"格式"**：`ParseType` 把 `#` 识别成"非格式名"分支，仅置两个 bool。同族还有 `-t*`（`name[0]=='*'` 单独处理）。
3. **隐藏子参数**：`-t#:e`（逐字节）、`-t#:a`（允许整文件成归档）、`-t#:s8m`（限起始范围）、`-t#r`（递归）、`.` 组合多类型——命令行帮助里完全没写。
4. **长度是"逼 handler 全跑一遍"得来的**：`OpenArchiveSpec` 在 handler 不报 `PhySize` 时会调 `archive->Extract(NULL, -1, testMode=true)` 空跑全解压（`OpenArchive.cpp:1600` 附近，注释点名 bzip2/gz/xz）。这就是为什么 `-t#` 有时比想象中慢。
5. **哈希是 2 字节 u16**：`HASH_VAL = GetUi16`（`<<8` 的旧写法被注释掉），WinCE 才退化为 1 字节。
6. **ZIP 有 LOCALS-CD-MODE 兜底**：正常路径失败会自动"顺序读所有 Local header"，这是 `-t#` 能啃下坏 CD 的根因。
7. **7z 签名是"减 1 存储"**：`k_Signature_Dec = {'7'+1,...}`，注册宏 `REGISTER_ARC_IO_DECREMENT_SIG` 在构造时把首字节减 1 变回 `'7'`——运行时签名表里存的是 `36 7A …`、注册后才是 `37 7A …`（直接抄源码常量要注意这个 +1）。
8. **`kPReArc` / `kPreArc`**：EXE/ELF/Mach-O 标记为"可做外壳开头"，扫描时会特殊对待（`IsPreArcFormat`），这解释了 7z 对 SFX 自解压的处理思路。
9. **错误信息复用**：`IsArc_After_NonOpen()`——handler 虽 `S_FALSE` 但"确认这是归档"时，7z 会保留 `NonOpen_ErrorInfo` 用于给出更准确的报错；扫描模式下则借此继续。
10. **`8MB` 是个硬编码魔法数**（`kMaxCheckStartPosition = 1<<23`），正是普通模式"看不到 119MB 处内嵌包"的直接原因。Rust 版若想"既快又全"，应把这个上限做成可配置（或像 `-t#` 一样在"扫描模式"下取消）。

---

## 附录 A：`-t#` 分段输出语义速查

```
7z l -t# fake.mp4
 #  名称      大小(字节)       Offset        类型
 1           119248031        0             (未知外壳/MP4)
 2.zip       8592555306       119248031     zip
 3           14354            8711803337    (尾部)
```
- 名称列 = `序号[.主扩展名]`（`CHandler::GetProperty`）。
- 大小 = handler 报的 `PhySize`；未知段 = 到下一个段起点的距离。
- `7z x -t# fake.mp4` = 把每段按 `[Offset,Size)` 原样 dump 成 `1 / 2.zip / 3`（`CHandler::Extract` 用 `CCopyCoder`）。

## 附录 B：复刻用伪代码（Rust 版骨架）

```rust
fn scan_file(path) -> Vec<Segment> {
    let idx = SignatureIndex::build(&SIGNATURES);   // u16 -> [(fmt, sig)]
    let mut segs: Vec<Segment> = vec![];
    let mut pos = 0u64;
    let mut max_end = 0u64;
    while pos < file_len {
        // 1) 环形缓冲读入 [pos-kBefore, pos+kAfter]
        let window = read_window(pos);
        // 2) 哈希粗筛：跳过 u16 不在表里的字节
        while pos < file_len && !idx.contains(u16le(window, pos)) { pos += 1; }
        if pos >= file_len { break; }
        // 3) 逐个候选（含 difficult），按签名长/扩展名排序
        let mut opened = false;
        for (fmt, sig) in idx.candidates(u16le(window,pos)) {
            if !window.at(pos - sig.offset).starts_with(&sig.bytes) { continue; }
            let start = pos - sig.offset;
            let stream = LimitedStream::new(path, start, file_len - start);
            // 关键：流从 start 开始 + 弱判定，模仿 -t# 的 searchLimit==0
            if let Some(len) = fmt.try_open_and_get_phys_len(stream) {
                push_segment(&mut segs, start, len, Some(fmt), &mut max_end);
                pos = start + len;            // 跳过整段，避免内部假签名
                opened = true; break;
            }
        }
        if !opened { pos += 1; }
    }
    push_unknown(&mut segs, max_end, file_len); // 尾部未知段
    // 再把 segs 之间的空隙用未知段填满，得到无缝覆盖
    fill_unknowns(&mut segs, file_len);
    segs
}
```

---

*报告生成：基于 /home/user/hermes/yshjy/refs/7zip (tag 26.04) 源码逐个 grep + 阅读。所有行号均以该 checkout 为准。*
