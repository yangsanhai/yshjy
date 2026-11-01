# 实现来源账（IMPL-PROVENANCE）

> 由来：用户担心「写的时候自己发明了一套，参考项目里有解法但没抄」。
> 经 P1~P6 六批实测（126 个样本 + 126 条提交 + 死代码扫描），事实是：
> **不支持的算法/编排问题只有 1 个（RAR 头加密），其余全是「接线错了」或「我们比参考强」。**
>
> 判定四类：**A** 参考有解我们更弱（要改）· **B** 我们更好（不换，写注释）·
> **C** 参考也没有（标风险加测试）· **D** 参考的解法本身是历史包袱（不抄）
>
> 来源报告：`OPTIMIZE-ZIP-CORPUS.md` · `-7Z-CORPUS.md` · `-OWN-SAMPLES.md` · `-RAR-CORPUS.md` · `-WIRING-AUDIT.md`
> 建立：2026-11-02

---

## 一、A 类 · 该改的（按「伤信任 ÷ 成本」排序）

### A-1 · RAR5 头加密（`-hp` 加密文件名）完全不支持【风险最高】

| 项 | 内容 |
|---|---|
| 触发 | WinRAR 勾「加密文件名」的包 —— 国内隐私包基本都开 |
| 我们的表现 | 不给密码 → 「**分卷不齐 —— 后面还有卷没下回来**」；给对密码 → 「**这不是压缩包**」 |
| 对照 | `unrar t -pPASS123` → `All OK`，4 文件全过 |
| 我们的落点 | `src/extract_rar.rs:89`（单卷）、`:125`（分卷） —— `read_path(path)` **没传 options** |
| 参考实现 | `rars-0.11.0/src/lib.rs:1403` `read_path_with_options` + `:261` `with_password`；库自带测试夹具 `tests/fixtures/rar50/header_encrypted.rar` |
| 判定 | **A**（库能做的我们没调用）|
| 修法 | ① 打开改 `read_path_with_options` ② 失败且错误是 `NeedPassword` → 用三编码密码候选重开 ③ 错误映射改为 `NEED_PASSWORD`，绝不落 `MISSING_VOLUME` |
| 成本 / 风险 | 1 天 / 低 |
| **状态** | ✅ **已修 2026-11-02** —— 新增 `打开归档(path, password)`：先不带密码 `read_path`（明文包最快），只有 `NeedPassword` 才轮到三编码候选 `read_path_with_options`；不给密码报 `HeadEncrypted`、给错报 `WrongPassword`，绝不落 `MISSING_VOLUME`。单卷（`extract_rar_file_ctx`）和多卷（`extract_rar_volumes_ctx`，密码类错误原样上抛不被吞成「第 N 卷读不出来」）都改了。样本收进 `tests/data/rar5_header_encrypted.rar`（302 B，rars 夹具，密码 `password`）。回归测试 `extract_rar::tests::头加密rar_三态`（不给 → HeadEncrypted / 给错 → WrongPassword / 给对 → 解出 hello.txt），**做过反向验证**：临时改回不带密码的 `read_path`，测试立刻红并报出 `实际 NotArchive（a password is required）`，改回才绿。多卷头加密另用库夹具三卷实测：不给报头加密、对密码校验通过（4 KB 三卷拼好）、错密码报密码不对 |

### A-2 · 不支持的压缩方法 → 谎报「数据丢了 / 不是压缩包」

| 项 | 内容 |
|---|---|
| 触发 | zip 里用 LZMA(14)/PPMd(98)/XZ(95)/Implode(6)/Reduce/Shrink/Deflate64-raw；7z 里用 zstdmt-brotli / zstdmt-lz4 |
| 我们的表现 | zip → 「中间有一段数据丢了」；7z → 「这不是压缩包」 |
| 对照 | 8 个 zip 样本 + 2 个 7z 样本，7z/unrar 全都能解 |
| 我们的落点 | `src/error.rs:229` `UNSUPPORTED_METHOD` **全库零引用（死代码）**；正常解压路径落 `DataLost` 兜底 |
| 参考实现 | 我们自己的 `src/rescue.rs:615` 已经有文案「救援模式暂不支持压缩方法 {other}」—— **救援路径有、正常路径没有** |
| 判定 | **A**（钩子留好了没接线）|
| 修法 | 方案一（推荐，半天）：方法不在支持列表 → 抛 `UNSUPPORTED_METHOD`，文案「这个包用了 X 压缩，本程序不支持，用 7-Zip / PeaZip 打开」。方案二（几天）：真支持 BZip2/LZMA/XZ |
| 成本 / 风险 | 半天 / 低 |
| **状态** | ✅ **已修 2026-11-02** —— 新增 `ErrorCode::UnsupportedMethod`（`UNSUPPORTED_METHOD`）走通全链路：`error.rs` 枚举 + `as_str` + 两个退出码 + `main.rs` 文案映射 + `locales/*.toml` 7 语言（155 键对齐）+ `gui/assets/l10n/*.json` 7 语言（552 键对齐）。识别点：`extract.rs` 的 `条目打不开说明()` 认 zip crate 的 `CompressionMethodNotSupported(u16)`；`extract7z.rs` 认 sevenz 的 `UnsupportedCompressionMethod` / `Unsupported` / `ExternalUnsupported`；`job.rs` 把「不支持的压缩方法」从 `DataLost` 摘出。实测：zip 侧 7 个样本（lzma/ppmd/xz/implode/reduce/shrink/shrink_full_table）+ 7z 侧 2 个（zstdmt-brotli/lz4）全部改口；`solid.7z` 等正常包不受影响。回归测试 `job::tests::不支持的压缩方法报专门错误码`（先红后绿，红时报的正是 `compression method not supported: 6`）。`cargo test` 433 绿 + clippy 干净 + `flutter test` 420 绿 |

### A-3 · 好包被 `--test` 判成「这不是压缩包」（假阴性）

| 项 | 内容 |
|---|---|
| 触发 | `extended_timestamp_bad.zip`（坏 extra field，297 B）|
| 我们的表现 | `--list` 正常列 2 项；`--test` → 「这不是压缩包」；7z → 2 files ✅ |
| 我们的落点 | `src/api.rs:352` verify_only → `src/batch.rs:165` 的校验路径比 list 的解析路径严格，extra field 坏就判 `NotArchive` |
| 参考实现 | `refs/zip2/tests/zip_extended_timestamp.rs` —— 坏字段忽略、其余照常 |
| 判定 | **A**（两条路径结论不一致）|
| 修法 | 「extra field 解析失败」从 `NotArchive` 降级为警告 + 继续；只有连条目都读不出才判不是包 |
| 成本 / 风险 | 半天 / 低 |
| **状态** | ✅ **已修 2026-11-02** —— `src/repair.rs` 新增「坏 extra 链」检测（`坏extra区间` / `extra链坏了` / `空字段填充`）：EOCD 自洽时再扫 CD 条目，坏字段整块置空（长度不变不错位）后自动重试。回归测试 `extract::tests::坏extra字段的包也能解`（先红后绿），样本已收进 `tests/data/extended_timestamp_bad.zip`。验证：`cargo test` 433 全绿（432 passed / 1 ignored）+ `clippy -D warnings` 干净 + 真 CLI `--test` 与真解压双验（2 文件落盘，mimetype 39 B 内容正确）|

### A-4 · 救援路径被文件名里的假 ZIP64 签名骗出假条目

| 项 | 内容 |
|---|---|
| 触发 | `zip64_magic_in_filename_1~5.zip`、`deflate64_issue_25.zip`（共 6 个）|
| 我们的表现 | `--list` → 「中央目录坏了，已按文件头重建清单（1~3 项）」；`--test` → 「这不是压缩包 / 文件不完整」→ **两条命令互相打脸**；`_5` 我们 3 项 vs 7z 6 项 |
| 我们的落点 | `src/api.rs:1187` → `src/rescue.rs:314` `rescue_by_locals`（按 local header 扫描，不校验名字里的签名是否真记录）|
| 参考实现 | `refs/zip2/src/read/zip_archive.rs:969-986` `zip64_magic_in_filenames()`，判据原文：**"we still read them when they aren't genuinely ambiguous"** —— 不是看到签名就切，只有真歧义才拒 |
| 判定 | **A** |
| 修法 | 名字范围必须在 `PK0304` 结构边界内；遇 `PK0606/PK0607` 回溯最近合法 local header 并验 size/CD 自洽；不自洽整条丢弃（宁可少列，不许谎报）|
| 成本 / 风险 | 1 天 / 低（只动救援路径）|
| **状态** | ✅ **已修 2026-11-02（6 个样本修好 5 个）** —— 挖下去发现是**三层**问题叠在一起：<br>① `layout/zip.rs` 的 ZIP64 判据：以前「EOCD 前 20 字节开头是 `PK\x06\x07` 就当 ZIP64」，而文件名里正好塞了这串字节 → 去找不存在的 ZIP64 EOCD → 「这不是压缩包」。改成**只有 EOCD 字段真是哨兵值（0xFFFFFFFF）才必须走 ZIP64**；只是「看着像定位器」时先试着读，读不到就按普通 EOCD 处理（对齐 zip2 `zip64_magic_in_filenames()` 的「只有真装不下才需要」）。<br>② `rescue.rs` 的救援扫描：以前会把数据区里碰巧的 `PK\x03\x04` 当条目，名字整段是控制字节也照列（`--list` 列 1~3 项垃圾、`--test` 说不是包 → 两条命令打脸）。现在**名字含控制字符一律不认**（宁可少列）。<br>③ 报告层：这些包真正的结局是「条目名不安全 → 我们的安全闸拒绝写盘」，但以前报 `DataLost`（「中间有一段数据丢了」）。新增 `ErrorCode::UnsafePath`（`UNSAFE_PATH`，7 语言 locales 156 键 + GUI 552→553 键），文案「包里有不安全的名字 —— 已拒绝写出来，这是安全保护，不是包坏了」。<br>另加 `extract_zip_at_ctx` 兜底：zip crate 打不开、且修复流程也救不了时才走救援提取（**先让修复上**，否则 A-3 那种包会少解条目 —— 实测踩到，2 个文件只解出 1 个）。<br>回归测试 `job::tests::控制字符文件名报不安全路径而不是数据丢了`，样本 `tests/data/zip64_magic_in_filename.zip`（168 B）。`cargo test` 435 绿 + clippy 干净。<br>**剩 1 个**：`deflate64_issue_25.zip` 是 locate 阶段就失败的样本，另记 A-9 |

### A-9 · 定位阶段就失败，但文件里明明有 local header（能救却没救）

| 项 | 内容 |
|---|---|
| 触发 | `deflate64_issue_25.zip`（94 B，zip2 复现样本）|
| 我们的表现 | 「这不是压缩包（没找到可识别的压缩数据）」；7-Zip 说 `1 file, 94 bytes` |
| 结构 | 开头就是合法 local header（`PK\x03\x04`，method=9 deflate64）；但文件里有**两个假 EOCD**（偏移 42 和 66），字段全是垃圾（n_total=49135、cd_off=2 GB），**没有任何中央目录** |
| 根因 | `locate` 失败就直接判死。`--list` 有 `try_list_by_rescue` 兜底，`--test` / 解压路径没有 |
| 修法 | `job.rs` 的定位失败分支加与 `--list` 同款的救援兜底：按 local header 能重建出条目就走救援，重建不出才说不是包 |
| 成本 / 风险 | 半天 / 低 |
| **发现方式** | 2026-11-02 修 A-4 时从 6 个样本里剩下这一个，按硬规矩记账不动手 |
| **状态** | ✅ **已修 2026-11-02** —— `locate_zip` 的定位失败分支改成**先试救援**：`rescue_by_locals` 真能重建出条目就返回 `Target::Zip`，交给解压链（链上 A-4 的兜底接着走救援提取）。判据是硬的：重建不出条目就往下走原来的 Truncated / NotArchive 报错，所以「只有个 PK 签名 + 几十字节垃圾」的伪装包不会被误救。<br>自己造了 `tests/data/D9_no_central_directory.zip`（79 B：1 个 local header + 数据，无 CD 无 EOCD）验证：改前「这不是压缩包」，改后 `--test` 校验通过、真解压出 `hello.txt` 40 B 内容完全正确。回归测试 `job::tests::中央目录被删掉也能按文件头救出来`。<br>**注**：zip2 的 `deflate64_issue_25.zip` 救援重建不出（它连名字字段都是垃圾字节、csize 写着 4 GB），仍报「这不是压缩包」—— 但 `--list` 与 `--test` 现在结论**一致**了，A-4 要解决的「两条命令打架」已达成 |

### A-5 · zip 分卷（`.zip + .z01`）拼不起来

| 项 | 内容 |
|---|---|
| 触发 | `C12_split.z01`(307,200 B) + `C12_split.zip`(103,588 B)，5 文件分卷包 |
| 我们的表现 | `--list .zip` → 重建出 1 项；`--test` → 「这不是压缩包」；7z → `Volumes: 2`、5 files ✅ |
| 我们自己的诊断 | `reason=base_is_not_local_header base=307200`（数是对的，规则用错了）|
| 我们的落点 | `src/layout/zip.rs:390`（校验要求 base 处必须是 `PK0304`，但分卷续块开头是数据延续）|
| 参考实现 | 我们 `src/unpack.rs:27` 自己抄的 7-Zip 设计注释：「分卷由 handler 反向索取（`IArchiveOpenVolumeCallback`），opener 不必懂卷名规则」|
| 判定 | **A** |
| 修法 | 分卷流（`Shape::Volumes`）跳过「base 处必须 PK0304」，改判「`cd_at − cd_offset` 落在卷边界上」|
| 成本 / 风险 | 1 天 / 中（动 CD 定位，必须补样本回归）|
| 附带 | `tests/` 里**零断言**用我们引擎验过 C12（`make_samples.py:1011` 只拿 `7z t` 验）→ 假绿温床，修时必须补 |

### A-6 · 声明尺寸不按文件实际大小封顶

| 项 | 内容 |
|---|---|
| 触发 | `raw_deflate64_index_out_of_bounds.zip`(724 B → 报 1.17 GB)、`deflate64_issue_25.zip`(94 B → 报 3.75 GB) |
| 我们的落点 | 预览/清单阶段信 CD 的 uncompressed_size；`src/preview.rs:130-132` 只有「entries 为空且不完整」才提示残包，管不到这里 |
| 参考实现 | `refs/zip2/tests/declared_size_enforced.rs`、`legacy_oversized_declared_size.rs`；libarchive `entry_bytes_declared`（`docs/REFERENCE-INDEX` §10.2）|
| 判定 | **A** |
| 修法 | 声明解压大小 > 压缩后 × 1000:1（沿用现有压缩比闸）→ 标「声明的尺寸异常，可能是坏包」，不直接把 GB 摆给用户 |
| 成本 / 风险 | 半天 / 低 |

### A-7 · 部分损坏包的清单列不全（小账）

`D4_corrupt_member.zip`（实际 5 项，list 只列 2）· `E4_zip_middle_overwritten.mp4`（实际 6 项，list 只列 2）。
`--test` 报得对（「解开 4/5」「5/6」）。建议 list 时把「读不出元数据但 header 在」的条目也列出来标「元数据损坏」。**低优先**。

### A-8 · `--test --rescue` 会真落盘，违反「只测不写」

| 项 | 内容 |
|---|---|
| 触发 | `yshjy --test 某个包 --rescue` |
| 现象 | 在**当前工作目录**建 `<包名>.rescue/` 并把文件真解出来（实测 39 B 的包也照样建目录、写出 mimetype）|
| 违反 | `--test` 的文案承诺「一个字节都不落盘」 |
| 风险 | ① 用户以为只是检查，磁盘上却多了东西 ② 对**压缩炸弹**开 `--rescue --test` 会真写出，安全预期被打破 |
| 落点 | 救援分支没看 `verify_only`（`src/api.rs` 的 rescue 入口 → `src/rescue.rs`）|
| 修法 | rescue 路径尊重 `ctx.verify_only`：只统计条目和字节数，不写盘；输出照常给「能救回 N 个」 |
| 成本 / 风险 | 半天 / 低 |
| **发现方式** | 2026-11-02 修 A-3 时顺手撞见（项目根目录冒出一个 `.rescue/` 目录）—— 典型的「顺手发现」，按硬规矩先记账不动手 |
| **状态** | ✅ **已修 2026-11-02** —— 两处一起补：① `main.rs` 的 `do_rescue` 加 `verify_only` 参数并 `.with_verify_only(...)`（以前根本没传）② `rescue.rs:428` 的 `create_dir_all` 加 `if !ctx.verify_only`（这条是漏的，`PartFile::create_with` 早就支持 verify）。新增回归探针 `tests/rescue_verify.sh`（已挂进 `tests/all.sh`），**做过反向验证**：临时把 `if !ctx.verify_only` 改成 `if true` 重新编译 → 探针立刻报「✗ --test --rescue 写盘了：extended_timestamp_bad.zip.rescue」，改回来才绿。实测：`--test --rescue` 目录保持空 + 提示「🔍 只测不写」；不带 `--test` 的 `--rescue` 照常落盘（功能没修坏）|

---

## 二、B 类 · 我们更好（不换，必须在代码里写注释）

| # | 我们的行为 | 参考实现做不到 | 落点 | 为什么要注释 |
|---|---|---|---|---|
| B-1 | `zip64_demo.zip`：包在偏移 14（前置 `Leading junk.\n`），能定位并校验通过 | 7z：`Cannot open the file as [zip] archive` | `src/scan.rs` 全文件签名扫描 | 下轮审计会误判成「怎么垃圾文件也认」 |
| B-2 | `D2b_prefixed_2355200.zip`：偏移 2,355,192 定位成功 | 7z：打不开 | 同上 | 同上 |
| B-3 | `tar.gz/.xz/.zst/.bz2` 列 **5 个文件**（穿透到 tar） | 7z 只认 gzip 层 = 1 files | `src/unpack.rs` 分层 | 用户要的是「里面有几个」，别被"和 7z 对齐"改回去 |
| B-4 | RAR **中文密码**（UTF-8→GBK→UTF-16LE 三候选）`rar5_cnpwd.rar` 解开 | XADMaster `XADRAR5Parser.m:734` 只用 `NSUTF8StringEncoding` | `src/extract_rar.rs:309-311` | 中文 WinRAR 写 CP936，只试 UTF-8 必然报密码错 |
| B-5 | RAR **缺卷自查**：只有 `split_after` 位才说明后面还有卷，宁可漏报不乱报 | —（我们自研，实测无假警报） | `src/extract_rar.rs:167` | 乱报会把完好包拒收，比漏报更糟 |
| B-6 | 先 `test` 整包验密码再解（rars 解密失败不报错） | XADMaster 用 PBKDF2 passcheck 校验（更省时间，但要自己实现 PBKDF2） | `src/extract_rar.rs:174-192` | 这是有意的取舍：多解一遍换实现简单 |

---

## 三、C 类 · 参考也没有 / 验不了（标风险）

| # | 项 | 现状 | 风险 |
|---|---|---|---|
| C-1 | `RAR5ChecksumsAreEncrypted`（校验和本身也加密） | 未特殊处理；`rar` 命令行造不出这种包，**验不了** | 中（遇到会报 CRC 错）|
| C-2 | zip 里 `Overlapping files` / `absolute_paths` 等安全类边界 | 有 5 个样本在 zip2 里，本轮未单独验（P1 的 39 个里没有这两个） | 低（我们有 `src/safety.rs` + 路径安全检查）|

---

## 四、D 类 · 参考的解法是历史包袱（不抄）

| # | 参考的做法 | 为什么不抄 |
|---|---|---|
| D-1 | XADMaster 密码只用 UTF-8 | 对中文用户必然失败，我们已经超越了（见 B-4）|
| D-2 | 7-Zip 对 tar.gz 只列 1 项 | 那是它的口径，不符合「用户想知道里面几个文件」 |

---

## 五、死代码 / 未接线清单

`362` 个 pub 定义里 **21 个全库零引用**；已核实 7 个：

| 名字 | 位置 | 判定 |
|---|---|---|
| `UNSUPPORTED_METHOD` | `src/error.rs:229` | 🔴 死（见 A-2）|
| `rescue_broken_index` | `src/repair.rs:195` | 🔴 死：已被 `src/extract.rs:792-809` 内联流程取代，旧的没删 |
| `try_remux_with_ffmpeg` | `src/cover_extract.rs` | 🔴 死：装了 ffmpeg 能修的封面 moov，零调用 |
| `can_verify_video` | `src/tools.rs` | 🟡 死（能力探测，UI 没用）|
| `MAX_SIZE` | `src/extract_rar.rs` | ✅ `#[deprecated]` 兼容名，正常 |
| `build_sfx` | `src/pack/mod.rs` | ✅ 薄包装，真功能有 3 处调用 |
| `allows_sampling_scan` | `src/hardware.rs` | ⬜ 待核 |

未核的 14 个：`bits` · `cover_path` · `elapsed_millis` · `extract_7z` · `is_open` · `target_path` ·
`lang_name` · `long_flags` · `patch_count` · `set_done_bytes` · `sniff_head` · `unix_to_civil_` ·
`wait_until_written` · `with_progress`

> **注意事项**：`pub` 在 Rust 不触发 `dead_code`，所以 `clippy -D warnings` 全绿**挡不住这类问题**。
> 建议后面做成 `scripts/dead_pub.sh` 常驻检查。

---

## 六、P8 施工顺序（等用户点头才开工）

| 序 | 条目 | 成本 | 为什么排这儿 |
|---|---|---|---|
| 1 | ~~**A-3** `--test` 假阴性~~ ✅ **已修 2026-11-02** | — | 已完成（见 A-3 状态行）|
| 2 | ~~**A-2** 不支持压缩方法~~ ✅ **已修 2026-11-02** | — | 已完成（见 A-2 状态行）|
| 3 | ~~**A-8** `--test --rescue` 会落盘~~ ✅ **已修 2026-11-02** | — | 已完成（见 A-8 状态行）|
| 4 | ~~**A-1** RAR5 头加密~~ ✅ **已修 2026-11-02** | — | 已完成（见 A-1 状态行）|
| 5 | ~~**A-4** 救援假签名~~ ✅ **已修 2026-11-02**（5/6 样本，剩 1 个记 A-9） | — | 已完成（见 A-4 状态行）|
| 5.5 | ~~**A-9** 定位失败时没试救援~~ ✅ **已修 2026-11-02** | — | 已完成（见 A-9 状态行）|
| 6 | **A-5** zip 分卷拼接 | 1 天 | 网盘分卷常见；要动 CD 定位 |
| 7 | **A-6** 声明尺寸封顶 | 半天 | 惊吓 |
| 8 | B 类 6 条写注释 + 死代码清理 + `dead_pub.sh` | 半天 | 防止未来被改坏 |

**进度：9 条完成 6 条（A-1 / A-2 / A-3 / A-4 / A-8 / A-9），剩 3 条（A-5 / A-6 + B 类）约 2 天。**

## 七、每条施工的纪律（不变）

1. 先把样本收进 `tests/data/`，写**能反向验证**的断言（改坏必须红）
2. 一次只改一条，`cargo test` + 相关探针全绿
3. B 类必须在代码里写「为什么没抄」的注释
4. 修完在那条后面标 ✅ + 日期 + 回归脚本名

---

_六批审计到此收口。结论一句话：**不是算法自研问题，是接线问题 —— 5 天能修完。**_
