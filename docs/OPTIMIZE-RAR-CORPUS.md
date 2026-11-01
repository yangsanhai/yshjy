# P4 · RAR 层审计报告

> 原计划：用 `refs/XADMaster` 的 RAR 测试当考题
> **现实（侦察结论）**：`refs/XADMaster/XADMasterTests/RAR/` **只有 `.m` 测试代码，零个 `.rar` 样本**；
> Keka 上游闭源。参考项目的 RAR 样本**跑不了**。
> 替代方案：本机有 `rar 7.x` + `unrar`，**自己造样本实测** + 读 XADMaster 源码对照。
> 生成脚本：`~/下载/yshjy-优化表/make_rar_samples.sh`（不碰项目仓库）
> 日期：2026-11-02

---

## 一、先看清一件事：我们的 RAR 不是自己写的

`Cargo.toml:46` → `rars = "0.11.0"`（Apache-2.0 纯 Rust 读取实现）

所以 **RAR 的解析算法、解码器我们一行没写**，全靠成熟库。我们自写的只有四块**编排逻辑**：

| # | 自写部分 | 位置 |
|---|---|---|
| 1 | 分卷续卷检测（`is_split_after`） | `src/extract_rar.rs:167` `还有后续卷()` |
| 2 | 密码候选（UTF-8 → GBK → UTF-16LE） | `src/extract_rar.rs:309-311` |
| 3 | 先 `test` 验密码再解（库里解密失败不报错） | `src/extract_rar.rs:174-192` |
| 4 | 时间戳换算（RAR4 DOS / RAR5 Unix）+ 限额 | `src/extract_rar.rs:321` 起 |

**这意味着 RAR 层的风险面比 zip 层小得多** —— 真正要审的就这四块。

---

## 二、实测：7 种自造样本

| # | 样本 | 我们 | 结论 |
|---|---|---|---|
| 1 | `rar5_plain.rar`（4 文件） | ✅ 校验通过 | 正常 |
| 2 | `rar5_pwd.rar`（`-pPASS123`） | ✅ 带码校验通过 | 正常 |
| 3 | `rar5_cnpwd.rar`（中文密码「密码中文」） | ✅ **校验通过** | **我们更强**（见第四节）|
| 4 | `rar5_split.part1/2/3.rar`（3 卷齐） | ✅ 校验通过 | 分卷拼接正常 |
| 5 | `split_missing/`（少了 part2） | ✅ 「分卷不齐 —— 后面还有卷没下回来」 | **断得很准** |
| 6 | `rar5_headenc.rar`（`-hp` 加密文件名） | ❌ **两条都错** | **真缺口（见第三节）** |
| 7 | `rar4_plain.rar` | — | 本机 `rar 7.x` 不支持 `-ma4`，造不出来；RAR4 读取靠 `tests/data/rar_mtime_sample.rar` 覆盖（P3 已验 ✅）|

---

## 三、🔴 真缺口：RAR5 头加密（`-hp` 加密文件名）完全不支持

`-hp` 就是 WinRAR 界面上那个「**加密文件名**」勾选框 —— **国内用户很常勾**。

| 场景 | 我们 | unrar |
|---|---|---|
| 不给密码 | ❌ 「**分卷不齐 —— 后面还有卷没下回来**」 | 提示要密码 |
| 给对密码 `PASS123` | ❌ 「**这不是压缩包（没找到可识别的压缩数据）**」 | ✅ `All OK`，4 个文件全过 |

**两条错误都指向同一个根因，而且证据是硬的：**

`src/extract_rar.rs:89`（单卷）和 `:125`（分卷）打开包时是：

```rust
let archive = rars::ArchiveReader::read_path(path)   // ← 没有 options，没有密码
```

而 `rars 0.11` 明确提供了带密码读头的入口：

```
~/.cargo/registry/.../rars-0.11.0/src/lib.rs:1403   pub fn read_path_with_options(...)
~/.cargo/registry/.../rars-0.11.0/src/lib.rs:261    pub fn with_password(password: &'a [u8]) -> Self
```

**库能做的事我们没调用** —— 这正是「参考实现有解法、我们另写了一套」的
最典型形态，只不过这里的"参考实现"是我们自己依赖的那个库。

而 rars 自己是有测试证明支持的：
`src/test_features.rs:81` → `tests/fixtures/rar50/header_encrypted.rar`（专门测头加密）

### 连带问题：错误分类错了（与 P1-A2 同一个病）

`rars` 返回 `Error::NeedPassword`（库里的原文：`a password is required`），
我们的映射把它变成了 `MISSING_VOLUME`（「分卷不齐」）——
用户看到这句话会去**重新下载分卷**，而实际上包是完整的、只是要密码。

### 修复方向（P8 才动手）

1. 打开阶段改用 `read_path_with_options` + `ArchiveReadOptions::with_password`
2. 打开失败且错误是 `NeedPassword` 时 → **拿密码候选重开一次**（复用已写好的三编码候选）
3. 错误映射改掉：`NeedPassword` → `NEED_PASSWORD`（「这个包加密了文件名，需要解压码」），
   绝不能落 `MISSING_VOLUME`
4. 回归样本：把 `rar5_headenc.rar` 收进 `tests/data/`，加一条「无码要密码 / 有码能解」的断言

---

## 四、🟡 我们比参考实现强的地方（B 类，写注释保住）

| 点 | 我们 | XADMaster / 对照 |
|---|---|---|
| **中文密码** | 候选 UTF-8 → GBK → UTF-16LE 逐个试，`rar5_cnpwd.rar`（密码：密码中文）**解开了** | XADMaster `XADRAR5Parser.m:734` 只用 `NSUTF8StringEncoding` —— 中文 WinRAR 写的 CP936 密码它试不出来 |
| **缺卷自查** | 「分卷不齐 —— 后面还有卷没下回来」（只有 `split_after` 位才说明后面还有） | 注释里写得很清楚「宁可漏报，绝不乱报」，实测确实没误报 |

---

## 五、对照 XADMaster 源码的其余结论

| XADMaster 的处理 | 我们 | 判定 |
|---|---|---|
| `XADRAR5Parser.m:241` `RAR5HeaderTypeEncryption` 读加密头 → 派生 key → 再解后续头 | 没做（见第三节） | **A 类（该补）** |
| `:728` `keysForPassword:` PBKDF2 + passwordCheck 校验 | 我们用「先 test 整包」代替（库不暴露 passcheck） | **B 类（可接受）**：代价是多解一遍，换来不用自己实现 PBKDF2 校验 |
| `:797` `RAR5ChecksumsAreEncrypted`（校验和本身也加密） | 未特殊处理 | **C 类（未知）**：需要造这种包才能验，`rar` 命令行造不出来；先记着 |
| `:101` `volumesForHandle:firstBytes:name:` 卷名识别 | 我们 `src/job.rs:692` 认 `.part1.rar/.r00/.z01/.001` | ✅ 覆盖更全（XADMaster 只认 RAR 自家卷名） |

---

## 六、结论

- **RAR 层只有 1 个真缺口**：头加密 `-hp` 不支持 + 错误分类错成「缺卷」。
- 这个缺口**很好补**（库现成支持，改调用 + 改错误映射），但影响面不小：
  「加密文件名」是 WinRAR 的一键勾选，隐私包基本都开着。
- 其余四块自写逻辑（分卷、密码候选、先验后解、时间戳）实测都对，
  其中**中文密码**和**缺卷自查**是我们比参考实现强的地方。
- 本报告证据全部来自实跑（自造 7 种样本 + `unrar` 对照），不是读代码猜的。

**P4 结论：✅ 通过（1 个 A 类缺口 + 1 个 B 类取舍 + 1 个 C 类未知）。**
