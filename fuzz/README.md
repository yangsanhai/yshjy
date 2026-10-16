# yshjy 模糊测试（fuzz）

安全工具的必修课：**259 个单元测试全是"我想得到"的场景，fuzz 专门找想不到的**。

## 怎么跑

```bash
# 三个目标各跑 60 秒（首次要先编译，约 3 分钟）
for t in list_zip locate_zip detect_stream; do
  cargo +nightly fuzz run $t -- -max_total_time=60
done

# 想跑久一点（推荐：要认真体检就跑 10 分钟以上）
cargo +nightly fuzz run list_zip -- -max_total_time=600

# 复现某个 crash（artifacts 里的文件）
cargo +nightly fuzz run list_zip fuzz/artifacts/list_zip/crash-xxxxx
```

前置：nightly 工具链（`rustup toolchain install nightly`）+ `cargo install cargo-fuzz`。

## 三个目标分别盯什么

| 目标 | 走哪条路 | 为什么它值得 fuzz |
|---|---|---|
| `list_zip` | 统一接口的「列清单」（ZIP / 7z / RAR / 单流 / tar 全走一遍） | 纯读不写盘，却一次覆盖五个 handler 的头部解析 —— 性价比最高 |
| `locate_zip` | ZIP 定位（尾部窗口 → EOCD → ZIP64 → 反推基准偏移 → 结构自校验） | 全是偏移算术（相对/绝对换算、小端读整数、`checked_sub`），越界与下溢最容易出在这里；这也是我们"别人做不到"的核心能力 |
| `detect_stream` | 单流格式识别 + 按文件名的兜底判断 | 外部数据进门的第一道关，它错了后面全错 |

## 判据

**只有一条**：不 panic / 不越界 / 不无限循环。返回 `Err` 是**正常结果**
（畸形输入本来就该被拒绝）—— 我们不在 fuzz 里断言语义。

## 已经跑过的记录

**2026-10-17 首次体检（每个目标 60 秒）**：

| 目标 | 执行次数 | 覆盖率 | crash |
|---|---|---|---|
| `list_zip` | 769,482 | cov 485 / ft 837 | **0** |
| `locate_zip` | 8,334,881 | cov 138 / ft 186 | **0** |
| `detect_stream` | 18,770,829 | cov 94 / ft 95 | **0** |

合计约 **2787 万次**畸形输入，**零 crash**。

⚠️ **如实说明**：60 秒只能算「初步体检通过」，**不等于**「已证明没有内存安全问题」——
`locate_zip` 的覆盖率只有 138，说明它需要相当特定的结构才能进入深层分支（随机字节很难
凑出一个合法的 ZIP64 记录链）。要更严肃的结论，就按 `-max_total_time=600` 以上跑，
并保留 corpus 让覆盖率累积。

## 产物放哪

- `fuzz/corpus/<target>/` —— 语料（**本地留着能越跑越深**；本地生成的种子越多越好）
- `fuzz/artifacts/<target>/` —— crash 输入（**这是"案子"**，复现就靠它）

两者都**不入库**（见 `.gitignore`）。目标代码本身是源码，**要入库**。
