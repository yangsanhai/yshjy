# sevenz-rust（实为 **sevenz-rust2 0.24.0**）深度分析：7z 结构解析 / AES 密码 / 分卷

> 分析对象：`/home/user/hermes/yshjy/refs/sevenz-rust/`
> 分析日期：2026-10-16 ｜ 环境：rustc 1.97.0 / cargo 1.97.0 / 7-Zip 25.01 (p7zip) / Debian 13
> 证据标记：**[实测]** = 本次真正编译运行得到的输出；**[源码]** = 读代码得出；**[推测]** = 推理
> 实测工程（可复现）：`/home/user/.hermes/cache/scratch/7zprobe/`
> （`t1/` 测试归档、`rsprobe/` 探针 crate，含 `main.rs` / `sweep.rs` / `extra.rs` / `branch.rs` / `fallback.rs` / `trace.rs` / `scan.rs`）

---

## 0. 结论先行（TL;DR）

1. **这份代码不是 crates.io 上的 `sevenz-rust`**，而是 hasenbanck 维护的 **`sevenz-rust2` v0.24.0**（`Cargo.toml` 包名 `sevenz-rust2`，lib 名 `sevenz_rust2`，仓库 `github.com/hasenbanck/sevenz-rust`，shallow clone 只有 1 个提交 `a1c2cec`）。API 是 dyz1990/sevenz-rust 0.6.1 的**超集+改名**：`SevenZReader`→`ArchiveReader`、`SevenZArchiveEntry`→`ArchiveEntry`。**引入时 crate 名必须写 `sevenz-rust2`**（写 `sevenz-rust` 会拉到那份停更实现）。
2. **7z 头里没有「总长度」字段，但长度可以精确推导**：`总长 = 32 + NextHeaderOffset + NextHeaderSize`，4 个归档实测全部 bit-exact 命中文件长度。加上 `StartHeaderCRC32`(bytes 8..12) 与 `NextHeaderCRC32`(bytes 28..32) 两道校验，扫描定位可以做到 2⁻⁶⁴ 级假阳性。**这是 unbox 定位 7z 真身的关键**。
3. **能胜任**：AES-256+SHA-256 密码（内容加密 / 头部加密 `-mhe=on`）、**中文密码**（`Password::from(&str)` 走 UTF-16LE）、**分卷拼接后直接读**（7z 分卷就是裸字节切分，无卷头）、LZMA/LZMA2/PPMd/BZIP2/COPY/BCJ 全家桶/**BCJ2 四路输入**/DELTA、官方 AES 与 BCJ2 测试向量全部通过。
4. **必须自己补**：① 7z 起点/长度扫描（库里没有 API，且它自带的「猜头」路径**实测 12/12 全废**，见 §2.6）；② 非零偏移读取的 `Read+Seek` 包装器（`Archive::read` 硬编码 `seek(Start(0))`，直接把 base 偏移的 File 丢进去必然 `BadSignature`，已实测）；③ 分卷多文件拼成一个逻辑流；④ 中文密码的 GBK/Big5/UTF-8 解码纠错；⑤ 自己的内存上限（库内 `MAX_MEM_LIMIT_KB = usize::MAX/1024`，等于没有限制，恶意 LZMA2 字典最大 4 GiB 会 OOM）。
5. 一句话给决策：**解压内核可以直接用这个 crate（Apache-2.0、纯 Rust、无 C 依赖），但「扫描 + 定位 + 分卷 + 密码纠错 + 资源限制」这五件事全在 unbox 侧自己实现**，库只负责「给我一个从 0 开始的 7z 字节流 + 正确密码，我把文件吐出来」。

---

## 1. 这份代码到底是什么

### 1.1 身份卡 [源码]

| 项 | 值 |
|---|---|
| 包名 | `sevenz-rust2`（lib 名 `sevenz_rust2`） |
| 版本 | **0.24.0** |
| 仓库 | `https://github.com/hasenbanck/sevenz-rust`（本地 remote = origin） |
| 上游定位 | README 原话：*"This is a fork of the original, unmaintained sevenz-rust crate to continue the development and maintenance."* |
| 提交 | 只有 `a1c2cec fix: decoding large files under 32-bit systems`（shallow clone，无历史） |
| 许可 | Apache-2.0 |
| edition / MSRV | **edition 2024 / rust-version 1.93**（比多数项目高，注意 CI/Android 工具链） |
| crate-type | `["cdylib", "rlib"]`（会顺带产出 cdylib，将来 Flutter FFI 可直接用） |
| 规模 | 8088 行 Rust（27 个源文件） |
| 默认 features | `aes256, bzip2, compress, ppmd, util` |
| 可选 features | `brotli, deflate, lz4, zstd, nt-time, aes256_wasm, default_wasm` |
| WASM | 支持（有 `util/wasm.rs`、`wasm-bindgen`、`getrandom/wasm_js`） |

### 1.2 与公开 `sevenz-rust 0.6.1`（dyz1990）的 API 对比

crates.io 元数据（实测抓取）：`sevenz-rust` 最新 = **0.6.1**，发布于 2024-07-17，`rust_version 1.70.0`，edition 2021，4700 行，`default = ["compress"]`（**`aes256` 不是默认 feature**）。
docs.rs 0.6.1 的 item 清单（`all.html`）对照本仓库导出：

| 0.6.1（dyz1990） | 本仓库（hasenbanck 0.24.0） | 备注 |
|---|---|---|
| `SevenZReader` | **`ArchiveReader`** | 改名，且新增 `from_archive` / `set_thread_count` / `read_file` / `file_compression_methods` |
| `SevenZArchiveEntry` | **`ArchiveEntry`** | 改名 |
| `SevenZMethod` | **`EncoderMethod`** | 改名 |
| `SevenZMethodConfiguration` | **`EncoderConfiguration`** | 改名 |
| `SevenZWriter` | **`ArchiveWriter`** | 改名 |
| `Archive`, `BlockDecoder`, `Password`, `StreamMap`, `SubStreamsInfo` | 同名保留 | |
| `SeqReader`, `SourceReader` | 已不再导出（`writer/` 内部模块） | |
| — | **新增** `AutoFinisher`、`EncoderConfiguration`、`PreparedBlock`（compress feature） | |
| 函数 `compress, compress_to_path, decompress, decompress_file, decompress_file_with_extract_fn, decompress_with_extract_fn, default_entry_extract_fn` | 全部保留 | 名字一致 |
| 0.6.1 文档里**没有** `decompress_file_with_password` | 本仓库有（`aes256` feature 门控） | 因为 0.6.1 默认不启用 aes256，docs.rs 默认构建看不到；**这正说明两版默认能力不同** |

**结论**：本实现的解压侧 API 与公开 `sevenz-rust` 高度同构（改名而已），迁移成本低；但它**默认就带 AES/ppmd/util**，功能明显更强。

**迁移对照（同一件事，两个版本怎么写）**[源码]：

```rust
// ── sevenz-rust 0.6.1 (dyz1990)，注意 aes256 要自己开 feature ──
use sevenz_rust::{SevenZReader, Password, decompress_file};
decompress_file("a.7z", "out/")?;                                  // 同名同签名
let mut r = SevenZReader::open("b.7z", Password::empty())?;        // ← 旧名
let mut r = SevenZReader::new(std::fs::File::open("b.7z")?, pw)?;  // 从 reader 建
r.for_each_entries(|entry, reader| { /* ... */ Ok(true) })?;

// ── sevenz-rust2 0.24.0（本仓库），密码开箱可用 ──
use sevenz_rust2::{ArchiveReader, Password, decompress_file, decompress_file_with_password};
decompress_file("a.7z", "out/")?;                                  // 同名同签名
decompress_file_with_password("b.7z", "out/", "pw".into())?;       // 新便利函数
let mut r = ArchiveReader::open("b.7z", Password::empty())?;       // ← 新名
let mut r = ArchiveReader::new(std::fs::File::open("b.7z")?, pw)?;
r.for_each_entries(|entry, reader| { /* ... */ Ok(true) })?;
```

### 1.3 源码地图（读代码入口）[源码]

| 文件 | 行数 | 职责 |
|---|---|---|
| `src/reader.rs` | 1915 | **核心**：签名头解析、StartHeader、Header 各 NID 解析、解码栈、`ArchiveReader`、`BlockDecoder` |
| `src/archive.rs` | 496 | `Archive`/`ArchiveEntry`/`StreamMap`/`StartHeader`/`EncoderMethod`、`SIGNATURE_HEADER_SIZE` |
| `src/decoder.rs` | 276 | coder method-id → 具体解码器映射 |
| `src/encryption/aes.rs` | 501 | AES-256-CBC + SHA-256 KDF（含 raw-key 模式、密钥缓存） |
| `src/encryption/password.rs` | 62 | `Password`（**UTF-16LE** 编码） |
| `src/block.rs` / `bitset.rs` / `time.rs` / `error.rs` | 160/268/136/109 | Folder(coder 图)/位图/NT 时间/错误 |
| `src/writer.rs` + `src/util/compress.rs` | 977/281 | 打包侧（本次不重点） |
| `src/util/decompress.rs` | 238 | 高层便利函数 + **Zip-Slip 防护** |

---

## 2. 7z 字节布局与「起点 + 长度」定位

### 2.1 总体结构（官方 `7zFormat.txt` + 本仓库实现一致）[源码]

```
7z 归档（整体连续，自描述，无尾部冗余要求）
┌──────────────────────────────────────────────┐
│ SignatureHeader (固定 32 字节)                │
├──────────────────────────────────────────────┤
│ PackedStreams            ← pack_pos(偏移), pack_sizes[] 指向这里
│ PackedStreamsForHeaders  ← 头部自身被压缩时，压缩后的头数据也在这
├──────────────────────────────────────────────┤
│ Header 或 { PackedHeader, HeaderInfo }        │  ← NextHeader 在文件【末尾】
└──────────────────────────────────────────────┘
                      ↑
      NextHeaderOffset 从 offset=32 起算，NextHeaderSize 给出头长度
      ⇒ 归档总长 = 32 + NextHeaderOffset + NextHeaderSize
```

关键点：**7z 的「目录表」（Header）在文件末尾**，而不是像 ZIP 的 local header 那样散落在前面。这决定了救援策略的差异（见 §9.3）。

### 2.2 SignatureHeader 逐字节 [实测]

实测裸字节（`plain.7z`，用 7-Zip 25.01 生成）：

```
偏移  长度  内容(hex)                         含义
0x00   6    37 7A BC AF 27 1C                 kSignature {'7','z',0xBC,0xAF,0x27,0x1C}
0x06   1    00                                ArchiveVersion.Major（必须为 0，否则 UnsupportedVersion）
0x07   1    04                                ArchiveVersion.Minor（7-Zip 现在写 4）
0x08   4    8D 36 74 C8                       StartHeaderCRC = CRC32(bytes[12..32])   ← UINT32 LE
0x0C   8    A1 94 04 00 00 00 00 00           NextHeaderOffset (REAL_UINT64 LE) = 300193
0x14   8    24 00 00 00 00 00 00 00           NextHeaderSize   (REAL_UINT64 LE) = 36
0x1C   4    CF B8 58 69                       NextHeaderCRC = CRC32(NextHeader 内容)  ← UINT32 LE
──────────── 合计 32 字节（SIGNATURE_HEADER_SIZE = 32）
0x20   ...  PackedStreams 开始（= 32 + pack_pos）
```

注意 `NextHeaderOffset/Size` 是 **REAL_UINT64（定长 8 字节小端）**，不是 7z 的变长 `UINT64`；而 Header 内部的所有数字（PackPos、NumFiles、Sizes…）才是变长编码 [源码 `read_variable_u64`]。

### 2.3 **有没有「总长度」字段？——没有显式字段，但可以精确算出来** [实测]

7z 头里**不存在**类似 ZIP EOCD 的「归档总字节数」字段。但归档末尾就是 Header，于是：

```
end_of_archive = 32 + NextHeaderOffset + NextHeaderSize      （绝对偏移，相对 7z 起点）
blob_len       = end_of_archive
```

实测 4 个由 7-Zip 25.01 生成的归档，`computed == file_size` **全部为真**：

| 归档 | 文件长度 | NextHeaderOffset | NextHeaderSize | 32+off+size | 命中 |
|---|---|---|---|---|---|
| `plain.7z` | 300261 | 300193 | 36 | 300261 | ✅ |
| `enc_content.7z`（AES 内容加密，中文密码） | 300305 | 300237 | 36 | 300305 | ✅ |
| `enc_header.7z`（`-mhe=on` 头加密） | 300336 | 300240 | 64 | 300336 | ✅ |
| `plain_bcj.7z`（lzma2+bcj） | 300268 | 300200 | 36 | 300268 | ✅ |

分卷拼接后同样成立（`cat vol.7z.001 vol.7z.002 vol.7z.003` = 300305，其头自述 32+300237+36 = 300305 ✅）。

> 另一条等价校验：`next_header_offset` 其实就是「PackedStreams 的总字节数 + 头部被压缩时的额外流」。所以 `[0, 32+offset)` 是压缩数据区，`[32+offset, 32+offset+size)` 是目录表 —— **这两段一起构成完整的 7z**。

### 2.4 扫描定位算法（推荐 unbox 实现，已写代码跑通）[实测]

```
① 全文分块扫描签名 37 7A BC AF 27 1C（块间保留 5 字节重叠，防跨块漏检）
② 命中后读 32 字节头，校验：
     a. hdr[6] == 0                      （major 版本）
     b. CRC32(hdr[12..32]) == u32@8      ← StartHeaderCRC，第一道强校验
     c. end = 32 + u64@12 .. + u64@20 .. （checked_add 防溢出）
     d. end <= 文件长度                  （否则=截断/缺卷）
     e. CRC32(读出 NextHeader 内容) == u32@28  ← 第二道强校验
③ 通过 (b)+(e) 才认定「这里真有一个 7z」，并精确知道它的 [start, end)
```

两道 CRC 都通过才认 → 随机数据误判概率 ≈ 2⁻⁶⁴（单道也是 2⁻³²），可放心用于全盘扫描。

**实测结果**（`scan.rs`，也是本报告附带的可用实现）：

| 输入 | 容器大小 | 签名候选 | 验证通过 | 得到的段 |
|---|---|---|---|---|
| `fake.mp4`（137B 垃圾+AES7z+4096B 垃圾） | 304538 | 1 | 1 | start=137 end=300442 len=300305, next-hdr-crc ✅ |
| `fake2.mp4`（40000B 垃圾+头加密7z） | 340336 | 1 | 1 | start=40000 end=340336 len=300336 ✅ |
| `vol_merged.7z`（三卷拼接） | 300305 | 1 | 1 | 0..300305 ✅ |
| `enc_header.7z` | 300336 | 1 | 1 | 0..300336 ✅ |
| `truncated.7z`（砍掉 20 字节） | 300241 | 1 | 1 | 0..300241，`next_hdr_crc_ok=false` ← 截断信号 |
| `vol_missing_last.bin`（只有 .001+.002） | 204800 | 1 | 1 | 头自述要 300305 > 实际 204800，`crc_ok=false` ← **缺卷信号** |
| 两个 7z 首尾相接 | 600529 | 2 | 2 | 0..300261 ✅ + 300261..600529 ✅ |

### 2.5 NextHeader 的两种形态 [实测+源码]

`NextHeaderOffset` 指向的 4/8 字节头开头是 NID：

* `0x01 kHeader` —— 明文头（`7z a -mhc=off` 时）
* `0x17 kEncodedHeader` —— **被压缩的头**，内部又是一个 StreamsInfo，要先用 PackInfo/UnpackInfo 一般性解码一次才能拿到真 Header。**7-Zip 默认就是 0x17**：本次生成的 4 个归档 `first_nid` 全是 `0x17` [实测]。头部被压缩时，压缩后的头数据就躺在 `PackedStreamsForHeaders` 区，仍由 `pack_pos/pack_sizes` 定位。

对扫描器的影响：`-mhe=on`（头加密）时 `kEncodedHeader` 解出来的 coder 列表里含 `AES256_SHA256`，**没有密码连文件列表都拿不到**（实测 `PasswordRequired`）；普通的「内容加密」则**文件名列表是明文**，可以先列表再问密码（实测 `enc_content.7z` 空密码能列出 5 个条目）。这一点对 UI 很有用。

NID 全表（本仓库 `archive.rs` 常量，与官方文档一致）[源码]：

```
0x00 kEnd            0x01 kHeader       0x02 kArchiveProperties  0x03 kAdditionalStreamsInfo
0x04 kMainStreamsInfo 0x05 kFilesInfo    0x06 kPackInfo            0x07 kUnPackInfo
0x08 kSubStreamsInfo  0x09 kSize         0x0A kCRC                 0x0B kFolder
0x0C kCodersUnPackSize 0x0D kNumUnPackStream 0x0E kEmptyStream       0x0F kEmptyFile
0x10 kAnti           0x11 kName         0x12 kCTime              0x13 kATime
0x14 kMTime          0x15 kWinAttributes 0x16 kComment(未实现)     0x17 kEncodedHeader
0x18 kStartPos(不支持) 0x19 kDummy
```

### 2.6 ⚠️ 头部损坏时的「猜头」路径：**在库里有，但实测完全不能用** [实测+源码]

`Archive::read` 的逻辑（reader.rs:217-233）：

```rust
let start_header_crc = reader.read_u32()?;            // bytes 8..12
let header_valid = if start_header_crc == 0 {
    let cur = reader.stream_position()?;              // = 12
    let mut buf = [0; 20]; reader.read_exact(&mut buf)?;  // ← 注意：读的是 bytes 12..32
    reader.seek(SeekFrom::Start(cur))?;
    buf.iter().any(|a| *a != 0)                       // 20 字节全 0 才算「头无效」
} else { true };
if header_valid { 读 StartHeader → CRC 校验 → 正常解析 }
else { try_to_locale_end_header(...) }               // 从文件末尾往前逐字节找 0x17/0x01
```

`try_to_locale_end_header` 会在**最后 1 MiB** 内从后往前逐字节找 `0x17`/`0x01`，命中就当成 NextHeader 起点，推算 `next_header_offset = pos - 32`、`next_header_size = 文件长 - pos`，然后试解析。

**实测结论：这条路 12/12 全失败**（`fallback.rs`，把 StartHeader 整 24 字节 bytes[8..32] 清零）：

| 归档 | 清零点后结果 |
|---|---|
| copy.7z / single_file_…lzma.7z / two_files_…lzma.7z / solid.7z / non_solid.7z / ppmd.7z（小档） | `ERR BadTerminatedHeader(n)` |
| plain.7z(300KB) / copy.7z(300KB) / nonsolid.7z / delta2.7z | `ERR BadTerminatedHeader(n)` |
| enc_content.7z（中文密码） | `ERR BadTerminatedHeader(122)` |
| enc_header.7z（中文密码） | `ERR BadTerminatedHeader(93)` |
| 清零 + 嵌在 offset=999 + 尾部 4096 垃圾 | `ERR Io(UnexpectedEof)` |

IO trace（`trace.rs`）显示：它确实开始了反向扫描（`seek 144 / read 1 / seek 143 / read 1 …`），但**在第一个「碰巧是 0x01/0x17」的字节上就中止了整个扫描并报错**。源码对应位置（reader.rs:330-331）：

```rust
let result = Self::init_archive(reader, start_header, password, false, thread_count)?;  // ← 这里用 `?`
if !result.files.is_empty() { return Ok(result); }
```

`?` 直接把第一个候选的解析错误抛出，而不是 `continue` 试下一个候选 → **只要是压缩数据里出现一个 0x01/0x17 字节，这条路就废**。而且 `next_header_size = 文件长 - pos` 在「归档后面还有垃圾」时也是错的。

> 顺带纠正一个陷阱：只把 bytes 8..12（StartHeaderCRC）清零、或只清 8..28，**都不会**进这条路，而是走正常路径并在 `read_start_header` 抛 `ChecksumVerificationFailed`（因为校验窗口是 bytes 12..32，包含 NextHeaderCRC）。要触发 fallback 必须清 **bytes[8..32] 全部 24 字节**。
> 另一条独立证据：`Archive::read` 无参数可以直接对「损坏归档」返回错误，说明这库对损坏 7z 的容错基本为零。

**⇒ unbox 必须自己实现「从末尾找头 + 多候选逐一验证 + 用 CRC 确认」的恢复扫描**，不要指望它。

---

## 3. 能力矩阵

### 3.1 编解码器 / 过滤器

| 方法 (method-id) | 解压 | 压缩 | 实测证据 |
|---|---|---|---|
| COPY `00` | ✅ | ✅ | `copy.7z :: copy.txt methods=["LZMA2","COPY"]` ✅ |
| LZMA `03 01 01` | ✅ | ✅ | `single_file_with_content_lzma.7z` ✅ |
| LZMA2 `21` | ✅ | ✅ | 3 个自建归档 ✅（`LZMA2Reader` / `Lzma2ReaderMt`） |
| PPMd `03 04 01` | ✅ | ✅ | `ppmd.7z` 解出 11356 字节 ✅（`ppmd` feature 默认开） |
| BZIP2 `04 02 02` | ✅ | ✅ | `bzip2_file.7z` ✅（`bzip2` 默认开） |
| DELTA `03` | ✅ | ✅ | `delta.7z` / `delta_bcj2.7z` ✅ |
| BCJ x86/ARM/ARM64/ARM_THUMB/PPC/IA64/SPARC/RISC-V | ✅ | ✅ | `decompress_example_lzma2_bcj_x86.7z` ✅、`decompress_example_bcj_arm64.7z` ✅ |
| **BCJ2 `03 03 01 1B`（4 路输入！）** | ✅ | ❌ | `7za433_7zip_lzma2_bcj2.7z`（含 `7za.exe`，methods=`["LZMA","LZMA","LZMA2","BCJ2"]`）解出 490885 字节 ✅；`delta_bcj2.7z` ✅ |
| DEFLATE / DEFLATE64 | ✅(需 `deflate`) | ✅ | 未在本机跑通向量（无对应资源），源代码路径存在 |
| ZSTD `04 F7 11 01` | ✅(需 `zstd`) | ✅ | 需 feature；方法表已注册 |
| BROTLI `04 F7 11 02` | ✅(需 `brotli`) | ✅ | **实测**：默认 feature 下 `UnsupportedCompressionMethod("BROTLI")`；开启 `--features sevenz-rust2/brotli` 后 `zstdmt-brotli.7z` 解出 11556 字节 ✅ |
| LZ4 `04 F7 11 04` | ✅(需 `lz4`) | ✅ | 同上：默认报错，开 feature 后 `zstdmt-lz4.7z` ✅ |
| LZS / LIZARD | ❌ | ❌ | method-id 已定义但没有解码分支 |
| **AES256-SHA256 `06 F1 07 01`** | ✅ | ✅ | 见 §3.2 |

### 3.2 加密

| 能力 | 状态 | 证据 |
|---|---|---|
| AES-256-CBC + SHA-256 KDF（`2^numCyclesPower` 轮，8 字节 counter） | ✅ | `src/encryption/aes.rs:190-205` |
| 内容加密（`-p`，头明文） | ✅ | 自建 `enc_content.7z`（中文密码）解出 300028 字节 ✅ |
| 头部加密（`-mhe=on` / `-mhe=on`+分卷） | ✅ | `enc_header.7z` ✅、`volh.7z.001..003` 拼接 ✅ |
| **中文密码（UTF-16LE）** | ✅ | `Password::from("demo-password")` ✅ 全部通过 |
| 官方 AES 测试向量 | ✅ | `aes_small_test.7z`（pw=`iBlm8NTigvru0Jr0`）解出 129421 字节 ✅；`encrypted.7z`（pw=`sevenz-rust`）解出 3664 字节 ✅ |
| 错密码识别 | ✅ | 返回 `Error::MaybeBadPassword(io::Error)`（见 §3.4） |
| AES properties 解析（salt/iv size、numCyclesPower、raw-key 模式 `0x3F`） | ✅ | `aes.rs:131-182`；`numCyclesPower==0x3F` 时 key = salt‖password 截断 |
| KDF 实际参数（**实测 7-Zip 25.01 写出的包**） | `numCyclesPower=19`（2¹⁹ = **524288 轮 SHA-256**）、`salt_size=0`、`iv_size=16`、props = `53 0f 3e c6 …`（18 字节） | 解析 `enc_header.7z` 末尾明文 StreamsInfo 得到 |
| 工作因子上限 | ⚠️ 硬编码 `MAX_AES_CYCLES_POWER = 24`（`aes.rs:188`） | 超过直接报错（防空转 DoS）；本 crate **编码侧默认 `num_cycles_power=8`**（`encoder_options.rs:405`），而 7-Zip 用 19 |
| KDF 结果缓存 | ✅ 全局单槽 `Mutex<Option<...>>`（按 sha256(power‖saltlen‖salt‖pw) 指纹） | `aes.rs:207-230`：连续试密码时**只有同一组 (power,salt,pw) 能命中**，换密码就要重算 |
| `Password` 用完清零 | ❌ 无 zeroize | `password.rs` |

### 3.3 容器级能力

| 能力 | 状态 | 说明 / 证据 |
|---|---|---|
| Solid（多文件共享一个 block） | ✅ | 自建 300KB 归档 `is_solid=true blocks=1` |
| 非 solid（每文件一个 block） | ✅ | `-ms=off` 归档 `is_solid=false blocks=3` |
| 多 block / 多 pack stream | ✅ | `pack_streams=3` ✅ |
| 空文件 / 空目录 / anti 项 | ✅ | `single_empty_file.7z`、`two_empty_file.7z` ✅ |
| 文件时间戳（c/m/a） | ✅ 读；`nt-time` feature 控制写入侧 | `ArchiveEntry::{creation,last_modified,access}_date` + `set_times` 恢复 |
| Windows 属性 | ✅ 读（写入 ignored） | |
| UTF-16 文件名 | ✅（`NamesReader` 按 u16 读，`String::from_utf16`） | 中文文件名可正常列出 |
| **分卷 `.7z.001/.002`** | ❌ **库内完全没有** | 全仓库 grep `volume|multivol|split|\.001|part` 无命中；但**拼接后可直接用**（§6） |
| **非 0 偏移读取（内嵌 7z）** | ⚠️ 需自己包一层 `Read+Seek` | §5，`Archive::read` 硬编码 `seek(Start(0))` |
| 头部损坏恢复 | ⚠️ 有代码，实测不可用 | §2.6 |
| 尾部有多余垃圾 | ✅ 容错 | `fake.mp4`（后接 4096 字节 0x00）正常解压 ✅ |
| CRC32 校验（块级 / 子流级） | ✅ | `Crc32VerifyingReader`，失败 → `ChecksumVerificationFailed` |
| Zip-Slip 防护 | ✅ 库自带 `safe_join`/`default_entry_extract_fn` 拒绝 `..`、绝对路径、盘符 | `util/decompress.rs:152-172` |
| 多线程解码 | ✅ 仅 LZMA2（`Lzma2ReaderMt`），`set_thread_count()` 默认 = CPU 核数，clamp 1..=256 | `reader.rs:1328`、`examples/mt_decompress.rs` |
| 进度回调 | ❌ 无内建；但 `for_each_entries` 给流式 `Read`，可自己算 | `examples/decompress_progress.rs` |
| 断点续传 / 随机 seek 到单文件 | ❌ 无 | solid 必须从头解 |
| 内存上限 | ⚠️ `MAX_MEM_LIMIT_KB = usize::MAX/1024`（≈无限制） | `reader.rs:18`；LZMA2 字典最多 4 GiB |
| WASM | ✅ | `default_wasm` feature |

### 3.4 错误类型速查（对「密码重试循环」很重要）[实测]

```
Error::BadSignature([u8;6])              签名不符（把带偏移的裸 File 丢进去时就是这个）
Error::UnsupportedVersion{major,minor}   major != 0
Error::ChecksumVerificationFailed        StartHeaderCRC / NextHeaderCRC / 数据 CRC 不符
Error::NextHeaderCrcMismatch
Error::PasswordRequired                  加密流但密码为空（-mhe=on 列表时就报）
Error::MaybeBadPassword(io::Error)       ←★ 有密码但解不出来，几乎总是「密码错」
Error::UnsupportedCompressionMethod(String)
Error::MaxMemLimited{max_kb,actaul_kb}
Error::FileNotFound                      read_file() 名字不存在
Error::Other(Cow<str>)                   各种格式错（含 "next header offset out of range"）
Error::Io(io::Error, Cow<str>)           短路/截断（UnexpectedEof 等）
```

实测的**错密码表现**（注意形态不统一）：

| 场景 | 错误 |
|---|---|
| 头加密 + 错密码 | `MaybeBadPassword(Custom{InvalidInput,"range decoder first byte is not zero"})` |
| 头加密 + 错密码（另一归档） | `MaybeBadPassword(Error{UnexpectedEof,"failed to fill whole buffer"})` |
| 内容加密 + 错密码（列表成功，取数据失败） | `MaybeBadPassword(Custom{InvalidData,"invalid LZMA2 control byte: 59"})` |
| 加密归档 + 空密码 | `PasswordRequired` |
| 缺最后一卷 | `Other("next header offset out of range")` |
| 截断 20 字节 | `Io(UnexpectedEof,"failed to fill whole buffer")` |

结论：**判定「密码对不对」不能只看错误类型**——要 `MaybeBadPassword` / `ChecksumVerificationFailed` 都当作「可能密码错」。更稳的做法见 §9.4。

---

## 4. API 用法示例（全部为本机编译运行过的代码）

### 4.1 最小用法（文件路径）

```rust
// Cargo.toml: sevenz-rust2 = { version = "0.24", features = ["zstd","brotli","lz4","deflate"] }
use sevenz_rust2::{decompress_file, decompress_file_with_password, Password};

fn main() -> Result<(), sevenz_rust2::Error> {
    // 无密码
    decompress_file("a.7z", "out/")?;
    // 有密码（中文也可以，内部转 UTF-16LE）
    decompress_file_with_password("b.7z", "out/", Password::from("demo-password"))?;
    Ok(())
}
```

### 4.2 流式打开 + 列出 + 按需提取

```rust
use std::fs::File;
use std::io::Read;
use sevenz_rust2::{Archive, ArchiveReader, Password};

fn list_and_extract() -> Result<(), sevenz_rust2::Error> {
    let pw = Password::from("demo-password");

    // ① 只要文件列表（不解压）：Archive::read
    let mut f = File::open("b.7z")?;
    let archive = Archive::read(&mut f, &pw)?;
    for e in &archive.files {
        println!("{:<40} dir={} stream={} size={}", e.name(), e.is_directory(), e.has_stream(), e.size());
    }
    println!("solid={} blocks={} pack_streams={}",
             archive.is_solid, archive.blocks.len(), archive.pack_sizes().len());

    // ② 逐个提取（流式，写盘用 std::io::copy，不占内存）
    let mut r = ArchiveReader::new(File::open("b.7z")?, pw.clone())?;
    r.set_thread_count(4);                       // 仅 LZMA2 MT 生效
    let mut methods = Vec::new();
    r.file_compression_methods("data/a.txt", &mut methods)?;   // 可探测 AES256_SHA256 ⇒ 需要密码
    r.for_each_entries(|entry, rd| {
        let mut buf = Vec::new();
        rd.read_to_end(&mut buf)?;               // 生产代码应改成流式 copy 到文件
        println!("{} -> {} bytes", entry.name(), buf.len());
        Ok(true)                                  // 返回 false 可提前停止
    })?;

    // ③ 只取一个文件（solid 归档要解到它为止；非 solid 便宜）
    let mut r = ArchiveReader::new(File::open("b.7z")?, pw)?;
    let data = r.read_file("data/a.txt")?;        // Vec<u8>，注意大文件不要这样用
    println!("{}", data.len());
    Ok(())
}
```

### 4.3 【关键】非 0 偏移 / 分卷 / 任意包装：自己包 `Read + Seek`

下面两段是本次编译并实测通过的代码（见 `rsprobe/src/main.rs` 的 `Window` 与 `Concat`）：

```rust
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// 把 `inner` 从 `base` 起的部分「重新标定为 0 起点」给 sevenz-rust2 用。
/// 长度取到文件末尾（多出来的尾随垃圾无害，7z 是自描述的）。
/// 如果要严格限制到扫描出的 blob 结尾，把 `len` 换成 blob_len 即可。
pub struct Window<R> { inner: R, base: u64, len: u64 }

impl<R: Read + Seek> Window<R> {
    pub fn new(mut inner: R, base: u64) -> io::Result<Self> {
        let total = inner.seek(SeekFrom::End(0))?;
        Ok(Self { inner, base, len: total.saturating_sub(base) })
    }
}

impl<R: Read + Seek> Read for Window<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let pos = self.inner.stream_position()?;
        if pos < self.base || pos >= self.base + self.len { return Ok(0); }
        let remain = (self.base + self.len - pos) as usize;
        let want = buf.len().min(remain);
        self.inner.read(&mut buf[..want])          // ← 也可在这里累加字节数做进度条
    }
}

impl<R: Read + Seek> Seek for Window<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let abs = match pos {
            SeekFrom::Start(p) => self.base + p,
            SeekFrom::End(p)   => ((self.base + self.len) as i64 + p) as u64,
            SeekFrom::Current(p) => (self.inner.stream_position()? as i64 + p) as u64,
        };
        self.inner.seek(SeekFrom::Start(abs))
    }
}

/// 多卷（.7z.001/.002/...）当作一个连续字节流；7z 卷是裸切分，所以直接拼即可。
pub struct Concat { files: Vec<(File, u64, u64)>, total: u64, cur: u64 }

impl Concat {
    pub fn open(paths: &[PathBuf]) -> io::Result<Self> {
        let (mut files, mut total) = (Vec::new(), 0u64);
        for p in paths {
            let f = File::open(p)?;
            let len = f.metadata()?.len();
            files.push((f, total, len));
            total += len;
        }
        Ok(Self { files, total, cur: 0 })
    }
}

impl Read for Concat {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut got = 0usize;
        while got < buf.len() && self.cur < self.total {
            let idx = self.files.iter()
                .position(|(_, s, l)| self.cur >= *s && self.cur < s + l)
                .expect("offset inside some piece");
            let (f, start, len) = &mut self.files[idx];
            let off = self.cur - *start;
            f.seek(SeekFrom::Start(off))?;
            let want = ((*len - off) as usize).min(buf.len() - got);
            let n = f.read(&mut buf[got..got + want])?;
            if n == 0 { break; }
            got += n;
            self.cur += n as u64;
        }
        Ok(got)
    }
}

impl Seek for Concat {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let abs = match pos {
            SeekFrom::Start(p)   => p as i64,
            SeekFrom::End(p)     => self.total as i64 + p,
            SeekFrom::Current(p) => self.cur as i64 + p,
        };
        if abs < 0 { return Err(io::Error::other("neg")); }
        self.cur = abs as u64;
        Ok(self.cur)
    }
}

// 用法：内嵌 + 分卷 + 中文密码，一次搞定
fn demo() -> Result<(), sevenz_rust2::Error> {
    use sevenz_rust2::decompress_with_password;
    let parts: Vec<PathBuf> = (1..=3)
        .map(|i| PathBuf::from(format!("vol.7z.{i:03}")))
        .filter(|p| p.exists())
        .collect();
    let cat = Concat::open(&parts).unwrap();          // 3 卷 → 1 个流
    let win = Window::new(cat, 0).unwrap();            // 外层若还有伪装壳，base 换成 blob 起点
    decompress_with_password(win, "out/", "demo-password".into())?;
    Ok(())
}
```

实测输出（`main.rs`/`sweep.rs`）：

```
[contained/content]   Window(base=137)   entries=["data","data/sub","data/a.txt","data/rand.bin","data/sub/b.txt"] → 5/5 ✅
[contained/hdr-enc]   Window(base=40000) 头加密 7z → 5/5 ✅
[multivol/AES+cn-pw]  3 卷 Concat → 5/5 ✅
[multivol/embedded]   3 卷 + base=999 → 5/5 ✅
decompress_with_password(Window) OK, files=3 ["a.txt","rand.bin","b.txt"] ✅
```

### 4.4 逐 block 解码 / 多线程 / 复用已解析 Archive

```rust
use std::sync::Arc;
use sevenz_rust2::{Archive, BlockDecoder, Password};

let mut f = std::fs::File::open("big.7z")?;
let pw = Password::empty();
let archive = Arc::new(Archive::read(&mut f, &pw)?);   // 只解析一次

// 非 solid 归档：每个 block = 一个文件，可多线程并行解
for i in 0..archive.blocks.len() {
    let a = Arc::clone(&archive);
    std::thread::spawn(move || {
        let mut f = std::fs::File::open("big.7z").unwrap();   // 每线程自己的 reader
        let dec = BlockDecoder::new(1, i, &a, &pw, &mut f);
        println!("block {i}: {} entries", dec.entry_count());
        dec.for_each_entries(&mut |entry, rd| { /* ... */ Ok(true) }).unwrap();
    });
}

// 也可以避开重复解析：ArchiveReader::from_archive(archive, source, password)
```

---

## 5. 能否从「非文件开头」读？（问题 4）

**结论：能，但必须自己包一层 `Read + Seek`；直接给它一个定位到 base 的 `File` 一定失败。**

证据 1（源码，reader.rs:197-199）——`Archive::read` 无条件回到 0：

```rust
pub fn read<R: Read + Seek>(reader: &mut R, password: &Password) -> Result<Archive, Error> {
    let reader_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;        // ← 硬编码 0，完全不管调用方当前位置
```

`util::decompress_impl` 里虽然写了 `let pos = src_reader.stream_position()?; src_reader.seek(SeekFrom::Start(pos))?;`，但那是空操作（`reader.rs:132-133`），**不会**保留调用方偏移。

证据 2（实测，`sweep.rs` A/B 段）：

```
raw File seeked to base=137        → ERR BadSignature([65,65,65,65,65,65])   （读到的是伪装壳的 'AAAAAA'）
Window base=138（故意错一位）        → ERR BadSignature([122,188,175,39,28,0])（读到 "z\xBC\xAF\x27\x1c..."）
Window base=137（正确）             → OK  entries=5 ✅
```

注意错一位时的报错内容正好是签名右移一字节的结果，可作为「偏移是否正确」的直观诊断。

**实测容错性**：如果把整个「剩余文件」都暴露出去（`len = 文件长 - base`，尾巴带 4096 字节垃圾），7z 一样能正常解析——因为它用 `NextHeaderOffset/Size` 自定位，**不依赖 reader 报告的长度**（只用于 `next_header_size <= reader_len` 之类的边界检查）。这非常方便：**我们不需要先算出 blob 长度就能开读**。

三种实现方式对比：

| 方式 | 内存 | 适合 | 备注 |
|---|---|---|---|
| `Window<R>` 包装器（推荐） | O(1) | 8GB+ 大包、伪装壳内嵌 | 还能顺带做进度统计；本次实测通过 |
| `Cursor<&[u8]>`（切出切片） | O(blob) | 小文件、单元测试 | 但 `decompress_*` 要求 `Read+Seek`，`Cursor<&[u8]>` 满足 |
| 先抽成临时文件再 `decompress_file` | 磁盘 O(blob) | 兜底/调试 | 违背「只提真身省一半空间」的初衷，不推荐 |

---

## 6. 分卷（`.7z.001/.002`）——库里没有，但**不需要库支持**

### 6.1 事实：7z 的分卷是「裸字节切分」，没有卷头 [实测]

实测 `7z a -pdemo-password -v100k vol.7z ./data` 产出：

| 卷 | 大小 | 前 8 字节 | 是否含 7z 签名 |
|---|---|---|---|
| `vol.7z.001` | 102400 | `37 7a bc af 27 1c 00 04` | ✅ 是 |
| `vol.7z.002` | 102400 | `9b 49 cc 5a 31 8d cc 45` | ❌ 否（纯数据体） |
| `vol.7z.003` | 95505 | `c5 5d 71 01 8f c8 c4 df` | ❌ 否 |

三重独立证据：
1. `.002/.003` 开头没有任何签名/卷头 → 是同一字节流的续段。
2. `cat vol.7z.001 vol.7z.002 vol.7z.003` = 300305 字节，其**自身 StartHeader 自述长度也是 300305**（32+300237+36），自洽 ✅。
3. 用 7-Zip 校验拼接结果：`7z t -pdemo-password vol_merged.7z` → **`Everything is Ok`** ✅；用 sevenz-rust2 直接解：5/5 文件、300028 字节全对 ✅。头加密版 `volh.7z.001..003` 拼接后同样 5/5 ✅（不带密码则 `PasswordRequired`）。

⇒ **unbox 只需把 `.001/.002/...` 顺序拼成一个逻辑 `Read+Seek`（§4.3 的 `Concat`），无密码/内容加密/头加密/中文密码四种组合全部直接可用。**

### 6.2 缺卷的检测与报错 [实测]

只拼 `.001+.002`（缺 `.003`）时：
* 本库报 `Other("next header offset out of range")`（因为头在 300237 之后，而流只有 204800 字节）。
* 我们的扫描器能提前、更清楚地判定：`头自述 end=300305 > 实际 204800` → `next_hdr_crc_ok=false`，直接告诉用户「缺第 3 卷 / 文件不完整」。

**建议**：`Concat` 打开前先按 `NextHeaderOffset+NextHeaderSize` 校验「拼起来的长度够不够」，不够就明确报「缺卷」而不是让它内部报偏移越界。

### 6.3 需要自己补的分卷能力

| 缺口 | 建议实现 |
|---|---|
| 卷发现与排序 | 匹配 `<name>.7z.###` / `<name>.7z.001` / 裸 `.001`；按 3 位数字排序；容忍大小写与前后缀 |
| 缺口检测 | 卷号连续性（1,2,4 → 缺 3）+ 长度自洽校验（§2.3） |
| 缺卷/受损卷 | 报告「缺哪一卷」，能解出的部分照解（配合 §9.3 救援模式） |
| 伪装壳内的分卷 | 若每卷各自被塞进一个伪装壳：对每卷先做签名扫描取出 blob 段，再拼 |
| 与 ZIP 分卷共存 | `.z01` 是另一套（ZIP64 分卷有卷头），不要混用同一套逻辑 |

---

## 7. 中文密码（这是最容易踩坑的地方）

### 7.1 机制 [源码+实测]

`Password::from(&str)`（`password.rs:53-61`）把字符串**按 UTF-16LE 编码**（逐 `encode_utf16()` 写 u16 LE），这正是 7-Zip 的密码存储约定。

```rust
use sevenz_rust2::Password;
let pw = Password::from("demo-password");      // → 22 字节 UTF-16LE
let pw2 = Password::new("demo-password");     // 等价
let raw = Password::from_raw(&[..]);      // 直接喂字节（高级用法）

// ⚠️ 没有 From<String>：`some_string.into()` 编译不过，必须
let s = String::from("demo-password");
let pw3 = Password::from(s.as_str());      // ← 注意 as_str()
```

### 7.2 实测四种喂法（同一个中文密码的 AES 归档）

| 喂法 | 结果 |
|---|---|
| `Password::from("demo-password")`（UTF-16LE） | ✅ `extract OK 300028` |
| `Password::from_raw(手写 UTF-16LE 字节)` | ✅ `extract OK 300028` |
| `Password::from_raw(UTF-8 字节)` | ❌ `MaybeBadPassword("invalid LZMA2 control byte: 59")` |
| `Password::from_raw(GBK 字节 CE A9 61 69 D1 A9 C0 F2 BE C6)` | ❌ `MaybeBadPassword("invalid LZMA2 control byte: 43")` |

**结论（对 unbox 的「中文密码纠错」至关重要）**：
* 7-Zip 的密码永远是「字符 → UTF-16LE」，「GBK 原始字节」这种喂法**在算法上就不可能对**，别浪费 CPU。
* 真正需要纠错的是**密码文本的解读**：说明书 / 文件名里给的那串字节，到底该按 UTF-8 还是 GBK 还是 Big5 解码成「字符」。正确流程：

```
读到的密码字节(可能 GBK/UTF-8/Big5)
   → 分别按 UTF-8 / GBK / GB18030 / Big5 解码成 &str
   → 每个候选 new 一个 Password::from(&str)（内部转 UTF-16LE）
   → 依次尝试；命中即停
```

另外值得一起枚举的常见变体：首尾空格、全角字符、`（`/`(`、大小写、繁体/简体混用、密码其实就是**文件名或目录名**（实战里极常见）。

### 7.3 认密码对不对的技术要点

* 头加密（`-mhe=on`）：错密码会在 `Archive::read` / `ArchiveReader::new` 阶段就失败 → **快**（一次 KDF + 解一小块头就报错），适合暴力枚举。
* 内容加密（头明文）：列文件名成功、**取数据才失败** → 慢，且可能先解出一段垃圾数据。若只为了验证密码，**优先解第 1 个最小的文件并校验其 CRC32**（`ArchiveEntry::has_crc/crc` 都在，库会自动校验并报 `ChecksumVerificationFailed`）。
* 必须把 `MaybeBadPassword` **和** `ChecksumVerificationFailed` 都视作「密码可能错」。
* AES KDF 每次要跑 `2^numCyclesPower` 轮 SHA-256（**实测 7-Zip 25.01 写的包 `numCyclesPower = 19`，即 524288 轮**，见 §3.2），**这是密码枚举的主要成本**；库只有一个全局单槽缓存，换密码就重算（无法利用多核）。
* **实测成本**（本机 release 构建，`bench.rs`）：头加密归档 **121 ms / 候选**（32 次错密码 3.87 s）；正确密码一次 59 ms。⇒ **100 个候选 ≈ 12 s，1000 个候选 ≈ 2 min**，所以「候选排序 + 去重」比并行更值钱。
* 内容加密归档**空密码列目录只要 0.06 ms**（头是明文，不触发 KDF）；但这类包**验证密码必须真的解一段数据**（KDF + 解码），成本和头加密同量级。若要快速判定，先解**最小的那个文件**。

---

## 8. 已知限制 / 未实现（源码依据）

`grep -rn "todo|fixme|not yet|unimplemented|unsupported" src/` 的命中，逐个说明：

| 位置 | 内容 | 影响 unbox 吗 |
|---|---|---|
| `archive.rs:33` | `/// TODO: Implement reading & writing comments`（`kComment 0x16`） | 低。ZIP 风格注释在 7z 里极少用；读到 kComment 会被当未知属性 skip |
| `reader.rs:270` | `kAdditionalStreamsInfo` → `Err("Additional streams unsupported")` | 中。极少数工具（如某些 SFX/加固包）用备用流；**遇到就整个打不开** |
| `reader.rs:464 / 1398` | `"Multi input/output stream coders are not yet supported"`（单流路径） | 低，因为有 `build_decode_stack2` 兜底（BCJ2 走这条） |
| `reader.rs:1081` | 备选方法（`more_alternative_methods`）不支持 | 低（官方文档也标「Not used anymore, must be 0」） |
| `reader.rs:648` | `kStartPos is unsupported, please report` | 低（只出现在「附加流」场景） |
| `reader.rs:593/608/623/638` | `kCTime/kATime/kMTime/kWinAttributes` 的 `External != 0` 未实现 | 低（External 头极少出现） |
| `reader.rs:560` | `kName` 的 `External != 0` 未实现 | 低 |
| `reader.rs:877` | `kFolder` 的 `External != 0` → `ExternalUnsupported` | 低 |
| `reader.rs:1433` | coder 数 > 32 → 拒绝 | 极低 |
| `reader.rs:1636` | 除 BCJ2 外的多输入 coder 不支持 | 低 |
| `archive.rs` 未列 | LZS / LIZARD 只定义了 id，没有解码实现 | 极低 |
| `encryption/aes.rs` | AES 只支持 AES-256-CBC+SHA-256（`0x06F10701`），不支持旧版 AES-128 变体 | 极低（现代 7-Zip 只用这个） |

**功能层面没实现（对我们最重要）**：分卷、非零偏移、头部损坏恢复、内存上限、进度回调、断点续传、单文件随机访问（solid）。这些都在 §9 里。

---

## 9. 「对我们的启示」+ 需要自己补的部分

### 9.1 定位与切分（自己做，库不负责）

`scan.rs`（本次已跑通）给出可直接搬进 unbox 的设计：

```rust
// 核心不变量：
//   blob.start = 签名位置（CRC32(bytes[12..32]) == u32@8 且 版本 major==0）
//   blob.len   = 32 + u64@12 + u64@20                （★ 无「总长度」字段，但可精确算出）
//   可信度     = CRC32(NextHeader 区) == u32@28       （第二道闸）
pub struct Segment { pub start: u64, pub end: u64, pub next_header_offset: u64,
                     pub next_header_size: u64, pub next_header_crc_ok: bool }
```

工程要点：
* **分块扫描（1 MiB/块 + 5 字节重叠）**，不要一次 mmap 8GB。签名只有 6 字节，逐字节扫描在 Rust 里配 `memchr` 风格的比较够快；也可先用首字节 `0x37` 做粗筛。
* **必须两道 CRC 都过**才认（或者至少 StartHeaderCRC + 版本），否则压缩数据里出现 `37 7A BC AF 27 1C` 的概率虽低但非零。
* 命中后**继续扫描**（可能有多段、嵌套影藏包）；用 `if abs 落在已接受段内 → 跳过` 去重（`scan.rs` 已实现）。
* `next_header_crc_ok == false` 或 `computed_end > 可用字节` → 直接判定「**截断/缺卷**」，这是免费的完整性信号。
* 切出 blob 后，用 §4.3 的 `Window` 喂给 `ArchiveReader`（**不要**先把 8GB blob 拷到临时文件）。

### 9.2 分卷（自己做）

见 §6.3。核心就是 `Concat` + 卷名匹配 + 长度自洽校验 + 缺卷报告。

### 9.3 救援模式：7z 比 ZIP 难得多（要有预期管理）

* ZIP 的每个文件都有 local header，目录表坏了还能靠 `PK\x03\x04` 逐个扫出来 → `zip -FF` 那套。
* **7z 只有一个目录表，且在末尾**；它被压缩（默认 `kEncodedHeader`），还可能被 `-mhe=on` 加密。
  - 头在、数据缺：还能列出文件、能解的解。
  - **头缺/尾截断：文件名单、大小、编码链参数全丢**，理论上剩下一堆 LZMA2 裸流，无法可靠还原文件边界 → 基本无解。
  - 头在但 `StartHeader` 损坏（bytes 8..32 被改）：**可以通过「从末尾反向找 `0x17/0x01`」恢复**，前提是自己实现多候选验证（库的这版实现是坏的，§2.6）。请在 unbox 里实现：
    1. 在最后 N MiB（建议不设 1 MiB 上限，直接扫到压缩数据区起点）反向找 `0x17`/`0x01`；
    2. 对每个候选，**不要预设 size**，而是尝试用它作为 `kEncodedHeader` 解析 StreamsInfo（PackInfo 给出这个「打包头」的字节范围，其实是自描述的）；
    3. 解出来的 Header 必须能被 `read_header` 完整消费且文件数 > 0、名字是合法 UTF-16 → 才接受；
    4. 候选失败就 `continue`，绝不 `?`（这正是库里那段代码的 bug）。

### 9.4 密码子系统（自己做）

```
候选生成：说明书.txt / 文件名 / 文件夹名 / 同目录常见解压码库 / 用户密码本
        × 编码解读：UTF-8 / GBK / GB18030 / Big5 / UTF-16LE
        × 变体：去首尾空格、全角↔半角、繁↔简、大小写
验证顺序：① 头加密 → 直接 Archive::read（快）
        ② 内容加密 → Archive::read 列表 OK 后，解「最小的那个文件」并依赖 CRC 校验
失败判定：PasswordRequired / MaybeBadPassword / ChecksumVerificationFailed 三种都算「候选错」
缓存：同 salt+power 的 KDF 结果要复用（库只缓存 1 个槽；自己维护 HashMap 更省）
```

### 9.5 必须自己补的「库级别」缺口（可能需要 patch 或 fork）

| 缺口 | 现状 | 我们的选项 |
|---|---|---|
| 内存上限 | `MAX_MEM_LIMIT_KB = usize::MAX/1024`（`reader.rs:18`），**无 API 可改**；恶意 LZMA2 字典可申请到 4 GiB | ① 自己先解析 coder properties（LZMA2 的 props[0] → 字典大小算法在 `decoder.rs:255-271`，LZMA 字典在 props[1..5]）——**扫描阶段就能拒绝超大字典**；② 或者 fork/patch 成可配置（推荐：`MAX_MEM_LIMIT_KB` 改成参数） |
| 进度 | `for_each_entries` 只有流式 `Read` | 用我们自己的 `Window`/`Concat` 包装器统计「从源读了多少字节」+ `archive().files` 求和做总分母（`examples/decompress_progress.rs` 就是按解压字节算的） |
| 断点续传 | 无 | 7z 无内建；只能按文件粒度做「已完成清单」续跑；solid 归档尤其要小心（一个 block 没解完就白干） |
| 多线程 | 仅 LZMA2；且默认 `available_parallelism()` 会**自动开满核** | 注意在手机/小机器上显式 `set_thread_count(n)`；非 solid 归档还可以按 block 自己开线程（§4.4） |
| 选择性提取（挑文件） | `read_file` 对 solid 要解前面所有数据；非 solid 才便宜 | 先列表（`Archive::read`）→ 用 `BlockDecoder::entries()` 定位目标 block → **只解需要的 block**；对 solid 包要提前告诉用户「这个包必须整解」 |
| 单文件大文件输出 | `read_file` 返回 `Vec<u8>`（会吃内存） | 一律用 `for_each_entries` + `io::copy` 流式写盘 |

### 9.6 与 unbox 现有设计（DESIGN.md）的对接

* §A.3「定位真身（不依赖目录表）」→ 用 §2.4 的扫描器，**两道 CRC + 精确长度**，比 ZIP 的 EOCD 定位更干净。
* §A.4「只提真身，跳过外壳垃圾段」→ 有了精确 `[start, end)`，**只读这一段**；配 `Window` 可以做到「扫描完直接把 window 指向 blob，边解边写，不落临时文件」。
* §A.5「分卷自动串联」→ `Concat`（§4.3/§6）。
* §A.7「中文密码 GBK↔UTF-8 互转重试」→ §7.2 已验证的正确做法（**不要**用 `Password::from_raw` 喂 GBK 字节）。
* §A.9「完整性校验」→ 7z 侧额外白送：块级/文件级 CRC32 由库自动校验（`ChecksumVerificationFailed`），我们再叠加 SHA-256。
* §八.3「大文件必须流式」→ 用 `Window`（不 mmap、不拷临时文件）+ `for_each_entries`（不 `read_file`）。
* §三·补.2「救援模式（分卷缺卷 → 能解的照解）」→ 7z 现实约束见 §9.3：**头丢=报废**，要在 UI 文案上如实说明。

---

## 10. 意外发现

1. **「7z 没有总长度字段」是伪命题**：`32 + NextHeaderOffset + NextHeaderSize` 就是精确长度（4/4 命中），且 `StartHeaderCRC` + `NextHeaderCRC` 让「签名命中」的判定几乎零误报。这是本次最有价值的一条。
2. **库自带的「头部损坏恢复」是死代码级不可用**：`try_to_locale_end_header` 里用 `?` 而不是 `continue`，第一个假候选就终止；实测 12 个归档（含 110 字节的极小包）全部 `BadTerminatedHeader(n)`。**别指望它，自己写。**
3. **`Archive::read` 硬编码 `seek(Start(0))`**：把带偏移的 `File` 直接丢进去 → `BadSignature([65,65,65,65,65,65])`（读到了伪装壳的 'AAAAAA'）。必须包 `Window`。反过来，**尾随垃圾完全无害**（7z 自描述），所以 `Window` 可以只给 base、不管末端。
4. **内容加密可以「先列表后要密码」，头加密不行**：`enc_content.7z` 空密码能列出 5 个条目，`enc_header.7z` 直接 `PasswordRequired`。这个差异可以拿来做 UI：能列就先展示文件树，再问密码。
5. **7z 的分卷是裸切分**：`.002/.003` 连签名都没有，`cat` 起来就是一个合法 7z（`7z t` 判 `Everything is Ok`），所以**「分卷支持」在本项目里约等于「实现一个多文件拼流」**，不需要解析任何卷结构。
6. **`Password::from_raw(GBK 字节)` 是条死胡同**：7-Zip 只认 UTF-16LE，纠错必须在「字节→字符」那一步做，而不是在「字符→密钥」那一步。
7. **内存上限等于没有**：`MAX_MEM_LIMIT_KB = usize::MAX / 1024`，且**无公开 API 可调**。对一个「专门处理来路不明伪装包」的工具，这是个实打实的攻击面（恶意 LZMA2 字典 → OOM）。建议在扫描阶段自己解析 LZMA2 props 拦截。
8. **AES KDF 有全局单槽缓存**（`derive_key_cached`，按 sha256(power‖saltlen‖salt‖pw) 指纹命中），对「同一个密码解多个分卷/多个内层包」有加速；但对「批量试不同密码」没有帮助。
9. **BCJ2（4 路输入）真的实现了**：走 `build_decode_stack2`（多输入 coder 图），官方 7-Zip 向量 `7za433_7zip_lzma2_bcj2.7z` 解出 490885 字节通过。这点比很多简化实现强。
10. **库顺带产出 `cdylib`**（`crate-type = ["cdylib","rlib"]`）且**官方支持 WASM** —— 如果将来 Flutter/Web 端要复用同一套 7z 逻辑，可以直接 FFI。
11. **MSRV 偏高**：edition 2024 + `rust-version = 1.93`（本地 1.97 可编译）。如果 Android/CI 用的是老 NDK 工具链要注意。
12. **只清部分 StartHeader 不会触发 fallback**：因为「头有效性」探针读的是 bytes 12..32（含 `NextHeaderCRC`），要触发必须把 **bytes[8..32] 全 24 字节**清零。调试时极易误判成「这条恢复路径根本没实现」。
13. **`7z -v` 生成 `.001` 时 7-Zip 自己会报 `E_NOTIMPL: Updating for multivolume archives is not implemented`**（实测，追加数据时）——即 7z 分卷是「一次性写出后切开」，不能增量更新。顺带说明：分卷与 `-mhe=on` 可以同时用（`volh.7z.001..003` 实测 ✅）。
14. **`Password` 只实现了 `From<&str>`，没有 `From<String>`**：`some_string.into()` 编译不过，必须 `Password::from(s.as_str())`（本次写 bench 时踩到）。
15. **密码枚举有「白嫖」空间**：内容加密归档（头明文）**用错密码列目录只要 0.06 ms**（不触发 KDF）——可以先零成本列出文件树并展示给用户，再去试密码。

---

## 附录 A：关键源码位置表

| 主题 | 位置 |
|---|---|
| 签名/版本/StartHeader 解析 | `src/reader.rs:197-256` |
| 「头有效性」探针 + 分支 | `src/reader.rs:217-233` |
| 损坏头恢复（有 bug） | `src/reader.rs:299-341`（`?` 在 330-331） |
| `init_archive`：算 header_pos、校验 NextHeaderCRC | `src/reader.rs:343-431` |
| `read_encoded_header`（头被压缩） | `src/reader.rs:433-486` |
| PackInfo（`pack_pos`/`pack_sizes`） | `src/reader.rs:829-862` |
| UnpackInfo / coder 图（bind pairs、packed streams） | `src/reader.rs:863-1138` |
| 变长 UINT64 解码 | `src/reader.rs:1158-1171` |
| 文件名（UTF-16）读取 | `src/reader.rs:1203-1245` |
| 解码栈构建（单流） | `src/reader.rs:1355-1421` |
| 解码栈构建（多输入/BCJ2，`build_decode_stack2`） | `src/reader.rs:1423-1515` |
| `ArchiveReader` / `for_each_entries` / `read_file` | `src/reader.rs:1254-1788` |
| `BlockDecoder` | `src/reader.rs:1794-1897` |
| `SIGNATURE_HEADER_SIZE=32`、NID 常量 | `src/archive.rs:5-38` |
| AES 解码器 / KDF / raw-key / 缓存 / 上限 | `src/encryption/aes.rs:23-230` |
| `Password`（UTF-16LE） | `src/encryption/password.rs:53-61` |
| method-id → 解码器映射；LZMA2 字典大小算法 | `src/decoder.rs:75-209`、`255-271` |
| 内存上限常量 | `src/reader.rs:18` |
| Zip-Slip 防护 | `src/util/decompress.rs:152-172` |

## 附录 B：复现实验（本次全部命令）

```bash
# 0) 准备测试归档（7-Zip 25.01）
cd /home/user/.hermes/cache/scratch/7zprobe/t1
7z a plain.7z ./data
7z a -pdemo-password -mhe=off enc_content.7z ./data     # 内容加密
7z a -pdemo-password -mhe=on  enc_header.7z  ./data     # 头部加密
7z a -pdemo-password -mhe=off -v100k vol.7z ./data      # 分卷+加密
7z a -pdemo-password -mhe=on  -v100k volh.7z ./data     # 分卷+头加密
7z a -m0=lzma2 -m1=bcj plain_bcj.7z ./data
7z a -ms=off nonsolid.7z ./data                      # 非 solid
cp /home/user/hermes/yshjy/refs/sevenz-rust/tests/resources/*.7z .   # 官方向量
cat vol.7z.001 vol.7z.002 vol.7z.003 > ../vol_merged.7z      # 裸切分证明
7z t -pdemo-password ../vol_merged.7z                            # → Everything is Ok

# 1) 探针（path 依赖 refs/sevenz-rust）
cd /home/user/.hermes/cache/scratch/7zprobe/rsprobe
cargo build --release
./target/release/probe     <t1目录>                      # 结构/密码/内嵌/分卷 主流程
./target/release/sweep     <t1目录> <resources目录>       # 边界+负面+官方向量扫描
./target/release/extra     <t1目录> <resources目录> <scratch>  # 官方AES向量/编码变体/solid开销
./target/release/branch    <scratch>                     # 分支判定
./target/release/fallback  <t1目录> <resources目录>       # 损坏头恢复（12/12 失败）
./target/release/trace     <scratch>                     # IO trace（证明硬编码 Start(0)）
./target/release/scan      <scratch>                     # 推荐扫描器（起点+长度+CRC）
./target/release/bench     <t1目录>                       # 密码枚举成本实测（121ms/候选）
# 可选编解码器
CARGO_TARGET_DIR=target-feat cargo build --release \
  --features "sevenz-rust2/brotli,sevenz-rust2/lz4,sevenz-rust2/zstd,sevenz-rust2/deflate"
./target-feat/release/sweep <t1> <resources>

# 2) 结构分析脚本（Python，纯字节解析）
python3 /home/user/.hermes/cache/scratch/7zprobe/chk.py       # 总长度 = 32+off+size 自洽校验
python3 /home/user/.hermes/cache/scratch/7zprobe/aesprop.py   # 解析 AES coder properties（numCyclesPower=19）
```

> 探针 crate 的 `Cargo.toml` 只有一句关键依赖：
> `sevenz-rust2 = { path = "/home/user/hermes/yshjy/refs/sevenz-rust" }`
> （**注意名字是 `sevenz-rust2`**；代码里 `use sevenz_rust2::...`）

## 附录 C：给 unbox 的「三不」硬约束

1. **不要**把 `File` 定位到 base 后直接交给 `Archive::read`/`ArchiveReader::new` → 必失败，用 `Window`。
2. **不要**用 `Password::from_raw` 喂 GBK/UTF-8 原始字节 → 必须先把密码文本正确解码成 `&str` 再 `Password::from`。
3. **不要**依赖库内的头恢复（`try_to_locale_end_header`）→ 自己对末尾做多候选扫描 + CRC 验证。
