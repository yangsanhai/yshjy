# 参考样本对照审计 —— 优化表

> 数据来源：`refs/zip2/tests/data`（zip2 参考项目 39 个样本，含它 91 个测试用的全部 zip 素材）
> 方法：三方对照 —— 我们（`target/release/yshjy 0.9.52`）× Info-ZIP `unzip` × `7z`
> 安全边界：全程只用 `--list` 和 `--test`（都不落盘），没有写过任何一个字节
> 生成：2026-11-02 · 原始数据 `~/下载/yshjy-优化表/对照表.csv`

---

## 一、总览

| 判定 | 样本数 | 说明 |
|---|---|---|
| ✅ 与 7z 完全一致 | **14** | 条目数逐一对上 |
| 🔑 需要密码 | 2 | `aes_archive` / `ignore_encryption_flag`，list 正常列出条目，行为正确 |
| ⚪ 三方都不认 | 7 | 真畸形包（`invalid_offset` 等），我们给了 `--rescue` 建议 |
| 🟡 我们比 7z 强 | 1 | `zip64_demo.zip` |
| 🔴 真问题 | **15** | 归成 4 类根因（下表 A1/A2/A3/A4） |

**结论：引擎对畸形包的容错底子不错（14 个逐条对上），但挖出 4 类真问题 —— 其中三类是「谎报 / 误判」性质，比解不开更伤信任。**

（15 个样本 = A1 六个 + A2 八个 + A4 一个。`deflate64_issue_25` 算在 A1，`raw_deflate64` 算在 A2 ——
它们在 A3 里再次出现，是同一个样本的第二个症状，不重复计数。）

---

## A1 · zip 文件名里嵌 ZIP64 签名时，`--list` 谎报条目

**证据（6 个样本）**：`zip64_magic_in_filename_1~5.zip`、`deflate64_issue_25.zip`

| 样本 | 我们 `--list` | 我们 `--test` | 7z |
|---|---|---|---|
| _2 | 重建清单 **1 项** | ❌ 这不是压缩包 | 1 files（Headers Error）|
| _4 | 重建清单 **2 项** | ❌ 文件不完整 | 2 files（Headers Error）|
| _5 | 重建清单 **3 项** | ❌ 文件不完整 | **6 files** |
| deflate64_issue_25 | 重建清单 **1 项（3.75 GB！）** | ❌ 这不是压缩包 | Is not archive |

**同一个文件，`--list` 说有东西、`--test` 说解不开 —— 两条命令结论相反。**

**根因**：中央目录坏了时 `--list` 会自动降级到按 local header 重建清单
（`src/api.rs:1187` → `src/rescue.rs` 的 `rescue_by_locals`），但这条扫描路径
**没有校验「名字里出现的 `PK\x06\x06` / `PK\x06\x07` 到底是不是真的 ZIP64 记录」**，
被文件名里的假签名骗着切出了条目。`--test` 不走降级 → 结论相反。

**参考项目现成解法**：`refs/zip2/src/read/zip_archive.rs:969-986` `zip64_magic_in_filenames()`
它的判据写在注释里，抄这句话就够了：

> "we still read them **when they aren't genuinely ambiguous**"
> —— 不是"看到签名就切"，只有**真的歧义**才拒

**建议动作**：`rescue_by_locals` 扫 local header 时加三条自洽校验
1. 名字长度必须落在 `PK\x03\x04` 结构边界内，不许越界
2. 遇到 `PK\x06\x06`/`PK\x06\x07` 时回溯到最近的合法 `PK\x03\x04`，验证 size/CD 字段自洽
3. 不能自洽就**整条丢弃**（宁缺勿假 —— 谎报清单比报错更糟）

**成本**：1 天 ｜ **风险**：低（只动救援路径）｜ **落点**：`src/rescue.rs` + 5 个样本的回归测试

---

## A2 · zip 用了没实现的压缩方法 → 报成「中间有一段数据丢了」

**证据（8 个样本，全部 7z 能解）**

| 样本 | zip 里的压缩方法 | 我们报什么 |
|---|---|---|
| `legacy/implode.zip` | Implode (method 6) | ❌ 中间有一段数据丢了 |
| `legacy/reduce.zip` | Reduce (2–5) | ❌ 中间有一段数据丢了 |
| `legacy/shrink.zip` | Shrink (1) | ❌ 中间有一段数据丢了 |
| `legacy/shrink_full_table.zip` | Shrink (1) | ❌ 中间有一段数据丢了 |
| `lzma.zip` | LZMA (14) | ❌ 中间有一段数据丢了 |
| `ppmd.zip` | PPMd (98) | ❌ 中间有一段数据丢了 |
| `xz.zip` | XZ (95) | ❌ 中间有一段数据丢了 |
| `raw_deflate64_index_out_of_bounds.zip` | Deflate64-raw (9) | ❌ 中间有一段数据丢了 |

**这是「谎报」**：包完好、只是用了我们没实现的算法。用户看到这句话会去重新下载、换软件 ——
`src/error.rs:74` 的注释里已经写过一模一样的教训：

> "于是用户看到「中间有一段数据丢了」，跑去重新下载、换软件 —— 而包其实完好。"

**根因（铁证）**：`src/error.rs:229` 早就定义了

```rust
/// 用了我们不支持的压缩方法
pub const UNSUPPORTED_METHOD: u16 = 1 << 7;
```

但 **全库零引用**（`grep -rn UNSUPPORTED_METHOD src/` 只有定义那一行）——
**死代码**。跟 `docs/AUDIT-GAP.md` 点名的「`default_mode` 解析了零读取」、
「`keyboard.dart` 有映射零消费」是同一类病：**写的时候留了钩子，没接线。**

**两条路，建议先走方案一**

- **方案一（推荐 · 半天）只改诊断，不实现算法**
  method 不在支持列表时抛 `UNSUPPORTED_METHOD`，文案说人话：
  「这个包用了 LZMA / PPMd / Implode 压缩（本程序不支持），用 7-Zip 或 PeaZip 打开」，
  `humanize` 里配一条可执行建议。**别再报数据丢失。**

- **方案二（几天）真支持几个高频的**
  BZip2(12) / LZMA(14) / XZ(95) 收益最高（国外包常见）；PPMd(98)、deflate64(9)、
  implode/reduce/shrink 是老掉牙算法，收益极低。
  参考实现：`refs/zip2/src/…`（`lzma.rs` / `xz.rs` / `ppmd.rs` / `deflate64.rs` 四个独立模块）

**成本**：半天（方案一）｜ **风险**：低 ｜ **落点**：`src/extract.rs` 方法分派 + `src/error.rs` + `gui/lib/humanize.dart`

---

## A3 · 条目声明尺寸不按文件实际大小封顶（list 显示 1.17 GB）

**证据（2 个样本）**

`raw_deflate64_index_out_of_bounds.zip`，文件 **724 B**，`--list` 列出

```
1.  1.17 GB  <名字里有 \0 的垃圾>
合计 1.17 GB / 1 个文件
```

`deflate64_issue_25.zip`，文件 **94 B**，`--list` 列出

```
1.  3.75 GB
合计 3.75 GB（1 项，压缩后大小）
```

一个 724 字节的文件说有 1.17 GB、一个 94 字节的说有 3.75 GB ——
声明尺寸完全没跟「文件实际大小 + 压缩比上限」交叉校验。

**参考项目现成解法**
- `refs/zip2/tests/declared_size_enforced.rs`（按声明大小给输出封顶）
- `refs/zip2/tests/legacy_oversized_declared_size.rs`（老式虚报大尺寸）
- `docs/REFERENCE-INDEX.md` 里「条目 packSize 校验」标了 ✅（`src/rescue.rs` 29 处），
  但那是**救援路径**；`--list` 的预览路径没生效 → 又一处「做了但没全覆盖」

**建议动作**：preview/list 阶段加一条闸
`声明解压大小 > 压缩后大小 × 1000:1`（沿用现有压缩比闸默认值）→
标「声明的尺寸异常，可能是坏包」，不要直接把 1.17 GB 摆给用户看。

**成本**：半天 ｜ **风险**：低 ｜ **落点**：`src/preview.rs` + `src/layout/zip.rs`

---

## A4 · 好包被 `--test` 判成「这不是压缩包」（假阴性，比 A1 更影响日常）

**证据**：`extended_timestamp_bad.zip`（297 B，扩展时间戳字段故意写坏）

| 谁 | 结果 |
|---|---|
| 我们 `--list` | ✅ 正常列出 2 项（`mimetype` 39 B / `test.txt` 大小显示「—」）|
| 我们 `--test` | ❌ **这不是压缩包（没找到可识别的压缩数据）** |
| 7z | ✅ 2 files（Warnings: 1）|

**包是好的，7z 能解，我们 `--list` 也读对了 —— 只有 `--test` 说它根本不是压缩包。**

**为什么这条排在最前**：`--test`（界面上叫「只检查」）是用户主动用来「查这个包完不完整」的功能。
**它把好包判成坏包，用户就会去重新下载** —— 这跟 A2 的谎报是同一个性质，但触发面更广
（任何带坏 extra field 的包都会中）。

**根因方向**：`--test` 走的校验路径（`src/api.rs:352` verify_only → `src/batch.rs:165`）
比 `--list` 的解析路径更严格，extra field 解析失败就整包判 `NotArchive`；
而 `--list` 的解析路径能容忍「字段坏了但条目还在」。
**同一个包两条路径给了相反结论 —— 这是路径不一致，不是容错能力不够。**

**参考**：`refs/zip2/tests/zip_extended_timestamp.rs` —— 它的做法是「坏字段忽略、其余照常」，
不为一个坏 extra field 判死整个包。

**建议动作**：`--test` 的结构校验里，把「extra field 解析失败」从 `NotArchive` 降级为
「警告 + 继续校验其余部分」；只有在「连条目都读不出来」时才判不是包。
界面上把警告显示成「这个包有个字段不规范，但内容能正常解出来」——这才是用户要的答案。

**成本**：半天 ｜ **风险**：低（只放宽判定，不放松解码）｜ **落点**：`src/scan.rs` / `src/api.rs` verify_only 分支

---

## B1 · 0 条目的 zip 判成「这不是压缩包」（可选，最低优先）

`comment_garbage.zip` / `misaligned_comment.zip`：里面只有 EOCD + 注释、零条目。
我们：「这不是压缩包（没找到可识别的压缩数据）」；7z：「是 zip，0 files」。

影响极小（0 条目本来就没什么可解），但严格说是判定不准。
建议并入 A2 的话术一起改，不单独列活。

---

## C1 · 我们比 7z 强的地方（要写进文档，防止以后被「统一」掉）

`zip64_demo.zip` 文件头是：

```
4c 65 61 64 69 6e 67 20 6a 75 6e 6b 2e 0a | 50 4b 03 04 ...
"Leading junk.\n"                          ↑ PK 头在偏移 14
```

**7z 打不开，我们 `--list` 定位到偏移 14、`--test` 校验通过。**

这就是 `docs/REFERENCE-INDEX.md` §13 武器①「全文件签名扫描」的价值实证。
**建议**：在 `REFERENCE-INDEX` 或 `CHANGELOG` 里记一笔「这是有意能力，不是误判」——
否则下一轮审计会把它当成「怎么垃圾文件也认」而修掉。

---

## 二、建议动手顺序（按「伤信任程度 ÷ 成本」排）

| 序 | 项 | 成本 | 为什么排这个位置 |
|---|---|---|---|
| 1 | **A4 `--test` 假阴性** | 半天 | 好包判成坏包，用户直接去重下 —— 触发面最广 |
| 2 | **A2 错误分类** | 半天 | 谎报「数据丢失」最伤信任，而且常量早就写好了，只是没接线 |
| 3 | **A1 救援路径假签名** | 1 天 | `--list` 和 `--test` 两条命令互相打脸，还会报出 3.75 GB |
| 4 | **A3 声明尺寸封顶** | 半天 | 1.17 GB / 3.75 GB 摆给用户看，属于惊吓 |
| 5 | B1 话术 | 并入 2 | 顺手 |
| 6 | C1 写文档 | 10 分钟 | 防止未来被改坏 |

**共同纪律**（按项目铁律）：每条先把样本存进 `tests/data/` 并写一个**能反向验证**的断言
（改坏必须红），再动实现。

---

## 三、这次没覆盖的（下一批）

- `refs/sevenz-rust/tests` **30 个**（7z 层，含 `security_tests` / `regression_tests`）
- `refs/XADMaster/Tests` **22 个**（rar/其他容器层）
- 我们自己的 `tests/extreme_*` 已有 45 个探针，可与这批样本合并成一张回归矩阵

zip 层 39 个样本只是第一刀。7z / rar 层的「别人踩过的坑」还没开始喂。

---

_本表只诊断、不改代码。所有样本均为故意构造的畸形包，真用户撞上的概率不高；
但 **A2 的压缩方法问题是真会撞的**（lzma / xz / ppmd 的 zip 在国外下载包里很常见）。_
