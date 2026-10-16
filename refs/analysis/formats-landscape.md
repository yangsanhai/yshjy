# 压缩 / 归档格式全景 + 支持优先级（unbox 技术路线）

分析对象：`/home/user/hermes/yshjy/refs/7zip/`（7-Zip 26.04，`git 9128b80`）
数据来源：
- 第一部分：7-Zip 源码 `CPP/7zip/Archive/**` 的 `REGISTER_ARC*` 注册块 + 各 handler 的签名常量（逐个 `grep` + 读源码核对）。
- 第二 / 三部分：crates.io API（`https://crates.io/api/v1/crates`，经代理 `http://127.0.0.1:7890` 实测拉取）+ 联网检索核实（检索日期 2026-10-16）。
- 硬约束：**只用开源实现；RAR 只能解不能压**。

> 图标含义：`纯Rust=是` 表示不依赖 C/C++ 编译器/系统库；`纯Rust=否` 表示通过 `*-sys` FFI 绑定 C 库。

---

## 第一部分：7-Zip 全格式签名表

### 1.1 注册机制回顾

每个 handler 用 `CPP/7zip/Common/RegisterArc.h` 的宏把
`(name, ext, addExt, id, Signatures[], SignatureOffset, flags, isArcFunc)` 注册进全局 `CArcInfo` 表。
`Signatures[]` 是「分组字节串」格式：每个签名 = `[长度字节][签名数据...]`（`kMultiSignature` 时有多组）。
扫描器读 `Signatures` 构建 u16 哈希，`SignatureOffset` = 签名相对归档起点的偏移，决定 `startArcPos = pos - offset`。

- **总注册数：64 处宏调用（含 1 处被注释掉的 COFF 旧写法），实际生效 62 个 handler**（`zstd` 有 `zstd`/`zstd2` 双注册；`Chm`+`Hxs` 同文件双注册；`PE`+`COFF`+`TE` 同文件三注册）。
- 命令核实：`grep -rn "^\s*REGISTER_ARC" CPP/7zip/Archive/**`。

### 1.2 完整签名表（62 handler）

| # | 格式名 | 扩展名 | Magic bytes (hex) | 签名偏移 | 注册名(7z) | handler ID | flags 要点 |
|---|---|---|---|---|---|---|---|
| 1 | ZIP | zip z01 zipx jar apk odt ods docx xlsx epub ipa appx | `50 4B 03 04` / `50 4B 05 06` / `50 4B 06 06` / `50 4B 07 08` / `50 4B 30 30` | 0 | zip | 1 | kFindSignature, kMultiSignature, kUseGlobalOffset |
| 2 | 7z | 7z | `37 7A BC AF 27 1C` | 0 | 7z | 7 | kFindSignature（源码存 `'7'+1`，注册时 `DECREMENT_SIG` 减 1） |
| 3 | RAR (1.5–4) | rar r00 | `52 61 72 21 1A 07 00` (`Rar!\x1a\x07\x00`) | 0 | Rar | 3 | kFindSignature |
| 4 | RAR5 | rar r00 | `52 61 72 21 1A 07 01 00` (`Rar!\x1a\x07\x01\x00`) | 0 | Rar5 | 0xCC | kFindSignature |
| 5 | TAR | tar ova | `75 73 74 61 72` (`ustar`) | **0x101 (257)** | tar | 0xEE | kStartOpen, kSymLinks, kHardLinks |
| 6 | GZIP | gz gzip tgz tpz apk | `1F 8B 08` | 0 | gzip | 0xEF | kKeepName |
| 7 | BZIP2 | bz2 bzip2 tbz2 tbz | `42 5A 68` (`BZh`) | 0 | bzip2 | 2 | kKeepName |
| 8 | XZ | xz txz | `FD 37 7A 58 5A 00` | 0 | xz | 0xC | kKeepName |
| 9 | ZSTD | zst tzst | `28 B5 2F FD` | 0 | zstd / zstd2 | 0xe / 0xe+1 | kKeepName |
| 10 | CAB | cab | `4D 53 43 46 00 00 00 00` (`MSCF\0\0\0\0`) | 0 | Cab | 8 | kFindSignature |
| 11 | ISO9660 | iso img | `43 44 30 30 31` (`CD001`) | **0x8001** (`kStartPos+1`) | Iso | 0xE7 | — |
| 12 | UDF | udf iso img | `00 42 45 41 30 31 01 00` (`\0BEA01\x01\0`) + `01 43 44 30 30 31` | **0x8000** | Udf | 0xE0 | kMultiSignature, kStartOpen |
| 13 | WIM | wim swm esd ppkg | `4D 53 57 49 4D` (`MSWIM`) | 0 | wim | 0xE6 | kAltStreams,kNtSecure,kSymLinks |
| 14 | CPIO | cpio | `30 37 30 37 30 31`(5B `070701`) / `C7 71` / `71 C7` | 0 | Cpio | 0xED | kMultiSignature |
| 15 | AR | ar a deb udeb lib | `21 3C 61 72 63 68 3E 0A` (`!<arch>\n`) | 0 | Ar | 0xEC | — |
| 16 | RPM | rpm | `ED AB EE DB` | 0 | Rpm | 0xEB | — |
| 17 | DMG (UDIF) | dmg | `6B 6F 6C 79 00 00 00 04 00 00 02 00` (`koly`) | 0 | Dmg | 0xE4 | kBackwardOpen, kUseGlobalOffset |
| 18 | Compound/OLE | msi msp msm doc xls ppt aaf | `D0 CF 11 E0 A1 B1 1A E1` | 0 | Compound | 0xE5 | — (MSI/DOC/XLS 都走它) |
| 19 | CHM | chm chi chq chw | `49 54 53 46 03 00 00 00 60 00 00 00` (`ITSF`) | 0 | Chm | 0xE9 | — |
| 20 | HXS/Help2 | hxs hxi hxr hxq hxw lit | `49 54 4F 4C 49 54 4C 53 01 00 00 00 28 00 00 00` (`ITOLITLS`) | 0 | Hxs | 0xCE | kFindSignature |
| 21 | NSIS | nsis | `EF BE AD DE 4E 75 6C 6C 73 6F 66 74 49 6E 73 74` (`\xEF\xBE\xAD\xDENullsoftInst`) | **4** | Nsis | 9 | kFindSignature, kUseGlobalOffset |
| 22 | LHA/LZH | lzh lha | `2D 6C 68` (`-lh`) | **2** | Lzh | 6 | — |
| 23 | ARJ | arj | `60 EA` | 0 | Arj | 4 | — |
| 24 | Z (compress) | z taz | `1F 9D` | 0 | Z | 5 | — |
| 25 | PPMd | pmd | `8F AF AC 84` | 0 | Ppmd | 0xD | — |
| 26 | XAR | xar pkg xip | `78 61 72 21 00` (`xar!\0`) | 0 | Xar | 0xE1 | — |
| 27 | EXT2/3/4 | ext ext2 ext3 ext4 img | `53 EF` | **0x438** | Ext | 0xC7 | — |
| 28 | FAT | fat img | `55 AA` | **0x1FE** | FAT | 0xDA | — |
| 29 | NTFS | ntfs img | `4E 54 46 53 20 20 20 20 00` (`NTFS    \0`) | **3** | NTFS | 0xD9 | — |
| 30 | HFS/HFS+/HFSX | hfs hfsx | `42 44`(BD) / `48 2B 00 04`(H+) / `48 58 00 05`(HX) | **0x400** | HFS | 0xE3 | kMultiSignature |
| 31 | APFS | apfs img | `4E 58 53 42 00` (`NXSB\0`) | **32 (0x20)** | APFS | 0xC3 | — |
| 32 | GPT | gpt mbr | `45 46 49 20 50 41 52 54 00 00 01 00` (`EFI PART`) | **0x200 (1<<9)** | GPT | 0xCB | — |
| 33 | APM | apm | `45 52` (`ER`) | 0 | APM | 0xD4 | — |
| 34 | MBR | mbr | 无签名（`kPureStartOpen`） | — | MBR | 0xDB | kPureStartOpen |
| 35 | QCOW | qcow qcow2 qcow2c | `51 46 49 FB 00 00 00` (`QFI\xFB`) | 0 | QCOW | 0xCA | — |
| 36 | VDI | vdi | `7F 10 DA BE` | **0x40** | VDI | 0xC9 | — |
| 37 | VHD | vhd | `63 6F 6E 65 63 74 69 78 00 00 00` (`conectix`) | 0 | VHD | 0xDC | kUseGlobalOffset |
| 38 | VHDX | vhdx avhdx | `76 68 64 78 66 69 6C 65` (`vhdxfile`) | 0 | VHDX | 0xC4 | — |
| 39 | VMDK | vmdk | `4B 44 4D 56` (`KDMV`) | 0 | VMDK | 0xC8 | — |
| 40 | Sparse image | simg img | `3A FF 26 ED 01 00` | 0 | Sparse | 0xC2 | — |
| 41 | LVM | lvm | `4C 41 42 45 4C 4F 4E 45` (`LABELONE`) | **512** | LVM | 0xBF | — |
| 42 | LP (Linux / LUKS part.) | lpimg img | `67 44 6C 61 34 00 00 00` (`gDla4`) | 保留区 | LP | 0xC1 | — |
| 43 | CramFS | cramfs | `43 6F 6D 70 72 65 73 73 65 64 20 52 4F 4D 46 53` (`Compressed ROMFS`) | **16** | CramFS | 0xD3 | — |
| 44 | SquashFS | squashfs | `68 73 71 73`(hsqs) / `73 71 73 68`(sqsh) / `73 68 73 71` / `71 73 68 73` | 0 | SquashFS | 0xD2 | kMultiSignature |
| 45 | UEFI Capsule | scap | GUID `BD8666 3B760D30 40B70EB5 519E2FC5A0` 等 3 组(16B) | 0 | UEFIc | 0xD0 | kMultiSignature, kFindSignature |
| 46 | UEFI FFS | uefif | FFS1 `D954937A…` / FFS2 `78E58C8C…` (16B) | **16** | UEFIf | 0xD1 | kMultiSignature, kFindSignature |
| 47 | Mach-O | macho | `CE FA ED FE` / `CF FA ED FE` / `FE ED FA CE` / `FE ED FA CF` | 0 | MachO | 0xDF | kMultiSignature, kPreArc |
| 48 | PE/EXE | exe dll sys | `4D 5A` (`MZ`) | 0 | PE | 0xDD | kPreArc |
| 49 | COFF | obj | 无签名（机器类型字段） | — | COFF | 0xC6 | kStartOpen |
| 50 | TE | te | `56 5A` (`VZ`) | 0 | TE | 0xCF | kPreArc |
| 51 | ELF | elf | `7F 45 4C 46` | 0 | ELF | 0xDE | kPreArc |
| 52 | FLV | flv | `46 4C 56 01` (`FLV\x01`) | 0 | FLV | 0xD6 | — |
| 53 | SWF | swf | `46 57 53` (FWS) | 0 | SWF | 0xD7 | kKeepName |
| 54 | SWFc (压缩) | swf | `43 57 53`(CWS) / `5A 57 53`(ZWS) | 0 | SWFc | 0xD8 | kMultiSignature |
| 55 | MsLZ | mslz | `53 5A 44 44 88 F0 27 33 41` (`SZDD…`9B) | 0 | MsLZ | 0xD5 | — |
| 56 | MUB | mub | `CA FE BA BE 00 00 00`(7B) / `B9 FA F1 0E` | 0 | Mub | 0xE2 | kMultiSignature |
| 57 | AVB | avb img | `41 56 42 66 00 00 00 01` (`AVBf…`) | 0 | AVB | 0xC0 | kBackwardOpen |
| 58 | Base64 | b64 | 无签名 | — | Base64 | 0xC5 | kStartOpen, kByExtOnlyOpen, kKeepName |
| 59 | LZMA | lzma | 无签名 | — | lzma | 0xA | kStartOpen, kKeepName |
| 60 | LZMA86 | lzma86 | 无签名 | — | lzma86 | 0xB | kKeepName |
| 61 | IHex | ihex | 无签名（文本 `:`） | — | IHex | 0xCD | kStartOpen |
| 62 | Split | 001 | 无签名 | — | Split | 0xEA | — |

> ID 即 `CArcInfo.ID`（`Archive/IArchive.h` / `RegisterArc.h` 的第四参数字节）；`0xEF`、`0xEE`、`0xE7` 等与上表一致。
> `kPreArc`（PE/ELF/Mach-O/TE）= 「可作 SFX 自解压外壳开头」，扫描时特殊对待。
> **注意**：7z 的签名常量在源码里存成 `{'7'+1, 'z', …}`（`36 7A …`），由 `REGISTER_ARC_IO_DECREMENT_SIG` 在注册时把首字节减 1 还原为 `37`——直接抄常量务必 +1/-1 验证。

### 1.3 与「常见归档」无关但值得注意的 7-Zip 能力

7-Zip 的「格式」远不止压缩包：它内置了**磁盘镜像/文件系统**（EXT/FAT/NTFS/HFS/APFS/SquashFS/CramFS/UDF/ISO/LVM/QCOW/VHD/VHDX/VDI/VMDK/Sparse/GPT/MBR/APM）、**可执行文件容器**（PE/COFF/TE/ELF/Mach-O）、**固件**（UEFI Capsule/FFS、Intel ME）、**多媒体容器**（FLV/SWF）、**编码文本**（Base64/IHex）。unbox 若复用这套签名表做「万能格式侦察」，覆盖面极广，但**不要**把「能识别」等同于「要支持解包」。

---

## 第二部分：市面压缩 / 归档格式全景（四层普及度）

分层依据：Windows 2023 起原生支持 zip/rar/7z/tar/gz；Linux 默认工具链；Cofense 2024 归档流量统计（zip 占 >50%，img/tar/rar/7z/gz 合计超其余一半）。普及层 = 现在几乎每个人每天都会遇到；上古层 = 90 年代 BBS/DOS/Mac 时代产物，基本绝迹，仅见于考古/归档库。

### 2.1 普及层（必备）

| 格式 | 类型 | 扩展名 | 典型用途 | 现状 |
|---|---|---|---|---|
| ZIP | 归档+压缩 | .zip | 通用分发、Office/JAR/APK/EPUB 内核 | 事实标准，OS 原生支持 |
| 7z | 归档+压缩 | .7z | 高分压缩、软件分发 | 开源、跨平台，主流 |
| RAR | 归档+压缩 | .rar .r00 | 游戏/破解/下载站 | **专有，只能解不能压** |
| TAR | 纯归档 | .tar | Unix/Linux 打包 | 无压缩，配 *.gz/.xz/.zst |
| GZIP | 单流压缩 | .gz .tgz | 网页、日志、Linux 包 | 极普及 |
| BZIP2 | 单流压缩 | .bz2 .tbz2 | 文本压缩 | 普及但被 xz 挤占 |
| XZ | 单流压缩 | .xz .txz | Linux 发行版包、源码分发 | 主流（LZMA2） |
| ZSTD | 单流压缩 | .zst .tzst | 新项目默认、数据库、容器 | 上升最快 |
| LZ4 | 单流压缩 | .lz4 | 实时/低延迟、内核 | 常见（速度优先） |
| Brotli | 单流压缩 | .br | HTTP 内容编码、字体 | 浏览器/服务器常见 |

### 2.2 常见层（应支持）

| 格式 | 类型 | 扩展名 | 备注 |
|---|---|---|---|
| CAB | 归档+压缩 | .cab | Windows 安装包/驱动 |
| ISO | 光盘镜像 | .iso | 系统/软件分发 |
| CPIO | 纯归档 | .cpio | initramfs、RPM 内核 |
| RPM | 包 | .rpm | RedHat 系（CPIO+头部） |
| DEB | 包 | .deb | Debian 系（ar+tar） |
| AR | 纯归档 | .a .deb .lib | Unix 静态库、deb 外层 |
| WIM | 镜像 | .wim .esd | Windows 部署 |
| MSI | 安装包 | .msi .msp | 实为 OLE Compound |
| UDF | 文件系统 | .udf | DVD/蓝光 |
| DMG | 磁盘镜像 | .dmg | macOS 分发（UDIF） |

### 2.3 少见层（可选 / 视场景）

| 格式 | 扩展名 | 现状 | 备注 |
|---|---|---|---|
| LHA / LZH | .lha .lzh | 日系 / 复古仍偶见 | 有成熟 Rust 实现 |
| ACE | .ace | 2000 年代，已过时 | 旧算法可解 |
| ARJ | .arj | DOS 时代 | 复古场景 |
| Zoo | .zoo | 上古 | 罕见 |
| ARC | .arc | PEM/BBS 时代 | 与 FreeArc 的 .arc 不同 |
| StuffIt (SIT) | .sit .sitx | 经典 Mac | 复古 Mac |
| ZPAQ | .zpaq | 小众但活跃 | 极限压缩、增量备份 |
| FreeArc | .arc | **项目 2016 后停更** | 兼容性差 |
| LZFSE | （Apple 流） | Apple 生态 | LZVN/LZFSE |
| Snappy | （Google 流） | 库格式非文件 | 存储/传输压缩 |
| LZO | .lzo | 内核/嵌入式 | — |
| Lzip | .lz | 小众 Unix | LZMA 容器 |
| lzop | .lzo | 上古 Linux | LZO 容器 |
| PAQ 家族 | .paq8 .paq9 | 研究性 | 极限压缩，格式常不兼容 |

### 2.4 上古 / 几乎灭绝层（多数建议放弃）

| 格式 | 扩展名 | 现状 |
|---|---|---|
| Compress (.Z) | .Z | Unix 上古，仍见于老数据 |
| SHAR | .shar | shell 自解压脚本，几乎绝迹 |
| BTOA | .btoa | ASCII 二进制编码，绝迹 |
| XAR | .xar .pkg | macOS 旧包格式，仍偶见 |
| RAR1–4 | .rar | 被 RAR5 取代，但存量庞大 → **仍要能解** |
| StuffIt5 / SITX | .sitx | 绝迹 |
| CPIO (old) | .cpio | 旧二进制变体（0o70707 等） |
| HA | .ha | 上古（Harri Hirvola） |
| LHarc | .lzh | LHA 前身 |
| LArc | .lzs | 上古 |

---

## 第三部分：Rust 生态实现对照表 + 支持优先级

### 3.1 Rust 实现对照总表

数据（crates.io API 实测，2026-10-16）。`纯Rust` 列：✅=无 C 依赖，❌=FFI 绑定。

| 格式 | 推荐 crate | 版本 | 许可 | 纯Rust | 备注 |
|---|---|---|---|---|---|
| **ZIP** | `zip` | 9.0.4 | MIT | ✅ | 读+写，deflate 默认走 `flate2/miniz_oxide` |
| ZIP(备) | `rc-zip` / `async_zip` | 5.4.1 / 0.0.19 | MIT/Apache | ✅ | rc-zip 面向流式；async_zip 异步 |
| **7z** | `sevenz-rust2` | 0.24.0 | Apache-2.0 | ✅ | 维护活跃（`sevenz-rust` 后继），读写 LZMA/LZMA2 |
| **RAR** | `rars` | 0.11.0 | Apache-2.0 | ✅ | 纯 Rust，读+写，覆盖 RAR1.5→RAR7、多卷、加密 |
| RAR(备) | `unrar` | 0.5.8 | MIT/Apache（**底层 UnRAR 非自由**） | ❌ | FFI；许可**禁止**用于造 RAR 压缩器 |
| RAR(备2) | `rar` | 0.4.0 | MIT | ✅ | 功能有限，仅解压 |
| **TAR** | `tar` | 0.4.46 | MIT/Apache | ✅ | 事实标准 |
| **GZIP** | `flate2` | 1.1.10 | MIT/Apache | ✅ | miniz_oxide 后端纯 Rust；也可切 zlib-ng |
| GZIP(备) | `libflate` | 2.3.2 | MIT | ✅ | 纯 Rust 全实现 |
| **BZIP2** | `bzip2` | 0.6.1 | MIT/Apache | ❌ | 绑定 libbzip2 |
| BZIP2(纯) | `bzip2-rs` | 0.1.2 | MIT/Apache | ✅ | **仅解压** |
| **XZ** | `liblzma` | 0.4.8 | MIT/Apache | ❌ | 绑定 liblzma（`xz2` 是其前身 0.1.7） |
| XZ(纯) | `lzma-rs` | 0.3.0 | MIT | ✅ | 纯 Rust LZMA/LZMA2/XZ 解码 |
| **ZSTD** | `zstd` | 0.14.2 | BSD-3 | ❌ | 绑定官方 libzstd（性能最佳） |
| ZSTD(纯) | `ruzstd` | 0.9.1 | MIT | ✅ | 纯 Rust 解码（+实验压缩） |
| **LZ4** | `lz4_flex` | 0.14.0 | MIT | ✅ | 纯 Rust，最快；`lz4` 1.28.1 为 FFI 版 |
| **Brotli** | `brotli` | 9.0.0 | BSD-3/MIT | ✅ | 纯 Rust；`brotli-decompressor` 6.1.0 仅解 |
| **CAB** | `cab` | 0.6.0 | MIT | ✅ | 读+写；备 `ms-cabinet` 0.1.5 |
| **ISO9660** | `iso9660` | 0.1.1 | MIT/Apache | ✅ | 功能不全；备 `cdfs` 0.2.3 |
| **CPIO** | `cpio` | 0.4.1 | MIT | ✅ | ⚠️ 描述为「创建」，读支持需实测 |
| **RPM** | `rpm` | 0.30.2 | Apache/MIT | ✅ | 纯 Rust，读写（`rpm-rs` 0.8.1 为同名历史库） |
| **DEB** | `deb` | 0.5.20 | MIT | ✅ | 上层；底层需 `ar` + `tar` + `xz/zstd` |
| **AR** | `ar` | 0.9.0 | MIT | ✅ | 纯 Rust |
| **WIM** | `wim` | 0.1.3 | LGPL-2.1/GPL-3.0 | ✅ | 许可为 copyleft，注意 |
| **MSI** | `msi` | 0.10.0 | MIT | ✅ | 读+写（OLE Compound 容器） |
| **UDF** | ❌ 无可用 crate | — | — | — | `udf` crate 是 **MySQL UDF**，与 UDF 文件系统无关 |
| **DMG** | `udif` | 0.4.2 | MIT | ✅ | 跨平台 UDIF；`dmg` 0.1.2 只是挂载 |
| **LHA/LZH** | `delharc` | 0.8.0 | MIT/Apache | ✅ | 成熟，解压 |
| **ACE / ARJ / ZOO / ARC / HA / SQ / HYP…** | `unarc-rs` | 0.7.2 | MIT/Apache | ✅ | 一揽子复古格式（7z/ZIP/RAR/LHA/TAR/ACE/ARJ/ARC/ZOO/HA/JAR/UC2/SQ/SQZ/HYP/Z…） |
| **DOS/CP-M 上古** | `newtua-dos` | 0.1.4 | **LGPL-3.0** | ✅ | Squeeze/ARC/LBR/Crunch… |
| **经典 Mac** | `newtua-mac` / `newtua-stuffit` | 0.1.1 / 0.1.2 | **LGPL-3.0** | ✅ | BinHex/MacBinary/SIT |
| **StuffIt** | `stuffit` | 0.4.0 | MIT/Apache-2.0 | ✅ | .sit 解析+解压+创建 |
| **BinHex4** | `binhex4` | 0.1.2 | MIT | ✅ | 编解码 |
| **ZPAQ** | `omnizip-zpaq` | 0.21.117 | MIT/Apache-2.0 | ✅ | 纯 Rust context-mixing；备 `zpaq_rs` 1.0.4（FFI libzpaq） |
| **FreeArc** | ❌ 无 crate | — | — | — | 项目停更、格式无 Rust 实现 |
| **LZFSE** | `lzfse_rust` | 0.2.1 | MIT/Apache | ✅ | 纯 Rust；`lzfse` 0.2.0 为 FFI |
| **Snappy** | `snap` | 1.1.2 | BSD-3 | ✅ | 事实标准，纯 Rust |
| **LZO** | `rust-lzo1x` | 0.4.0 | MIT | ✅ | 纯 Rust LZO1X；`lzo` 0.1.4（Apache，仅解）；`minilzo-rs` 0.6.1 是 **GPL-3.0**，避用 |
| **Lzip** | `lzip` | 0.1.1 | MIT/Apache | ❌ | 绑定 lzlib，下载量极低（3k） |
| **lzop** | ❌ 无容器 crate | — | — | — | 需自写 lzop 头 + 复用 `rust-lzo1x` 解 LZO |
| **Compress (.Z)** | `newtua-lzw-z` | 0.1.0 | MIT/Apache | ✅ | 纯 Rust 解 .Z/LZC；备 `lzw` 0.10.0（通用 LZW） |
| **XAR** | `xar` | 0.1.1 | MIT | ✅ | 纯 Rust，读写 |
| **SHAR** | ❌ 无 crate | — | — | — | 本质是 shell 脚本，按文本解析即可 |
| **BTOA** | ❌ 无对口 crate | — | — | — | `btoa` crate 是 ASCII85，不符；自实现简单 |
| **全能兜底** | `libarchive2` | 0.2.1 | MIT/Apache | ❌ | 绑定 libarchive 3.8.1，一次覆盖 40+ 格式 |
| 全能(纯) | `libarchive_oxide` | 0.2.0 | MIT/Apache | ✅ | 纯 Rust 检测/压缩/解压，覆盖面待评估 |
| 全能(TS) | `compcol` | 0.7.2 | MIT | ✅ | no_std 压缩算法集合（流式 trait） |

**关键结论（RAR 许可）**：
- UnRAR 原始许可（`unrar` crate 底层）：「可用于处理 RAR 归档，但**不得**用于开发 RAR 兼容压缩器 / 重现 RAR 压缩算法」，且非 OSI 自由许可（Fedora 明确禁用）。
- **好消息**：`rars`（Apache-2.0、纯 Rust）已能读 RAR 1.5→RAR7。我们只需**只用其读路径**，即可在满足「开源 + 只解不压」约束下摆脱非自由 FFI。
- 结论：主用 `rars` 读；`unrar` FFI 仅作边界兼容兜底（并保留许可声明）；**绝不暴露任何 RAR 写/压缩能力**。

### 3.2 支持优先级建议（四档）

#### 🟢 v0.1 必备（8 个家族，覆盖现实文件 >95%）

| 格式 | 理由 | 选用 crate |
|---|---|---|
| **ZIP** | 全世界最常见（流量占比 >50%），Office/JAR/APK/EPUB 内核 | `zip` |
| **7z** | 开源高压缩事实标准 | `sevenz-rust2` |
| **RAR（仅解压）** | 存量巨大，用户强需求；受约束只能解 | `rars`（纯 Rust，read-only） |
| **TAR** | Unix 打包基座，与压缩层组合 | `tar` |
| **GZIP** | tar.gz / 网页 / 日志，最普及的流压缩 | `flate2` |
| **XZ** | Linux 包与源码分发主流 | `liblzma` + `lzma-rs` 兜底 |
| **ZSTD** | 增速最快的新默认 | `zstd` + `ruzstd` 兜底 |
| **BZIP2** | 存量 tar.bz2，成本极低 | `bzip2` + `bzip2-rs` 兜底 |

> v0.1 只需这 8 个：它们构成「现实中最可能遇到的压缩/归档」的最小完备集，且每个都有成熟且许可清晰的实现（唯一需留意许可的是 RAR——用 `rars` 规避）。

#### 🟡 应支持（第二波，v0.2–v0.5）

`LZ4`、`Brotli`（普及层补齐）→ `CAB`、`ISO`、`DEB`、`RPM`、`AR`、`MSI`、`WIM`、`CPIO`（常见层，多为 unpack 场景）。

#### 🔵 可选（有明确场景再上）

`LHA/LZH`（`delharc`）、`DMG`（`udif`）、`UDF`（**无 crate，需自研或绑 libarchive**）、`XAR`（`xar`）、`ACE/ARJ/ZOO/ARC/HA`（一把梭 `unarc-rs`）、`LZFSE`（`lzfse_rust`）、`Snappy`（`snap`）、`ZPAQ`（`omnizip-zpaq`）、`Compress .Z`（`newtua-lzw-z`）。

#### ⚫ 放弃（附理由）

| 格式 | 放弃理由 |
|---|---|
| **FreeArc** | 项目 2016 年停更、无 Rust 实现、格式无公开稳定规范 |
| **PAQ/PAQ8/PAQ9** | 研究性、格式常不向后兼容、无维护实现 |
| **StuffIt5/SITX** | 绝迹；crate 覆盖未知 |
| **SHAR / BTOA** | 非二进制归档（shell 脚本 / ASCII 编码），无维护价值 |
| **LHarc / LArc / HA** | 被 LHA/现代格式完全取代，考古价值高于工程价值 |
| **GCA / BGA**（日本上古压缩） | 极冷门，无 Rust 实现，用户量可忽略 |
| **lzop 容器 / Lzip** | 生态几乎为零（`lzip` crate 仅 3k 下载），收益远低于成本 |
| **RAR 压缩** | 触犯许可约束，永久排除写路径 |

### 3.3 工程建议（落地要点）

1. **统一抽象**：定义 `trait Unpacker { fn sniff(&[u8]) -> Confidence; fn list(..); fn extract(..); }`，归档类（ZIP/TAR/7z…）与单流压缩类（gz/xz/zst…）分成两个 trait 家族，与 7-Zip 的 `IsArc_*` 区分思路一致。
2. **FFI vs 纯 Rust 策略**：核心必备格式优先**纯 Rust**（可交叉编译、无系统依赖）：ZIP/7z/TAR/GZIP/XZ(lzma-rs)/ZSTD(ruzstd)/BZIP2(bzip2-rs)/RAR(rars)。性能敏感时再提供 FFI 后端（`zstd`/`liblzma`/`bzip2`）作为可选 feature。
3. **许可红线**：避开 `minilzo-rs`(GPL-3.0)、`wim`(LGPL/GPL)、`newtua-*`(LGPL-3.0) 作为默认依赖；如引入需隔离进程或明确许可声明。FFI 的 `unrar` 需随包附带 UnRAR 许可全文。
4. **签名表复用**：把第一部分 62 条签名直接落成 Rust 常量表（注意 7z 的 ±1、TAR 的 offset=257、ISO/UDF 的 0x8000 系、FAT 的 0x1FE 等），即可零成本获得「万能格式侦察」能力——这正好对接已有的 `7zip-scan.md` 扫描引擎设计。
5. **长度判定**：单流格式（gz/xz/zst/bz2）没有尾部索引，必须靠「解压到流结束」或「下一段签名/EOF」定长（参见 `7zip-scan.md` 第 2.2 节 7z 的 `Extract(testMode)` 做法）。

---

## 附：统计口径

- **7-Zip 支持格式数**：62 个 handler（64 处注册宏，含 1 处注释）。
- **四层普及度清单**：普及层 10 + 常见层 10 + 少见层 15 + 上古层 10 = 45 个条目。
- **Rust 实现对照**：覆盖 40+ 格式 / 50+ crate；其中纯 Rust 可用约 35 个格式，无可用 Rust 实现的是 UDF、FreeArc、SHAR、BTOA、lzop 容器。
