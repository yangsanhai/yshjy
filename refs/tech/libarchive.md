# libarchive 拆解：格式注册表与 bid 竞价（补拆 §10.2 第 3 项）

> 来源：`refs/外部-归档软件/libarchive/`（git clone `--depth 1`，31 MB，C 库，BSD-2-Clause）
> 读完范围：`libarchive/archive_read.c`（choose_format / bid_format / choose_filters）、
> `archive_read_private.h` 的 `struct archive_read`、`archive_read_support_format_all.c` 全文。
> 拆解日期：2026-10-28。
> **结论先行**：它是「加格式不改主流程」的教科书，但**我们已经用另一种方式实现了同一件事**
> （`src/scan.rs` 的候选队列 + 结构校验）。真正可抄的只有 2 条：**竞价时把最高分传下去剪枝**、
> **`entry_bytes_declared` 这个「声明大小」字段**（见 §5）。

---

## 1. 架构：两层链条，各自竞价

```
用户文件
  ↓
[filters 链]  ← 压缩层（gzip / xz / zstd / bzip2 …），可以叠多层（.tar.gz.xz）
  ↓
[formats 表]  ← 容器层（tar / zip / 7z / rar / iso …），只选一个
  ↓
archive_entry（统一的条目结构，文件大小/mtime/权限/路径全在这）
```

| 层 | 注册函数 | 数量（本仓） | 槽位上限 |
|---|---|---|---|
| 过滤器 filter | `archive_read_support_filter_*.c` | **15** 个 | `bidders[16]`（`archive_read_private.h:170`）|
| 格式 format | `archive_read_support_format_*.c` | 17 个文件，去掉 `_all` / `_by_code` 后 **15** 个真格式 | `formats[16]`（`archive_read_private.h:212`）|

> ⚠️ **硬上限 16，且是固定数组不是链表**。加第 17 个格式要改私有头文件重编译。
> 我们用的是 `Vec` / `&[..]`，没这个限制 —— 但要知道主流库也会做这种取舍（省一次堆分配）。

---

## 2. 竞价（bid）机制：`archive_read.c`

### 2.1 每个 format 有一个 `bid()` 函数，打分（`archive_read_private.h:198-211`）

```c
struct archive_format_descriptor {
	void	 *data;
	const char *name;
	int	(*bid)(struct archive_read *, int best_bid);   // ← 竞价：返回 0..8 分，或 ARCHIVE_FATAL
	int	(*options)(...);
	int	(*read_header)(...);
	int	(*read_data)(...);
	int	(*read_data_skip)(...);
	int64_t	(*seek_data)(...);
	int	(*cleanup)(...);
	int	(*format_capabilties)(...);
	int	(*has_encrypted_entries)(...);
}	formats[16];
```

### 2.2 `choose_format`：**把当前最高分传给下一个竞价者**（`archive_read.c`）

```c
choose_format(struct archive_read *a)
{
	slots = sizeof(a->formats) / sizeof(a->formats[0]);
	best_bid = -1;
	best_bid_slot = -1;
	a->format = &(a->formats[0]);
	for (i = 0; i < slots; i++, a->format++) {
		if (a->format->bid) {
			bid = bid_format(a, best_bid);        // ← 传 best_bid
			if (bid == ARCHIVE_FATAL) return (ARCHIVE_FATAL);
			if ((bid > best_bid) || (best_bid_slot < 0)) { best_bid = bid; best_bid_slot = i; }
		}
	}
	if (best_bid_slot < 0) { /* "No formats registered" */ return (ARCHIVE_FATAL); }
	if (best_bid < 1) { archive_set_error(..., "Unrecognized archive format"); return (ARCHIVE_FATAL); }
```

**两个设计点，都值得学**：

1. **`bid_format(a, best_bid)` 把「当前最高分」传进每个 bidder** —— 昂贵 bidder 一看
   「已经有 8 分的了，我最多也就能给 4 分」就直接返回 0 掉头走，**不做多余 I/O**。
   这是「先跑便宜的、贵的可以提前认输」的实现方式（`archive_read_support_format_all.c` 的
   注释里明说：`These bidders are all pretty cheap; ... If one of these bids high, we can maybe
   avoid running any of the more expensive bidders below.`）。
2. **`best_bid < 1` 才算「不认识」** —— 0 分是「我不认识」，`-1` 留给「程序错误」。
   **区分「不知道」和「出错了」**，这点我们 `src/scan.rs` 也做了（候选队列 vs 硬错误），
   方向一致。

### 2.3 `bid_format` 里那句 seek 复位（`archive_read.c`）

```c
bid_format(struct archive_read *a, int best_bid)
{
	bid = (a->format->bid)(a, best_bid);
	if (bid == ARCHIVE_FATAL) return (ARCHIVE_FATAL);
	if (a->filter->position != 0 && __archive_read_seek(a, 0, SEEK_SET) < 0)
		return (ARCHIVE_FATAL);
	return (bid);
}
```

**每个 bidder 跑完，位置必须回到 0**，否则下一个 bidder 从中间开始读就会误判。
我们 `src/scan.rs` 扫签名时是「只读 + 记录偏移」，天然没这个问题，**但这是同类系统的通病**，
记在这里：将来如果给某个格式加「预读一段判断」的优化，**必须保证不改动游标**。

### 2.4 上游自己留的 TODO：bidder 应该按「预读代价」排序（`archive_read_support_format_all.c:36-46`）

```c
/* TODO: It would be nice to compute the ordering here automatically so that people who enable just
 * a few formats can still get the benefits.  That may just require the format registration to include
 * a "maximum read-ahead" value (anything that uses seek would be essentially infinite read-ahead).
 * The core bid management can then sort the bidders before calling them. */
```

**人家（libarchive 主力）都还没做**的事。我们如果要优化扫描顺序，这条是现成的思路：
给每个格式声明「最大预读字节数」，按它排序竞价。**但别现在做** —— 我们的
`docs/TODO.md` 表尾第一条待查（`cargo test --release` 偶发失败）比这重要。

---

## 3. `archive_read_support_format_all`：一句话全注册

```c
int archive_read_support_format_all(struct archive *a) {
	archive_check_magic(...);
	archive_read_support_format_ar(a);
	archive_read_support_format_cpio(a);
	archive_read_support_format_empty(a);
	archive_read_support_format_lha(a);
	archive_read_support_format_mtree(a);
	...（后面还有 7z / cab / iso9660 / rar / rar5 / raw / tar / warc / xar / zip）
}
```

**顺序有讲究**（注释写明）：前面几个「便宜的」先注册；`read_ahead` 大的（tar 之类）放后面。
我们的 `src/scan.rs` 里候选顺序是**按签名扫描出来的偏移排的**，不是按格式类型 ——
因为我们主场景是「假视频里的真包」，偏移本身就是最强线索，这个差异是对的，别改成按格式排。

---

## 4. `entry_bytes_declared`：一个我们该抄的字段（`archive_read_private.h:184-192`）

```c
/* Declared uncompressed size of the entry whose header was most recently read, or -1 if no size
 * was declared. Lets format-agnostic consumers like archive_read_data_into_fd() cap their output
 * at the size an entry actually promised, regardless of whether the format reader enforces that
 * itself. */
int64_t	entry_bytes_declared;
```

**这个字段解决的是「炸弹」问题**：格式自己的读取器可能不检查条目实际写了多少字节，
于是上层可以拿「头部声明的大小」给输出**封顶** —— 多写的丢弃、少写的按截断报错。

对照我们：`src/safety.rs` 有炸弹限额（改成询问），`docs/REFERENCE-INDEX.md:38` 还挂着一条
「**条目 packSize 校验**（7-Zip 的 dataAfterEnd 判定）❌ 第22批」。**libarchive 这个字段
就是同一个需求在 C 世界的解法**：不是拒绝解压，而是「按声明大小封顶 + 超出即停」。
做那条待办时可以直接照这个语义实现（我们已经有 `Wrote`/`TotalHint` 事件，加一个
`declared` 字段就行）。

---

## 5. 可抄 / 不抄

| # | 内容 | 判定 |
|---|---|---|
| 1 | 竞价时传 `best_bid` 做提前剪枝 | 🟡 记住思路；扫描顺序优化是**将来**的事 |
| 2 | `entry_bytes_declared`（声明大小封顶） | 🔴 **做「条目 packSize 校验」时照抄语义** |
| 3 | bidder 跑完必须复位读位置 | 🟡 记档；我们现在没有预读，将来有时要守 |
| 4 | `bidders[16]` 固定数组 | ❌ 不抄（我们用动态容器） |
| 5 | filter 链可叠多层（.tar.gz.xz） | ✅ 我们已支持（`src/unpack.rs` 的多段虚接是另一回事，别混） |
| 6 | `archive_read_support_format_all` 一句话全注册 | ❌ 我们按特征自动识别，不需要用户声明 |

---

## 6. 与我们的类比表（给以后看文档的人）

| libarchive | yshjy |
|---|---|
| `bid()` 打分 | `src/scan.rs` 的候选 + `src/rescue.rs` 的结构校验 |
| `format->read_header/read_data` | 各 `src/extract*.rs` 实现 |
| `archive_entry` | 我们的 `Entry` 结构 |
| `entry_bytes_declared` | ❌ 还没有（对应 TODO「条目 packSize 校验」） |
| `archive_read_data_into_fd` | `src/extract.rs` 的落盘路径 |
| filter 链 | 我们一层层解（zip→inner 7z 走 `drill`） |
