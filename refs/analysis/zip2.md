# zip2 (zip-rs) 读取侧深度分析 —— EOCD / ZIP64 / 前置数据与偏移处理

> 分析对象：`/home/user/hermes/yshjy/refs/zip2/`，crate 名 `zip`，**v9.0.4**（`Cargo.toml:3`），edition 2024，MSRV 1.88，许可 MIT。
> 分析范围：只覆盖**读取/解析**（`src/read/`、`src/format/`、`src/extra_fields/`、`src/types.rs`），写入侧（4.5k 行 `write.rs`）不展开。
> 目的：判断能否用 zip2 直接吃下「MP4 外壳 + 被篡改偏移的 ZIP64」这种文件。
> 行号均对 v9.0.4 源码。

---

## 0. 结论先行（TL;DR）

1. **zip2 只给一个全局标量偏移（`ArchiveOffset`）**，而且是**两条路径、两种算法**：
   - **ZIP32 路径**：用「实际找到的第一条 CDFH 位置 − EOCD 里声明的 cd_offset」反推（等价 Python zipfile 的 concat 逻辑，`find_central_directory.rs:200-209`）；
   - **ZIP64 路径**：**完全不看 CD 在哪**，改用「实际找到的 EOCD64 记录位置 − EOCD64 locator 里声明的 EOCD64 偏移」（`find_central_directory.rs:283`）。
2. 因此，**只要「EOCD64 位置与 locator 声明值的差」≠「真实 CD 位置与 EOCD64 里 cd_offset 的差」，zip2 就必挂**，而且**没有任何 API 能救**——`ArchiveOffset::Known(n)` 在 ZIP64 分支被 locator 的声明值钉死（`find_central_directory.rs:254-270, 326-329`），命中不了就报错、**不回退扫描**。
3. EOCD 搜索窗口是**硬边界**：`file_len − (size_of::<Zip32CentralDirectoryEnd>() + 4 + 65536)`，即**从 EOF 往前 ~65572 字节**（`zip_archive.rs:187-191`）。EOCD 之外再往前的垃圾一律不看。
4. EOCD64 与 EOCD64 locator **必须紧邻**（`locator_offset − eocd64_offset == record_size + 12`，`blocks.rs:532-534` + `find_central_directory.rs:242`），中间夹垃圾必然失败。
5. zip2 **不校验** CD 里记的 local header offset 对不对：它直接 seek 过去要求 `PK\x03\x04`，不对就 `InvalidArchive("Invalid local file header")`（`types.rs:171`）。**零容错、零自愈**。
6. 好消息：zip2 解压单个成员**只用 CD 的 `compressed_size`，完全忽略 local header 里的 csize/usize**（`types.rs:201-210`）。所以「local header 尺寸与 CD 尺寸不一致」这件事对 zip2 **不是问题**（对我们已知的第 3 条现象无影响）——前提是能走到那一步。
7. `ZipArchive::new` 只读**尾部 ~64KB + CD（几十 KB）**，不会扫 8.7GB（除非走到极端回退分支，见 §7.9）。8.7GB 的 IO 不是瓶颈，**偏移对不上才是**。
8. 结论：**建议自研解析定位，再把 zip2 当解压器用**（要么用「最小 8 字节 locator 补丁」把文件喂给 zip2，要么自己解 CD 后用 flate2/zstd 解压）。详见 §8。

---

## 1. 源码地图

| 文件 | 行数 | 职责 |
|---|---|---|
| `src/read/zip_archive.rs` | 1172 | `ZipArchive` 主体：`get_metadata`（EOCD 定位总入口）、`read_central_header`（逐条读 CD） |
| `src/format/find_central_directory.rs` | 370 | **核心**：`find_central_directory_end`（EOCD/EOCD64 双分支 + archive_offset 推导）、`MagicPositionCache` |
| `src/read/magic_finder.rs` | 191 | `MagicFinder<Forward/Backwards>`：1024 字节窗口的魔数扫描器（正/反向） |
| `src/format/blocks.rs` | 673 | 定长块定义与 `FixedSizeBlock::parse`（魔数校验）、`Zip64CentralDirectoryEnd::parse` |
| `src/format/magic.rs` | 58 | 六种魔数常量 |
| `src/read/config.rs` | 20 | **`Config` / `ArchiveOffset`（唯一的偏移开关）** |
| `src/read/mod.rs` | 319 | `CentralDirectoryInfo::try_from`（`directory_start = cd_offset + archive_offset`）、`central_header_to_zip_file_inner`、`ZipReadOptions` |
| `src/extra_fields/zip64_extended_information.rs` | 285 | ZIP64 extra field (0x0001) 解析，**"无哨兵不生效"** 语义 |
| `src/extra_fields/extra_field.rs` | 347 | extra field 循环解析 + `apply_extra_fields`（用 extra 覆盖 CD 字段） |
| `src/types.rs` | 558 | `ZipFileData`：`data_start()` / `find_content()`（真正定位数据字节） |
| `src/read/stream.rs` | 713 | 流式（不可 seek）读取，只用 local header |

---

## 2. EOCD 定位：窗口、方向、候选循环

### 2.1 窗口大小（硬边界）

`src/read/zip_archive.rs:186-193`：

```rust
// Archive-comment limit is 64 KiB.
let min_eocd_position = file_len.saturating_sub(
    size_of::<Zip32CentralDirectoryEnd>() as u64 + size_of::<Magic>() as u64 + (1 << 16),
);

let mut eocd_finder = MagicFinder::new(&EOCD_SIG_BYTES, min_eocd_position, file_len);
let mut cdfh_cache  = MagicPositionCache::new(CDFH_SIG_BYTES);
let mut eocd64_cache = MagicPositionCache::new(EOCD64_SIG_BYTES);
```

- `size_of::<Zip32CentralDirectoryEnd>()` 在 64 位平台 = **32**（4×u16 + 2×u32 = 16，再加 `Box<[u8]>` 16）。
- ⇒ **搜索窗口 = `[file_len − 65572, file_len)`**，非"最后 64KB"字面值，但量级一致：**EOCD 起始位置必须落在文件末尾 65572 字节以内**。
- 超出这个范围：直接 `InvalidArchive("Could not find EOCD")`（`find_central_directory.rs:369`）。
- **注意**：`end` 传的是 `file_len`（真正的 EOF），所以 EOCD **之后允许有垃圾**——见 §2.3 的不等式检查。

### 2.2 反向扫描器（`MagicFinder<Backwards>`）

`src/read/magic_finder.rs`：

- `BUFFER_SIZE = 1024`（`:95`）——**每次 seek + read_exact 只读 1024 字节**。
- `Backwards::reset_cursor`（`:59-64`）：从 `end − 1024` 开始。
- `Backwards::move_cursor`（`:74-87`）：`cursor + 3 − 1024`（3 字节重叠防漏），夹在 `[start, end]`；`cursor <= start` 时终止。
- ⇒ 扫描覆盖的区间正好是 `[min_eocd_position, file_len)`，**没有遗漏、但也没有任何扩展**。

### 2.3 候选循环与合法性检查

`src/format/find_central_directory.rs:93-117`：

```rust
const EOCD_FIXED_SIZE: u64 = 22;
while let Some(eocd_offset) = eocd_finder.next(reader)? {
    let eocd = match Zip32CentralDirectoryEnd::parse(reader) { Ok(e) => e, Err(e) => { …; continue } };
    // ! Relaxed (inequality) due to garbage-after-comment Python files
    if eocd.zip_file_comment.len() as u64 + eocd_offset + EOCD_FIXED_SIZE > file_len {
        parsing_error = Some(invalid!("Invalid EOCD comment length")); continue;
    }
    …
}
```

要点：

- **不是"取最后一个 EOCD 就完事"**，而是**倒序逐个候选试**：每个候选都要过 EOCD 魔数 + 注释长度边界，然后（视情况）进 ZIP32/ZIP64 分支；分支内部失败就 `continue` 试更靠前的候选。
- 注释长度检查是**放宽的不等式**：只要求"注释不能越过 EOF"，**允许注释之后还有垃圾**。
- 全部候选失败后返回最后一次错误，`zip_archive.rs:194-231` 外层还会再来一轮（复用同一个 finder，继续往前吃候选）。
- **CDFH / EOCD64 的魔数位置被缓存**（`MagicPositionCache`，`find_central_directory.rs:23-79`），跨候选/跨外层轮次不会重复扫。`eocd_complexity.rs` 给的是"读放大 ≤ 302%"的线性上界。

### 2.4 ZIP64 是否启用：只看 32 位 EOCD 的哨兵

`src/format/blocks.rs:379-383`：

```rust
pub fn may_be_zip64(&self) -> bool {
    self.number_of_files == u16::MAX
        || self.central_directory_size == u32::MAX
        || self.central_directory_offset == u32::MAX
}
```

⇒ **32 位 EOCD 里三个字段必须至少有一个是哨兵**，否则 zip2 走 ZIP32 分支，**永远不会去看 ZIP64 EOCD**。（有的写入器不写哨兵，zip2 会漏判 → 这是已知坑；`tests/zip64_extra_field_without_sentinel.rs` 讲的是另一件事。）

我们的目标文件：`EOCD.cd_offset == 0xFFFFFFFF` ⇒ `may_be_zip64() == true` ✅。

---

## 3. ZIP64 解析流程

### 3.1 locator（定位 EOCD64）

`find_central_directory.rs:119-138`：

```rust
if eocd.may_be_zip64() {
    // locator 就在 EOCD 前面 20 字节
    let locator64_offset = eocd_offset - (4 + size_of::<Zip64CDELocatorBlock>());
    reader.seek(SeekFrom::Start(locator64_offset))?;
    let locator64 = Zip64CentralDirectoryEndLocator::parse(reader);  // 魔数 PK\x06\x07
    Ok((locator64_offset, locator64?))
}
```

- 位置是**算出来的**，不是搜出来的：`eocd_offset − 20`。
- 魔数不对 ⇒ `.ok()` 变 `None` ⇒ **回退 ZIP32 分支**（`:138-141`、`:143`）。
- `Zip64CentralDirectoryEndLocator` = `{ disk_with_central_directory: u32, end_of_central_directory_offset: u64, number_of_disks: u32 }`（`blocks.rs:389-408`）。
  ⚠️ `end_of_central_directory_offset` 按规范是**相对 ZIP 起点的偏移**，但 zip2 在后面会把它当"声明的绝对位置"用（见 §4.2）。

### 3.2 locator 合法性检查

`find_central_directory.rs:216-225`：

```rust
if locator64.end_of_central_directory_offset >= locator64_offset {  // 声明值 < 实际 locator 位置
    parsing_error = Some(invalid!("Invalid EOCD64 Locator CD offset")); continue;
}
if locator64.number_of_disks > 1 {
    parsing_error = Some(invalid!("Multi-disk ZIP files are not supported")); continue;
}
```

- 第一条把**声明值（相对）**和**实际 locator 位置（绝对）**直接比大小 —— 对"头部被前置了 N 字节"的包是**宽松**的（等于白送 N 字节余量）；但对"声明值被改大"的包是**致命**的（直接判非法）。
- 分卷（`number_of_disks > 1`）**不支持**，直接拒绝。

### 3.3 EOCD64 解析 + "必须紧邻"

`find_central_directory.rs:229-247`：

```rust
fn try_read_eocd64(reader, disk_with_central_directory, expected_length) -> ZipResult<Zip64CentralDirectoryEnd> {
    let z64 = Zip64CentralDirectoryEnd::parse(reader, expected_length)?;
    if z64.disk_with_central_directory != disk_with_central_directory { return Err(invalid!("Invalid EOCD64: inconsistency with Locator data")); }
    if z64.record_size + Zip64CentralDirectoryEnd::RECORD_OVERHEAD != expected_length {
        return Err(invalid!("Invalid EOCD64: inconsistent length"));
    }
    Ok(z64)
}
```

其中 `expected_length = locator64_offset − eocd64_offset`，`RECORD_OVERHEAD = 4 (魔数) + 8 (record_size 字段) = 12`（`blocks.rs:511`）。

⇒ **硬性要求：`EOCD64 记录` 与 `EOCD64 locator` 之间不能有任何一个字节**（`record_size + 12 == locator_offset − eocd64_offset`）。中间塞垃圾的伪装包在这里就废了。

`Zip64CentralDirectoryEnd::parse`（`blocks.rs:513-564`）本身还检查：

- `record_size >= 44 − 4 = 40`（`MIN_SIZE = 44` 是不含魔数的块大小，`blocks.rs:507`；`MIN_FULL_SIZE = 52` ⇒ 含魔数的最小记录 56 字节）；
- `record_size + 12 <= max_size`（不许越过 locator）；
- `record_size > 44` 时把多余的字节当 **extensible data sector** 读进内存（`Vec<u8>`）。

⇒ 若 `record_size` 字段被改成很大（伪装/垃圾），这里会分配一大块内存或直接报错。

### 3.4 EOCD64 定位：先猜、后扫

`find_central_directory.rs:249-364`：

```
search_start = locator64.end_of_central_directory_offset      // ← 声明值，被当绝对位置用
search_end   = locator64_offset                               // 实际 locator 位置

if archive_offset == Known(n):
    guess = (search_start + n).min(search_end)
    if 该位置魔数命中 → 只试这一个候选；失败 ⇒ 直接报 "Could not find EOCD64"，不回退
else (Detect):
    用 eocd64_cache 扫 [search_start, search_end) 内所有 PK\x06\x06 候选，逐个 try_candidate
```

`try_candidate`（`:276-308`）里两个关键点：

```rust
let archive_offset = eocd64_offset - eocd_offset;   // :283   ★★ 绝对位置 − 声明位置
…
if eocd64_offset < eocd64.number_of_files
        .saturating_mul((size_of::<Magic>() + size_of::<ZipCentralEntryBlock>()) as u64)  // = n*46
        .saturating_add(eocd64.central_directory_offset)
{ return Ok(None); }                                // :292-302
```

- **`:283` 是整份分析里最重要的一行**：ZIP64 模式下 `archive_offset` **只由 EOCD64 记录的真实位置与 locator 声明值求差得到**，与真实 CD 在哪、与 CD entry 记的 offset 都**无关**。
- `:292-302` 的守卫把**绝对的** `eocd64_offset` 与 **（相对语义的）`cd_offset + n_files*46`** 相比 —— 语义混用。对"声明 cd_offset 被改大"的包会误杀（见 §8.3 判定表 F3）。
- 注意：**ZIP64 分支从不扫描/校验第一条 CDFH 是否真的在 `directory_start`** —— 直接信任 `cd_offset + archive_offset`，走错了就是后面读 CD 时报错。

---

## 4. 偏移处理：archive_offset 的两条路径

### 4.1 ZIP32 路径（= Python zipfile 的 concat 检测）

`find_central_directory.rs:143-213`：

```rust
let cd_offset_from_eocd = u64::from(eocd.central_directory_offset);

if eocd.number_of_files == 0 {                       // 空包特判 :148-161
    if cd_offset_from_eocd > eocd_offset { …error… }
    return Ok(… archive_offset: eocd_offset.saturating_sub(cd_offset_from_eocd) …);
}
if cd_offset_from_eocd >= eocd_offset { …error… }    // :164-167

let cd_result = match archive_offset {
    ArchiveOffset::Known(n) => {                     // :176-193
        let guess = cd_offset_from_eocd + n;
        if guess 处 4 字节 == PK\x01\x02 { Some(guess) } else { None /* 不回退扫描 */ }
    }
    _ => cdfh_cache.first_in_range(reader, cd_offset_from_eocd, eocd_offset)?,  // :196
};

if let Some(cd_offset) = cd_result {
    let archive_offset = cd_offset - cd_offset_from_eocd;   // :203  ★ 实际 − 声明
    return Ok(…);
}
```

- 即：**在 `[声明的 cd_offset, EOCD)` 区间内找第一条 `PK\x01\x02`**，用它反推偏移。上限是 4 GiB（`cd_offset` 最大 `0xFFFFFFFF`），窗口仍是 1024 字节 → 极端情况下会很慢（`eocd_complexity.rs` 就是压这个的）。
- 与 Python `zipfile` 的 concat 逻辑**思路相同**，但 zip2 是"扫描找第一条 CDFH"而不是"用 EOCD 位置差反推"。

### 4.2 ZIP64 路径

```
archive_offset = 实际 EOCD64 签名位置 − locator.end_of_central_directory_offset
```

（`find_central_directory.rs:283`，返回值经 `:312-323` 或 `:340-353` 冒泡到 `CentralDirectoryEndInfo.archive_offset`）

**与 ZIP32 路径的语义差异是最大的坑**：

| | ZIP32 | ZIP64 |
|---|---|---|
| 基准来自 | EOCD 的 `cd_offset` **vs** 实际第一条 CDFH | locator 的 `end_of_central_directory_offset` **vs** 实际 EOCD64 位置 |
| 是否验证 CD 落点 | **是**（必须命中 CDFH 魔数） | **否** |
| 若哪一方被篡改 | 只要 CDFH 能找到就自愈 | 只要 EOCD64/locator 关系对不上就废 |

两者只有在"**文件内所有内部偏移都被同一个 delta 平移**"时才给出一致结果。

### 4.3 `directory_start` 与每条的 `header_start`

`src/read/mod.rs:50-88`（`CentralDirectoryInfo::try_from`）：

```rust
let (relative_cd_offset, number_of_files, disk_number, disk_with_central_directory) = match &value.eocd64 {
    Some(DataAndPosition { data: eocd64, .. }) => (eocd64.central_directory_offset, …, …),  // ZIP64 用 EOCD64 的值
    _ => (u64::from(value.eocd.data.central_directory_offset), …),                          // 否则用 32 位 EOCD 的值
};
let directory_start = relative_cd_offset.checked_add(value.archive_offset)?;   // :77-79  ★
```

`src/read/zip_archive.rs:271-312`（`read_central_header`）：`reader.seek(directory_start)` 起，循环 `number_of_files` 次调 `ZipCentralEntryBlock::parse`（魔数必须是 `PK\x01\x02`，否则 `terminal_error` + 返回 Err）。随后：

```rust
central_header_to_zip_file_inner(reader, /*archive_offset=*/0, central_header_start, block)  // :299
file.header_start = file.header_start.checked_add(dir_info.archive_offset)?;                 // :307-310
```

`src/read/mod.rs:216-241`（`central_header_to_zip_file_inner`）：

```rust
let mut result = ZipFileData { …, header_start: offset.into() /* CD 里的 32 位 offset */, … };  // :226
result.apply_extra_fields(&mut file_name_raw)?;                                                 // :233 ★ 可能覆盖
result.header_start = result.header_start.checked_add(archive_offset)?;                         // :236-239 ★
```

⇒ 每条成员的实际定位：

```
真实数据位置 = (CD entry 的 offset 字段，或被 ZIP64 extra 覆盖后的值) + archive_offset
```

**整个归档只有一个标量 `archive_offset`。** 没有 per-entry 修正，没有二次校准。

### 4.4 `ArchiveOffset::Known(n)` 到底能干什么

`src/read/config.rs:6-20`：

```rust
pub struct Config { pub archive_offset: ArchiveOffset }
pub enum ArchiveOffset { Detect /*默认*/, Known(u64) }
pub fn with_config(config: Config, mut reader: R) -> ZipResult<ZipArchive<R>>   // zip_archive.rs:420
```

- ZIP32：`Known(n)` ⇒ 只检查 `cd_offset + n` 处是否有 `PK\x01\x02`，**否则直接报错不扫描**（`:186-188`）。
- ZIP64：`Known(n)` ⇒ 只检查 `locator_claim + n` 处是否有 `PK\x06\x06`，**否则直接报错不扫描**（`:254-270, 326-329`）。

⇒ `n` 的**唯一**合格定义是：`n = 实际 EOCD64 位置 − locator 声明值`（ZIP64）/ `n = 实际第一条 CDFH 位置 − EOCD 声明 cd_offset`（ZIP32）。
然后要求 `cd_offset + n == 真实 CD 位置`、`offset_i + n == 真实 local header 位置`。

**结论：`Known(n)` 不是"告诉 zip2 ZIP 从第几字节开始"的自由参数，而是"整个归档的内部偏移统一平移了 n 字节"的声明**，且 ZIP64 下这个声明是被 locator 记账的。它**不比 `Detect` 更强**（Detect 会自动算出同一个 n），只在"想跳过扫描/想强制失败"时有意义。

### 4.5 防御性检查清单（会拒绝文件的地方）

| # | 检查 | 位置 |
|---|---|---|
| 1 | EOCD 必须在末尾 65572 字节内 | `zip_archive.rs:187-191` |
| 2 | EOCD 注释不得越过 EOF | `find_central_directory.rs:114-117` |
| 3 | 32 位 EOCD 至少一个哨兵，否则不进 ZIP64 分支 | `blocks.rs:379-383` |
| 4 | locator 必须在 `eocd_offset−20` 处且魔数正确 | `find_central_directory.rs:130-138` |
| 5 | `locator.claim < 实际 locator 位置` | `:217-220` |
| 6 | `number_of_disks <= 1` | `:222-225` |
| 7 | EOCD64 与 locator **紧邻** | `:242` + `blocks.rs:532` |
| 8 | EOCD64.disk == locator.disk | `:237-239` |
| 9 | **`eocd64_offset >= n_files*46 + eocd64.cd_offset`** | `:292-302` |
| 10 | ZIP32：`cd_offset < eocd_offset`；第一条 CDFH 魔数命中 | `:164-167, 176-198` |
| 11 | 每条 CD entry 魔数 = `PK\x01\x02` | `read/zip_archive.rs:275-282` |
| 12 | ZIP64 extra field 长度自洽（`len >= 已消费`） | `zip64_extended_information.rs:226-228` |
| 13 | local header 魔数 = `PK\x03\x04`（**定位数据时才查**） | `types.rs:171` |

---

## 5. ZIP64 extra field 的解析（"无哨兵不生效"）

`src/extra_fields/zip64_extended_information.rs`，APPNOTE 4.5.3，`0x0001`，字段顺序 `[uncompressed u64][compressed u64][header offset u64][disk u32]`。

关键设计（v9 的新语义，`:143-241`）：

```rust
fn read_field(reader, len, consumed_len, is_zip64) -> ZipResult<Option<u64>> {
    if len < 24 && !is_zip64 { return Ok(None); }        // :155-157 空转，但字节照样消耗
    let value = reader.read_u64_le()?;
    *consumed_len += 8;
    Ok(is_zip64.then_some(value))                        // ★ 只有条目字段是哨兵才"保留"
}
```

- **是否"读"由块长度决定，是否"用"由对应条目字段是否为 `0xFFFFFFFF` 决定**（`:186-212`）。
- 头部 offset 字段：`entry_header_start.is_some_and(|s| u64::from(s) == ZIP64_BYTES_THR)`（`:211`）——**local header 没有这个字段，传 `None`，所以 local 里的 ZIP64 extra 永远不覆盖 offset**。
- 多出来的字节用 `reader.take(leftover_len)` 丢掉（`:229-235`），保留解析器步进一致。

覆盖点：`src/extra_fields/extra_field.rs:241-258`

```rust
ExtraField::Zip64ExtendedInformation(zip64_block) => {
    self.large_file = true;
    if let Some(Zip64Sizes{sizes}) { uncompressed_size = …; compressed_size = …; }   // 尺寸被覆盖
    if let Some(head_start) = zip64_block.header_start { self.header_start = head_start; }  // offset 被覆盖
}
```

**对我们场景的直接含义（重要）**：

- 如果 CD entry 的 4 字节 offset ≠ `0xFFFFFFFF`（即使旁边挂了一个 ZIP64 extra），**ZIP64 extra 里的 64 位 offset 会被丢弃**，zip2 用 CD 的 4 字节字段 + `archive_offset`。
- 如果 CD entry 的 offset == `0xFFFFFFFF`，zip2 **优先信任 ZIP64 extra 里的 64 位 offset**。
  ⇒ 篡改者改的是"4 字节字段"还是"extra 里的 8 字节字段"，决定我们要补哪一处（见 §8.4）。
- 尺寸同理：`csize/usize` 字段是哨兵时，用 extra 里的值；否则用字段里的值。**zip2 从不读 local header 的尺寸**（`find_content` 用 `data.compressed_size`，`types.rs:209`）。

---

## 6. 数据字节定位（真正的读取路径）

`src/types.rs:163-210`：

```rust
pub fn data_start(&self, reader) -> ZipResult<u64> {           // :163  OnceLock 缓存
    reader.seek(SeekFrom::Start(self.header_start))?;         // :168  ★ 直奔 CD 给的 header_start
    let block = ZipLocalEntryBlock::parse(reader)?;           // :171  ★ 必须 PK\x03\x04，否则 Err
    let variable_fields_len = block.file_name_length + block.extra_field_length;
    let data_start = self.header_start + (4 + size_of::<ZipLocalEntryBlock>()) + variable_fields_len;  // :178-184
    …
}
pub(crate) fn find_content(&self, reader) -> ZipResult<Take<&mut R>> {   // :201
    let data_start = self.data_start(reader)?;
    reader.seek(SeekFrom::Start(data_start))?;
    Ok(reader.take(self.compressed_size))                     // ★ 用 CD 的 compressed_size
}
```

- `ZipLocalEntryBlock` 定长 26 字节（`blocks.rs:218-229`），加魔数 30。
- ⇒ **`header_start` 错一字节 → 立刻 `InvalidArchive("Invalid local file header")`**。这是 zip2 对我们这类文件的典型报错。
- **local header 里的 csize/usize 完全不参与定位与长度约束** ⇒ 我们已知的"local 尺寸与 CD 尺寸差 2355200"对 zip2 无影响（但对 7-Zip 之类会读 local 头做校验的工具就有影响）。
- 只读成员数据，不读整文件：`take(compressed_size)` + 解压器流式处理（`readers.rs:298-342`，deflate/zstd/bzip2/lzma/xz/ppmd/deflate64 由 feature 决定）。**支持真正的"不解全文件就提取单个成员"** ✅。

---

## 7. 关键 API 清单

### 7.1 打开 / 偏移

| 签名 | 位置 | 用途 |
|---|---|---|
| `ZipArchive::<R>::new(reader: R) -> ZipResult<ZipArchive<R>>` | `zip_archive.rs:362` | 默认 `Detect` |
| `ZipArchive::<R>::with_config(config: Config, reader: R) -> ZipResult<ZipArchive<R>>` | `zip_archive.rs:420` | **唯一能手动指定基准偏移的入口** |
| `pub struct Config { pub archive_offset: ArchiveOffset }` | `read/config.rs:6` | 配置结构 |
| `pub enum ArchiveOffset { Detect, Known(u64) }` | `read/config.rs:13` | `Detect`(默认) / `Known(n)` |
| `ZipArchive::offset(&self) -> u64` | `zip_archive.rs:448` | **返回它算出的 archive_offset**（诊断探针！） |
| `ZipArchive::central_directory_start(&self) -> u64` | `zip_archive.rs:435` | 返回它算出的 CD 绝对位置 |
| `ZipArchive::comment(&self) -> &[u8]` | `zip_archive.rs:453` | EOCD 注释原文 |
| `ZipArchive::raw_zip64_extensible_data_sector(&self) -> Option<&[u8]>` | `zip_archive.rs:458` | ZIP64 EOCD 的 extensible data |
| `ZipArchive::metadata(&self) -> Arc<ZipArchiveMetadata>` | `zip_archive.rs:370` | 元数据快照（可跨线程复用） |
| `unsafe ZipArchive::unsafe_new_with_metadata(reader, metadata) -> Self` | `zip_archive.rs:410` | 免重复解析、并行解压 |

> ⚠️ 路径：`Config`/`ArchiveOffset` 在 **`zip::read::`** 下（`read/mod.rs:17`），**不在 crate 根**（`lib.rs:17-24` 没 re-export）。写代码时是 `zip::read::{Config, ArchiveOffset}`。

### 7.2 遍历 / 读取成员

| 签名 | 位置 | 用途 |
|---|---|---|
| `len(&self) -> usize` / `is_empty()` | `:430, :440` | 成员数 |
| `by_index(&mut self, i) -> ZipResult<ZipFile<'_,R>>` | `:683` | 解压读（流式） |
| `by_name(&mut self, name) -> ZipResult<ZipFile<'_,R>>` | `:539` | 按名 |
| `by_path<T: AsRef<Path>>(&mut self, p) -> ZipResult<ZipFile<'_,R>>` | `:597` | 按路径 |
| `by_index_raw(&mut self, i) -> ZipResult<ZipFile<'_,R>>` | `:693` | **不解压**，拿原始（压缩后）字节流 |
| `by_index_seek(&mut self, i) -> ZipResult<ZipFileSeek<'_,R>>` | `:625` | **可 seek**，仅 `Stored`（`UnsupportedArchive` otherwise） |
| `by_index_data(&self, i) -> ZipResult<ZipFileEntry<'_>>` | `:688` | 只要元数据 |
| `by_index_with_data_descriptor(&mut self, i)` | `:553` | 顺带解析 data descriptor |
| `by_index_with_options(&mut self, i, ZipReadOptions)` | `:708` | 带密码/覆盖长度/忽略 CRC |
| `file_names(&self) -> impl Iterator<Item=ZipResult<Cow<'_,str>>>` | `:463` | 文件名流 |
| `has_overlapping_files(&mut self)` / `has_overlapping_entries(&mut self)` | `:480, :493` | 反 zip-bomb/重叠检测 |
| `root_dir(&self, filter)` | `:737` | 探测单一顶层目录 |
| `extract(&mut self, dir)` / `extract_with_options(...)` / `extract_unwrapped_root_dir(...)` | `read/extract.rs:332, 340, 406` | 落盘 |
| `ZipArchive::decompressed_size(&self) -> Option<u128>` | `:149` | 预估总量（含 data descriptor 时 `None`） |

### 7.3 `ZipFile`（成员）上的常用方法

`name()`, `enclosed_name() -> Option<PathBuf>`（安全路径）, `name_raw() -> &[u8]`, `header_start() -> u64`, `central_header_start() -> u64`, `data_start() -> Option<u64>`（**需已有读取动作填充**）, `compressed_size()`, `size()`, `crc32()`, `compression()`, `encrypted()`, `is_dir()`, `is_symlink()`, `unix_mode()`, `external_attributes()`, `comment()`, `last_modified()`, `extra_data_fields() -> impl Iterator<Item=&ExtraField>`（`read/zipfile.rs:62-216, 320-378`）。

### 7.4 `ZipReadOptions`（`read/mod.rs:251-318`）

```rust
ZipReadOptions::new()
    .password(Option<&[u8]>)            // 解密
    .ignore_encryption_flag(bool)       // 忽略加密位，当明文读（对付假加密位）
    .ignore_crc32(bool)
    .override_compressed_size(u64)      // 覆盖 CD 的 csize
    .override_uncompressed_size(u64)
    .override_crc(u32)
```

> `override_compressed_size` / `override_uncompressed_size` 是**只作用于当次读取**的 override，**不能改 CD 解析阶段的定位**。对我们"CD 尺寸不可信"的场景，它只能救"长度"、救不了"位置"。

### 7.5 流式（不可 seek）API

| 签名 | 位置 | 备注 |
|---|---|---|
| `read_zipfile_from_stream<R: Read>(reader) -> ZipResult<Option<ZipFile<'_,R>>>` | `stream.rs:231` | **只用 local header**，遇到 CDFH/EOCD 魔数返回 `None` |
| `read_zipfile_from_stream_with_compressed_size(reader, u64)` | `stream.rs:237` | data descriptor 条目必须给尺寸，否则 `UnsupportedArchive`（`stream.rs:311-319`） |
| `read_zipfile_from_stream_with_options(reader, ZipReadOptions)` | `stream.rs:247` | LZMA 需要 `override_uncompressed_size` |
| `ZipStreamReader::new(r).visit(v)` / `.extract(dir)` | `stream.rs:29, 53, 86` | 顺序解析 + 回读 CD 补元数据 |

---

## 8. 对我们的启示：怎么用 zip2 处理被篡改偏移的 ZIP

### 8.1 先明确我们面对的是什么

已知（实测）：

```
F  = 8711803337                    // 总文件长
MP4 外壳     119248031 字节
ZIP64 数据  8592555306 字节
EOCD.cd_offset            = 0xFFFFFFFF          （哨兵 → zip2 会进 ZIP64 分支 ✅）
EOCD64.cd_offset  与真实 CD 位置相差 2355200
local header 的 csize/usize 与 CD 记录值相差 2355200
7-Zip 报 "Cannot open the file as [zip] archive"
我们自算 ZIP起点 = CD实际位置 − ZIP64EOCD的cd_offset 后能正确提取
```

### 8.2 诊断：三个 delta 必须相等

定义（都要在真实文件上量出来）：

| 符号 | 含义 |
|---|---|
| `C` | 真实第一条 CDFH（`PK\x01\x02`）的绝对偏移 |
| `D64` | ZIP64 EOCD 里 `central_directory_offset` 字段的值 |
| `P` | 真实 EOCD64 记录（`PK\x06\x06`）的绝对偏移 |
| `L` | EOCD64 locator 里 `end_of_central_directory_offset` 字段的值 |
| `O_i` / `H_i` | 第 i 条 CD entry 记的 local header offset / 它的真实位置 |

| 量 | 值 |
|---|---|
| `Δ_cd` = `C − D64` | **我们的 `ZIP起点` 就是它** |
| `Δ_eocd64` = `P − L` | **zip2 `Detect` 会算出的 `archive_offset`** |
| `Δ_entry` = `H_i − O_i`（抽样校验是否恒定） | |

**判定表：**

| 情形 | zip2 行为 |
|---|---|
| **A.** `Δ_eocd64 == Δ_cd == Δ_entry ≡ Δ`（全局一致平移） | ✅ **`ZipArchive::new()` 直接可用**；`archive.offset()` 会返回 `Δ`。也可以显式 `with_config(Known(Δ))` |
| **B.** `Δ_eocd64 == Δ_cd` 但 `Δ_entry` 不一致 | ❌ 开得了包、读不出数据（`Invalid local file header`）；`override_*` 救不了；**必须自研或逐条 patch CD** |
| **C.** `Δ_eocd64 ≠ Δ_cd`（我们文件很可能是这种） | ❌ `directory_start` 落错 → `InvalidArchive("Invalid Central Directory header")`。`Known(n)` **也救不了**：`n` 被 `n = P − L` 钉死 |
| **D.** `L ≥ 实际 locator 位置`，或 EOCD64 与 locator 之间有垃圾 | ❌ `Invalid EOCD64 Locator CD offset` / `Invalid EOCD64: inconsistent length` |
| **E.** `P < n_files*46 + D64`（`D64` 被改大） | ❌ `Could not find EOCD64`（`:292-302` 守卫误杀）；若 `D64` 被改大很多，`n_files` 一多必然踩 |
| **F.** EOCD 距 EOF > 65572 字节（尾部垃圾很长） | ❌ `Could not find EOCD`，**与偏移无关**，纯窗口问题 |
| **G.** 32 位 EOCD 三个字段都不是哨兵 | ❌ 走 ZIP32 分支、`cd_offset = 0xFFFFFFFF` 当真值 → 在 ~4GB 区间盲扫 CDFH，极慢且大概率找错 |

**建议先跑这个诊断**（伪代码，不用真的编译 zip2）：

```python
# diagnose.py —— 只读尾部几十 MB，不要扫 8.7GB
F = os.path.getsize(path)
tail = read(path, max(0, F - 64*1024*1024), F)              # CD/EOCD 区都很小，64MB 足够
base = F - len(tail)
eocd_off  = base + tail.rindex(b"PK\x05\x06")               # 最后一个 EOCD（zip2 是倒序逐个试，先取最后一个）
assert eocd_off >= F - 65572, "F 情形：EOCD 超出 zip2 窗口"
loc_off   = eocd_off - 20                                   # zip2 就是这么算的
assert tail[loc_off-base:loc_off-base+4] == b"PK\x06\x07",  "D 情形：locator 不在预期位置"
L         = u64(tail, loc_off-base+8)                       # locator.end_of_central_directory_offset
P         = base + tail.rindex(b"PK\x06\x06", 0, loc_off-base)   # 真实 EOCD64
D64       = u64(tail, P-base+48)                            # EOCD64.central_directory_offset（偏移 48）
n_files   = u64(tail, P-base+32)
assert loc_off - P == u64(tail, P-base+4) + 12,             "D 情形：EOCD64 与 locator 不紧邻"
C         = base + first_cdfh(tail, P)                      # 真实第一条 CDFH
print("Δ_eocd64 =", P - L, " Δ_cd =", C - D64, " n_files =", n_files, " 守卫 =", P >= n_files*46 + D64)
```

- `Δ_eocd64 == Δ_cd` 且守卫/紧邻都过 ⇒ **zip2 可以直接用**；
- 否则 ⇒ 走 8.3 的方案。

### 8.3 方案 A（推荐）：给 zip2 打「最小 8 字节补丁」

因为 `Detect` 在 ZIP64 下算 `n = P − L`，而 `directory_start = D64 + n`，我们只要**让 `L` 满足 `P − L' = C − D64`**，zip2 就会自己算出正确的 `n` 和 `directory_start`：

```rust
let n = C - D64;                 // 我们自研解析器知道 C 和 D64
let l_prime = P - n;             // 待写入 locator 的新值

// 虚拟文件：前段直通，尾部窗口在内存里改过
struct Patched<R> { inner: R, base: u64, data: Vec<u8> }   // 总长必须保持 == F
// seek 语义：pos < base → inner.seek(pos)；否则读 data[pos-base..]
let reader = Patched::new(file, loc_off, patch_u64_at(file, loc_off + 8, l_prime));
let mut zip = zip::ZipArchive::new(reader)?;
assert_eq!(zip.offset(), n);                        // 自检
assert_eq!(zip.central_directory_start(), C);       // 自检
```

- 补丁只在一个**连续尾部区域**（`CD + EOCD64 + locator + EOCD`，通常几十 KB～几 MB）里改字节，**不移动任何东西**，所以文件长度、EOCD 注释边界检查全不受影响。
- **必须保留 EOCD 里的 `0xFFFFFFFF` 哨兵**，否则 `may_be_zip64()` 变 false，zip2 直接走 ZIP32 分支（§2.4）。
- 若 `Δ_entry` 也一致（情形 B 不成立），到此为止即可。
- 若每条 CD entry 的 offset 各自被改（`H_i − O_i` 不恒定），再逐条就地改写：
  - CD entry 的 offset 字段非 `0xFFFFFFFF` ⇒ 改 4 字节（`CDFH + 42`）；
  - 是 `0xFFFFFFFF` ⇒ 改 ZIP64 extra 里的 8 字节（需按 `extra_field_length` 步进找 `0x0001` 块）；
  - 取值为"真实 absolute 位置"还是"`H_i − n`"都行，只要与 `n` 自洽。
- 代价：一次尾部区域读 + 几十 KB 内存，**不改盘、不复制 8.7GB**。

### 8.4 方案 B：`with_config(Known(n))`（只在"全局一致平移"时能用）

```rust
let mut zip = zip::ZipArchive::with_config(
    zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(n) },
    file,
)?;
```

- 适用：`Δ_eocd64 == Δ_cd == Δ_entry`（情形 A）。此时 `Detect` 也能成功，`Known` 只是省掉 EOCD64 扫描。
- **不适用**我们的候选场景：`Known` 不改变"基准来自 locator"这一事实，也不提供 per-entry 修正。
- 附带好处：`Known` **禁止回退扫描**，能在"偏移对不上"时**立刻**给出 `Could not find EOCD64` / `Invalid CDFH offset`，比 `Detect` 盲扫 4GB 更快、更适合做探针。

### 8.5 方案 C：自定义 `Read + Seek` 重映射包装器 —— 为什么它不比 A/B 强

想法很自然："包一层 reader，把 zip2 的 seek 位置 + delta 再转发给真实文件"。问题是：

- zip2 的 seek 是**绝对的、语义混合的**：它会去 `[file_len−65572, file_len]` 扫 EOCD、去 `directory_start` 读 CD、去 `header_start_i` 读 local header、去 `P−20` 读 locator。**一个平移量无法同时满足这四类位置**（这正是 §4.2 两条路径基准不同的根源）。
- 全局常量平移 ⇒ 数学上等价于 `Known(delta)`，没有额外能力。
- 分段映射（"读 CD 区加 `Δ_cd`，读 local header 加 `Δ_entry`"）需要 reader **知道这次 seek 的语义**，而 reader 只拿到一个 `u64` —— 除非我们把"已知的 CD 区/每条 header 位置表"硬编码进去，那其实就是把方案 A 的补丁表换了个位置放，反而更脆。
- 唯一有意义的本地技巧：**用一个会打印所有 `(seek 目标, 读到的前 4 字节)` 的探针 reader 观察 zip2 把 CD 定在哪** —— 这是很好的黑盒诊断手段（配合 `zip.offset()` / `zip.central_directory_start()`）。

### 8.6 方案 D：完全自研解析（建议作为主路径）

理由（对着我们的文件逐条）：

1. zip2 只有**一个全局偏移标量**，而我们的文件**至少有两个基准**（CD/目录表基准 vs local header 基准）且它们相差的不是同一个量；CD 尺寸与 local 尺寸还各自不一致。
2. ZIP64 下 zip2 的偏移推导**基准是 locator 而不是 CD**，与我们的公式不同源，无法通过配置对齐（§8.2 情形 C）。
3. zip2 对 CD→local header 的落点**零容错**（§6），而伪装包恰恰会破坏这里；救援场景（DESIGN.md 的"救援模式"）本来就需要扫描式恢复，zip2 没有这个能力。
4. 我们的产品要求「不依赖被篡改的目录表」「报告残缺/救援」——这些 zip2 天然给不了。
5. 但 zip2 的**解压能力**（deflate/zstd/bzip2/lzma/xz/ppmd/deflate64 + AES-256/192/128 + AE-2）很值得复用，只是 `Decompressor` 是 `pub(crate)`（`lib.rs:32` 只 `pub use compression::{CompressionMethod, SUPPORTED_COMPRESSION_METHODS}`），**不能单独借用**。要么绕道方案 A（把自研的定位结果喂给 zip2），要么自己接 `flate2` / `zstd` 等（`unbox/Cargo.toml` 已经有 `flate2`）。

**推荐组合**：

```
自研：定位 EOCD/locator/EOCD64/CD（不依赖被篡改字段） + 算 Δ + 逐条校验 PK\x03\x04
  ├─ 若 Δ 全局一致（情形 A）  → ZipArchive::new（零成本复用）
  ├─ 若只差 locator/几条字段 → 方案 A 的尾部补丁 → ZipArchive::new
  └─ 若 CD/条目彻底被毁        → 自研救援扫描 + 自接 flate2/zstd 解压（并复用 zip2 只做 CRC/AES？不行，AES 也私有）
```

### 8.7 其他要注意的可用性/合规点

- **许可 MIT** ✅ 符合 DESIGN.md 的"只用开源技术"红线。
- **默认 feature 不含 `legacy-zip`**（`Cargo.toml:105-115`）：**传统 ZipCrypto 加密（`PK\03\04` + 12 字节加密头）默认不可用**，要显式 `features = ["legacy-zip"]`。中文圈资源包大量用 ZipCrypto ⇒ **一定要开**。
- `aes-crypto` 在默认 feature 里 ✅（AES-256/192/128、AE-1/AE-2）。
- 默认 feature 还包含 `bzip2/zstd/lzma/xz/ppmd/deflate64` —— 全开会让编译变重；只需 deflate 时用 `default-features = false, features = ["deflate-flate2-zlib-rs"]`。
- edition 2024 + MSRV 1.88，与 `unbox`（edition 2024）一致 ✅。

---

## 9. 意外发现 / 易踩的坑清单

1. **两条路径两种偏移算法**（§4.1 vs §4.2）：ZIP32 用 CD 实际位置反推（有自愈能力的扫描），ZIP64 用 EOCD64 实际位置反推（无验证、无自愈）。**同一个库，行为随归档是不是 ZIP64 而变**，这是最容易误判的地方。
2. **`ArchiveOffset::Known(n)` 在 ZIP64 下被 locator 钉死**（`find_central_directory.rs:254-270, 326-329`）：猜错就 `continue`，而且明确写了 "Known offset didn't work — don't fall back to scanning"。它不是"手动指定 ZIP 起点"的自由参数。
3. **EOCD64 与 locator 必须零间隙**（`:242` + `blocks.rs:532`），这是"Junk 藏在两个结构之间"型伪装包的必杀点。
4. **守卫把绝对偏移和相对偏移混着比**（`:292-302`）：`eocd64_offset(绝对) < n_files*46 + cd_offset(相对)`。对"声明 cd_offset 被改大"的包会误杀 ⇒ `Could not find EOCD64`；对"头部被前置"的包反而宽松。
5. **ZIP64 extra field "无哨兵不生效"**（`zip64_extended_information.rs:143-167`）：CD entry 的字段不是哨兵时，extra 里的值**被丢弃但字节被消耗**。所以 zip2 对"只改了 extra 没改字段"的篡改是免疫的；反过来，字段是哨兵时就优先信任 extra ⇒ **要知道改哪儿，先看哨兵**。
6. **local header 的尺寸从不参与读取**（`types.rs:201-210`）：所以"local 尺寸 ≠ CD 尺寸"这类伪造对 zip2 无效，但对 7-Zip 有效——**这解释了为什么 7-Zip 打不开而 zip2 可能打得开**（前提是偏移对得上）。
7. **EOCD 搜索窗口 = 65572 字节**（公式见 §2.1），且 `MagicFinder` 每次只读 1024 字节 —— 尾部有长垃圾（>65KB）的包 zip2 直接判死，与我们精心计算的偏移毫无关系。
8. **极端回退分支会盲扫 ~4GB**：EOCD64 无效 + `EOCD.cd_offset == 0xFFFFFFFF` 时，ZIP32 分支会在 `[0xFFFFFFFF, eocd)` 区间用 1024 字节窗口找 `PK\x01\x02`（`find_central_directory.rs:196`）—— 慢、且可能命中数据区里的假 CDFH，进而算出垃圾 `archive_offset`。`eocd_complexity.rs` 就是为退化性能写的回归测试（读放大 ≤302%）。
9. **EOCD 注释之后允许垃圾**（`:114` 是放宽的不等式）——如果我们的伪装包是"ZIP 在前、MP4 躯干在后"，EOCD 之后的垃圾是 OK 的，只要注释仍在窗口内。
10. **`MagicPositionCache` / `eocd_finder` 会跨"外层重试轮"复用**（`zip_archive.rs:191-193` + `:194-231`）：所以 zip2 对多候选 EOCD 的处理是**线性且不重复扫**的，这一块设计相当好，别以为它会指数爆。
11. **`by_index_seek` 只支持 `Stored`**（`zip_archive.rs:632-641`），压缩成员想随机 seek 会报 `UnsupportedArchive`。
12. **`read_zipfile_from_stream` 走的是 local header 语义**（`stream.rs` 全家桶）：data descriptor 条目**必须由调用方给 `compressed_size`**（`:311-319`），否则 `UnsupportedArchive("The file length is not available in the local header")`。对我们"local 头不可信"的场景，这条路基本不能用。
13. **`ZipFile::data_start()` 是 `Option`**：`None` 表示"还没读过、没填 OnceLock"（`types.rs:164-165`），不是"没有数据"——别误判。
14. **`Config`/`ArchiveOffset` 在 `zip::read::` 下**，crate 根没有 re-export（`lib.rs:17-24`），写 `use zip::ArchiveOffset` 会编译失败。
15. **`try_read_eocd64` 用 `record_size` 直接 `vec![0u8; record_size as usize - 44]` 分配**（`blocks.rs:536-539`）：`record_size` 是文件里的 8 字节字段（被篡改者可控），配上 `max_size` 上限（必须小于到 locator 的距离）才安全 —— 我们的工具若自研，记得别照抄这个"直接分配"的写法。
16. **`unsafe_new_with_metadata` + `metadata()`** 是官方推荐的并行解压姿势（`zip_archive.rs:374-415`）：8.7GB 包多线程解压时可以省掉重复解析 CD。
17. zip2 里 `find_central_directory_end` 和 `MagicPositionCache` 都是 `pub(crate)`（`format/mod.rs:25-26, 8`）**不可外部复用**；能用的只有 `ZipArchive` 这一层。

---

## 10. 附：与 Python `zipfile` / 7-Zip 的定位差异速查

| | Python `zipfile` | zip2 (zip-rs) 9.0.4 | 7-Zip |
|---|---|---|---|
| EOCD 搜索 | 尾部固定窗口（默认 64KB + 注释），只认**最后一个** EOCD | 尾部 65572 字节，**倒序逐个候选**试 | 尾部窗口，逐个候选 |
| ZIP32 concat 偏移 | `concat = EOCD位置 − cd_size − cd_offset`（**用位置差算，不扫 CDFH**） | 扫描找第一条 `PK\x01\x02`，再算差 | 类似 Python，并校验 |
| ZIP64 偏移 | 同上思路，用 locator/EOCD64 的位置差 | 只用 `实际EOCD64位置 − locator声明值` | 用 locator 位置差 + CD 落点校验 |
| CD→local 落点校验 | 读 local 头，尺寸不符时按 CD 覆盖 | **不校验，直接 seek，魔数不对就报错** | **校验**（因此对伪造的 local 尺寸零容忍 → 这正是它打不开的原因之一） |
| 手动指定偏移 | 无 | `ArchiveOffset::Known(n)`（半残，见 §4.4） | `-m` 之类无对应 |

---

## 11. 一句话给决策

**不要指望 zip2 自己解析这个文件**（它的 ZIP64 偏移基准来自 locator 而非 CD，且只支持一个全局标量）；
**用我们自己的解析器算 `Δ = C − D64`，再通过「8 字节 locator 补丁 + 尾部窗口虚拟 reader」把文件交给 `zip::ZipArchive::new`**，
这样既保住 zip2 成熟的解压/校验/并行能力，又完全绕开它那套对不上号的偏移推导。

（若诊断表落到情形 A —— 全局一致平移 —— 那就一行 `with_config(Known(Δ))` 收工。）
