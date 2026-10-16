# SteganographierGUI「藏（hide）」实现逆向报告

- 目标文件：`/home/user/hermes/yshjy/refs/SteganographierGUI/Steganographier.py`（3847 行，**CRLF**，单文件含 GUI+CLI）
- 代码版本：git HEAD `c2c6eaa`（2026-10-11，`git log` 只有单个 boundary commit，历史不可用）
- 方法：静态阅读 + **AST 解析 + 实际执行**（exec 源码、打桩缺失依赖）产出真实字节，并用 `ffmpeg` 解码验证、`7z`/`zipfile`/`pyzipper` 提取验证
- 复现脚本：`/home/user/hermes/yshjy/refs/analysis/sg_hide_probe.py`（可重跑，输出见本报告「实测证据」）

---

## 一、概要

隐写的本质只有一句话：**把待隐藏的内容先压成一个 ZIP，再把这个 ZIP 原样塞进外壳 MP4 的 box 结构里**——普通模式塞成「文件末尾追加的一个合法 `free` 盒」，ZArchiver 模式塞成「紧跟 `ftyp` 之后的 `free` 盒」并同步改写 `stco/co64`。ZIP 是**未改动的标准 ZIP**，因此"改后缀为 .zip 直接解压"成立（靠解压器对 prepend 数据的偏移修正）。加密走 `pyzipper` 的 **AES-256 / AE-2 / 强制 STORED（不压缩）**，不加密走标准 `zipfile` + 随机压缩级别。两种模式之外，ZArchiver 模式还会往原子链里插**随机噪声块**（伪造的压缩包特征码 + `os.urandom` + 空 `mdat`）做哈希随机化。**最重要的发现：这段噪声的写入语句被过度缩进，落在了 `while` 原子遍历循环体内（AST 已确认），于是噪声被插到每个顶层原子之后（含 `mdat` 之前），而 `stco/co64` 只补偿了 `free` 原子的长度——实测该模式的产物视频已无法正常解码。**

---

## 二、字节级布局图

### 2.1 普通模式 `type_option_var == 'mp4'`（"WinRAR 兼容"，CLI 默认）

实测：外壳 `cover.mp4`（ftyp28 + free8 + mdat12884 + moov979 = 13899 B）、载荷 12840 B、无密码 → 产物 14955 B。

```
 offset 0                                                         13899
 +--------------------------------+--------+-----------+----------------------+
 |  外壳 MP4 原文（逐字节照抄）    | free   | padding   |  ZIP 原文            |
 |  ftyp | free | mdat | moov     | 8B hdr | 64..256 B |  PK..EOCD(+comment)  |
 +--------------------------------+--------+-----------+----------------------+
                                 13899    13907       14102              14955
                                  ↑                      ↑
                    free.size = 8+pad+zip_size   7z 报 "Offset = 14102"
                    (大端 4B + 'free'；载荷 >4GB 时改用 16B large-size 头)

 free 盒内部布局：  [00 00 04 20]['f''r''e''e'][195B os.urandom][853B ZIP]
                     └── 声明大小含自身 8B ──┘  └ 随机填充让原子不以 'PK' 开头，
                                                同时让每次产物的哈希都不同
```

- **绝对没有任何东西写在 ZIP 之后**：文件最后 40 字节实测就是 ZIP 注释的尾部（ASCII 十六进制串）。
- 外壳字节 100% 未改（`stco/co64` 不动）→ `ffmpeg -v error -i out.mp4 -f null -` **rc=0**，视频照常播放。
- `7z l out_plain.mp4` 输出 `Type = zip, Offset = 14102, Physical Size = 853`：解压器从尾部找 EOCD，再把"ZIP 起点 ≠ 文件起点"的差值补偿掉。

### 2.2 ZArchiver 模式 `type_option_var == 'mp4(zarchiver)'`

同一个外壳、同一载荷 → 实测产物 56826 B：

```
 0      28       910  918               14248        27132              42515     43494    56826
 +------+---------+---+-----------------+------------+------------------+---------+--------+
 | ftyp | free    |fre|  J1  13330 B    | mdat 12884 |  J2  15383 B     | moov979 | J3     |
 |  28B | 882B    | 8B| sig+rand×2+mdat |  (原样)    |  sig+rand×2+mdat | stco已改| 13332B |
 +------+---------+---+-----------------+------------+------------------+---------+--------+
        └ ZIP 874B，无 padding，原子载荷
          从第 36 字节起就是 'PK\x03\x04'                ↑ stco: 44 → 926 (+882)
                                                        真实 mdat 载荷在 14256，差 13330 = J1

 J（噪声块，每份 ≈10KB~20KB）:
   [随机挑选的 7~8B 压缩包特征码] [5~10KB urandom] [再一个随机特征码] [5~10KB urandom] [00 00 00 08 'mdat']

 噪声特征码池: RAR4, RAR5, 7Z, ZIP(PK\x03\x04), GZIP, BZIP2, XZ 共 7 种，每次随机两枚
```

**关键：J1/J2/J3 不是写在文件末尾，而是跟在每一个顶层原子后面**——因为写噪声的 13 行语句被缩进在 `while pos < len(remaining_data)` 的循环体里（见第六节"意外发现"）。于是 `mdat` 之前被插进了 J1，`moov` 之前被插进了 J2。

后果（实测）：
- `stco` 只加了插入的 `free` 长度 882（44 → 926），但 `mdat` 实际被 J1 推后了 13330 B → 解码器按 926 去读，读到的是 J1 的随机字节。
- 非 faststart 外壳（`ftyp/free/mdat/moov`）：`ffmpeg` 报 **`moov atom not found`**（噪声的随机 4 字节被当成 box size，链在第 2 个原子处就断了）。
- faststart 外壳（`ftyp/moov/free/mdat`）：`ffmpeg` 报 **`header damaged`**，rc=69（moov 能读到，但样本数据全错位）。
- 但**数据仍可取出**：`7z x out_z.mp4` → `Type = zip, Offset = 36, Everything is Ok`；本程序自己的 `extract_with_offset_correction` 也成功。

### 2.3 MKV 模式 `type_option_var == 'mkv'`（对照，非 MP4）

```
外壳 MP4 ──mkvmerge──> 输出 MKV
  mkvmerge -o 输出.mkv 外壳.mp4 --attach-file <临时zip> --attach-file <随机8KB临时文件>
```
ZIP 变成 MKV 附件（附件 1 = ZIP，附件 2 = 8KB 垃圾附件，使附件数为 2、降低可疑度）。取回时 `mkvinfo` 按 MIME 行反查附件名，再 `mkvextract attachments 1:out`。输入 >2GB 被直接拒绝（CLI 与 GUI 都拦）。

---

## 三、分步算法流程（伪代码）

```text
# ============ hide_file(input, cover_video, password, output_file, type, ...) ============
1. cover = choose_cover_video_file(...)          # 见 §5.3，决定用哪个外壳 MP4
2. tmp_zip = build_temp_archive_path(input)      # <input 的父目录>/<basename>_hidden_<N>.zip
                                                  #   刻意不落在输入目录内部（自包含膨胀 bug）
3. compress_files(tmp_zip, input, password)      # 见下方 "compress_files 细节"
4. branch type:

   # ---- A. type == 'mp4'（默认，WinRAR 兼容）----
   out = get_output_file_path(...)                       # _hidden_<N>.mp4 / _hidden_z<N>.mp4 / _hidden_<N>.mkv
   pad = random.randint(64, 256)                         # 含两端
   (free_hdr, _) = create_free_atom_header(pad + zip_size)
       # total = 8 + payload；total > 0xFFFFFFFF 时改为 b'\x00\x00\x00\x01free' + u64(total+8)
   copy cover.mp4  →  out           # 8MB 分块，逐字节，不做任何解析
   out.write(free_hdr);  out.write(os.urandom(pad));  copy tmp_zip → out
   # 结束。没有尾部噪声，没有尾标记，ZIP 的 EOCD 就是文件末尾

   # ---- B. type == 'mp4(zarchiver)' ----
   atoms = read_mp4_atoms(cover)      # 只需要找到 ftyp 的 offset/size
   hidden_data = tmp_zip 全文
   free_atom   = create_free_atom_header(len(hidden_data)) + hidden_data   # 无 padding
   delta       = len(free_atom)                    # 仅此一个补偿量
   out.write(cover[ftyp.offset : ftyp.offset+ftyp.size])   # 先写 ftyp
   out.write(free_atom)                                    # 再插 free（数据就在 ftyp 之后）
   rest = cover[ftyp 之后全部]
   pos = 0
   while pos < len(rest):                    # 按顶层 box 逐个走
       (size, atype) = parse_box_header(rest, pos)     # size==0 取到末尾；size==1 读 large size
       if size 非法(<=8 或越界) or 剩余不足: out.write(rest[pos:]); break
       box = rest[pos : pos+size]
       out.write(find_and_update_offsets_in_atom(box, atype, delta))   # 只改 stco/co64
       pos += size
       # ↓↓↓ 以下本应在循环外，实际在循环内 ↓↓↓
       head_signatures = {RAR4,RAR5,7Z,ZIP,GZIP,BZIP2,XZ}
       out.write(random.choice(head_signatures))          # 假特征码 #1
       out.write(os.urandom(1024 * random.randint(5,10))) # 5~10KB
       out.write(random.choice(head_signatures))          # 假特征码 #2
       out.write(os.urandom(1024 * random.randint(5,10))) # 5~10KB
       add_empty_mdat_box(out)                            # b"\x00\x00\x00\x08mdat"
   # 结束：文件以最后一个噪声块的空 mdat 收尾

   # ---- C. type == 'mkv' ----
   mkvmerge -o out.mkv cover.mp4 --attach-file tmp_zip --attach-file <urandom(8KB)>

5. finally: os.remove(tmp_zip)      # 清理失败只记日志，不掩盖真正的异常
```

```text
# ============ compress_files（写包细节，反指纹是重点）============
input = normpath(input)                       # 防 PowerShell 补全产生的尾部分隔符
sha256 = 文件/目录内容哈希（目录：文件名+内容，按 names 排序）
comment = f"SHA-256 Hash of '{basename}':\n{sha256}\nTimestamp '{now}'\nTimehash '{sha256(now)}'"

entries = 收集文件列表，并**排除临时 zip 自身**（否则一边读一边写自己 → 无限膨胀，实测 60MB/s）
random.shuffle(entries)                       # ① 条目顺序随机

if password:
    zf = pyzipper.AESZipFile(tmp, 'w', ZIP_DEFLATED, encryption=WZ_AES)
    zf.setpassword(password.encode('utf-8'))
    for e in entries: zf.write(e, arcname, compress_type=ZIP_STORED)   # ② 加密时只存储，不压缩
else:
    zf = zipfile.ZipFile(tmp, 'w')
    for e in entries:
        compresslevel = random.randint(1,9)   # ③ 压缩级别随机（ZIP_DEFLATED）
        date_time     = 随机 1980..2099 的合法时间元组   # ④ 每个条目时间戳随机
        zf.write(e, arcname, compress_type, compresslevel)

zf.comment = comment.encode('utf-8')          # ⑤ 注释固定写在 EOCD 前
# arcname：单文件 = basename；目录 = <目录名>/<相对路径>（保持顶层目录结构）
# 注意：写包时**不做** sanitize_path（原始名照写，含中文 → 自动置 UTF-8 标志位 0x800）
```

---

## 四、必须回答的 7 个问题（逐条答复）

### Q1 字节级布局：压缩包被写到哪里？顺序？

| 模式 | ZIP 落点 | 绝对顺序 | 尾部 |
|---|---|---|---|
| `mp4` | 外壳**之后**（追加） | `[外壳全文][free头8B][随机pad 64~256B][ZIP]` | ZIP 的 EOCD（文件即结束） |
| `mp4(zarchiver)` | **`ftyp` 之后**（插入，第 2 个 box） | `[ftyp][free(内含ZIP)][原第2个box][噪声J1][原第3个box][噪声J2]…` | 最后一份噪声的空 `mdat` |
| `mkv` | MKV 附件 1 | mkvmerge 决定 | 附件 2 = 8KB 随机 |

- 普通模式 ZIP 前面那 64~256 字节随机填充是**故意的**：让 `free` 原子内部不以 `PK` 开头（避免"文件里出现裸压缩包签名"这种扫描特征），并让同一输入的每次产物哈希都不同。
- 普通模式下"ZIP 位于外壳 box 链之后"这一事实不影响播放：播放器读到 `moov` 结束就停，后面多出来的 `free` 盒（声明大小合法）最多被静默跳过。

### Q2 普通模式 vs ZArchiver 模式的生成差异

| 维度 | `mp4`（普通） | `mp4(zarchiver)` |
|---|---|---|
| 解析外壳 | 完全不解析，整文件流式复制 | `read_mp4_atoms` 找 `ftyp` |
| 落点 | 文件末尾追加 | `ftyp` 之后插入 |
| ZIP 前置填充 | 64~256B 随机 | **无**（原子载荷第 1 字节就是 `PK`） |
| 是否改 atom | **不改** | 改 `stco/co64`（+插入长度） |
| 尾部噪声 | 无 | 每个顶层原子后一份 J（特征码×2 + urandom×2 + 空 mdat） |
| 取回方向 | 从尾部：EOCD → 中心目录 → prepend 偏移修正（7z/WinRAR/zipfile 均可） | 从头部：跳过 `ftyp` 后找第一个 `size>8 且载荷以 PK\x03\x04 开头` 的 `free` |
| 输出命名 | `xxx_hidden_<N>.mp4` | `xxx_hidden_z<N>.mp4` |
| 实测可播 | 是（ffmpeg rc=0） | **否**（moov atom not found / header damaged） |
| CLI 可选 | 是（`-t mp4`） | **否**（`-t` 的 choices 只有 `['mp4','mkv']`，只能从 GUI 选） |

### Q3 怎么改 MP4 atom？为什么？靠什么保证封面视频仍被识别？

`find_and_update_offsets_in_atom(atom_data, atom_type, delta)`（L3198）递归实现：

1. `atom_type == 'stco'`：从 `atom_data[12:16]` 读 `entry_count`，对 `16 + i*4` 的每条 32 位偏移 `+= delta`；若 `len < 16 + count*4` 则原样返回。
2. `atom_type == 'co64'`：同上，但每条 8 字节（`16 + i*8`）、`>Q`。
3. `atom_type ∈ {moov, trak, mdia, minf, stbl, udta}`：进 `update_container_atom_offsets`，从 `pos=8` 起遍历子 box（处理 `size==0` → 到容器末尾、`size==1` → large size），递归回到第 1 步。
4. 其它类型原样返回。

**为什么必须改**：`stco/co64` 是"chunk 在文件中的绝对偏移表"。ZArchiver 模式把 `free` 插在 `ftyp` 后面，它后面所有字节整体后移 `delta`，不改表的话播放器会照着旧偏移读到错误位置。**为什么改了还不够**：`delta` 只等于 `free` 原子长度，而噪声块（J1/J2…）同样在推后数据却完全没被计入，所以实测 `stco` 少补了 13330 B。

**保证封面视频仍被识别的真正机制在普通模式**：普通模式**一个字节都不动外壳**（没有任何 atom 改写），因此天然可播——这也解释了 README 里"生成的隐写视频用播放器播放时就是普通视频"只对普通模式成立。

改表的覆盖缺口（对我们是硬约束）：只认 `stco/co64`；不处理 `stsd/avcC`、`tfhd.base_data_offset`/`tfdt`（fMP4/CMAF）、`sidx`、`saio/saiz`；也不认 `mvex`。所以这套逻辑对**分片 MP4 会失效**。

### Q4 分卷（.001/.002）怎么处理？

**没有分卷逻辑**——全文件搜不到任何 `.001`/`volume`/`split` 相关代码。分卷完全是**用户侧手工行为**：
- GUI 一次可拖入多个文件/目录，`processed_files` 递增 → 每个输入生成**独立的一个产物**（`_hidden_1.mp4`、`_hidden_2.mp4`…），外壳视频也随之切换（`processed_files % len(covers)`）。
- README 明确把 `>4GB` 的建议写成"分文件夹或分卷处理"，程序只负责提醒（见 Q6）。
- mkv 模式对 >2GB 输入直接报错拒绝。

### Q5 密码怎么设？ZIP 加密？7z？AES？用 pyzipper 吗？

- **用 pyzipper，AES**。`compress_files`（L1962）有密码分支：
  ```python
  pyzipper.AESZipFile(tmp, 'w', compression=pyzipper.ZIP_DEFLATED, encryption=pyzipper.WZ_AES)
  zip_file.setpassword(password.encode('utf-8'))
  ```
- 实测产物中心目录 extra 字段：`01 99 07 00 02 00 'A''E' 03 00 00` → tag `0x9901`、len=7、version=`0x0002`（**AE-2**）、vendor=`AE`、strength=`0x03`（**AES-256**）、真实压缩方法=`0x0000`（**Stored，不压缩**）。`flag_bits = 0x1`（加密位，无 bit3）。
- 也就是说：**有密码 = AES-256 + 只存储**（代码里 `random.choice([pyzipper.ZIP_STORED])` 把压缩方式锁死为 STORED，注释还留着被注释掉的 `ZIP_DEFLATED`），所以加密产物体积 ≈ 明文大小。
- **无密码 = 标准 `zipfile` + `ZIP_DEFLATED` + 随机 compresslevel 1~9**（不是 ZIP_STORED）。
- 密码编码：写端 UTF-8；读端 `password.encode()`（默认 UTF-8）一致；`_extract_with_7zip` 另有中文路径的临时目录中转处理（L2471 附近注释）。
- **坑**：`Steganographier.load_passwords`（L2458）对密码做 `encode('ascii','ignore')` —— 密码本里的非 ASCII 密码会被**静默削掉**（`密码123` → `123`）。写端不削、读端密码本削，中文密码走密码本必然失配；只有 GUI 输入框/`-p` 直接传的密码才原样生效。
- 全程 7z **只在读端**出现（`_extract_with_7zip`，优先于一切方法）；写端从不用 7z 打包。
- **读端的隐藏硬依赖**：`extract_with_offset_correction`（zarchiver 模式读端）**只**用 `pyzipper.AESZipFile`（即便包没加密，走的是 `BytesIO` + AES 解析器），完全不走标准 `zipfile`。所以缺 `pyzipper` 时这条路必然失败，而且 `reveal_file` 的队列会**静默回退**到 `mp4_trailing`/`free_atom`，用户看到的现象是"解压成功"，只是用的方法不是你以为的那个（探针里实测复现过）。

### Q6 输出命名 / 说明书 / 封面来源 / 大小-时长校验

**输出命名**（`get_output_file_path` L2342；`output_option` 三选一）：

| output_option | mp4 模式 | zarchiver | mkv |
|---|---|---|---|
| `原文件名` | `<输入去扩展名>_hidden_<N+1>.mp4` | `…_hidden_z<N+1>.mp4` | `…_hidden_<N+1>.mkv` |
| `外壳文件名` | `<输入目录>/<外壳名>_<N+1>.mp4` | `<外壳名>_z<N+1>.mp4` | `<外壳名>_<N+1>.mkv` |
| `随机文件名` | `generate_random_filename(16)_<N+1>.mp4` | `…_z<N+1>.mp4` | `…_<N+1>.mkv` |

- 目录作为输入时不存在扩展名，直接用 `目录名_hidden_N`。
- 冲突处理（issue #37）：`_resolve_hide_conflict_path`（L1759）在 `auto_rename_hide_on_conflict` 开启时生成 `xxx (1).mp4`，并用 `_reserved_hide_outputs` 占位避免同批次撞号。
- CLI：未给 `-o` → `<输入目录>/<basename>_hidden.<type>`；给了 `-o` 但无扩展名 → 视为目录再拼上面这个规则（L3790-3805）。

**说明书/密码提示：不生成**。整份代码没有任何"输出 readme/说明/密码提示文件"的逻辑。存在的是：
- `modules/PW.txt` —— 用户维护的**密码本（读端字典）**，一行一个密码、可用 TAB 分隔（只取第一列），UTF-8-sig；CLI `-pf` 可换路径；`解TMD压` 的密码本格式被兼容（README L259）。
- `modules/steganographier_gui_comm.txt` / `_activate.txt` —— 右键菜单把文件路径塞给已有 GUI 实例的通信文件（500ms 轮询）。
- `logs/steganographier_<时间戳>.log`（UTF-8 BOM）—— 运行日志，CLI 也写。

**封面视频从哪来**（`choose_cover_video_file` L1843 / `initialize_cover_video_files` L1837）：
- 默认目录 `application_path/cover_video`（仓库里只放了一个 Rick Astley 10.8MB 视频，作者用来演示"看起来正常"）。GUI 可 `select_video_folder` 改目录并写进 `config.json`。
- 四种模式：①下拉框指定具体文件；②`随机选择模式` —— 把目录里所有 `.mp4` shuffle 后 `pop()`，选完再 `initialize` 重填（尽量不重复）；③`时长顺序模式` —— `get_cover_video_files_info(sort_by_duration=True)` 后按 `processed_files % len` 轮转；④`名称顺序模式` —— natsort 后按 `processed_files % len` 轮转。
- CLI 未指定 `-c`：依次找 `cover_video/第一个 mp4` → 程序目录第一个 mp4 → 输入所在目录第一个 mp4，都没有就退出（L3807-3827）。

**大小/时长校验**（GUI 拖入时，不阻断）：
- `get_video_duration`（L604）用 **hachoir** 解析外壳时长，失败返回 `None`（→ 0）；`get_cover_video_files_info`（L618）把时长/大小拼成下拉框文案（所以下拉项形如 `xxx.mp4 - 3m:00s - 10.37 MB`，`choose_cover_video_file` 里用 `rfind('.mp4')` 把后缀再切回来）。
- `check_size_and_duration`（L670）阈值表：≤200MB/1min、200-400MB/3min、400-500MB/15min、500MB-1GB/30min、1-3GB/60min、3-4GB/120min、>4GB/120min 以上，低于阈值即"不合理"。
- `check_file_size_and_duration`（L1178）+ `get_warning_text`（L1185）：弹一次提示框（`check_file_size_and_duration_warned` 只提醒一次/进程），文案附建议值表，明说"仅为提醒，并非报错"。
- 硬拦截只有一条：mkv 模式输入 >2GB。

### Q7 其它机制 → 见第六节"意外发现"

---

## 五、关键代码位置表

| 位置 | 符号 | 一句话作用 |
|---|---|---|
| `Steganographier.py:1594` | `class Steganographier` | 全部隐写/解隐写逻辑所在类 |
| `:1596` | `Steganographier.__init__` | 工具路径（mkvmerge/7z.exe）、密码本 `modules/PW.txt`、`cover_video` 默认目录 |
| `:1843` | `choose_cover_video_file` | 4 种外壳视频选择策略，返回外壳绝对路径 |
| `:1837` | `initialize_cover_video_files` | 随机模式的候选池 shuffle/重填 |
| `:1891` | `compress_files` | 打包核心：SHA-256 注释、条目 shuffle、随机日期/压缩级别、pyzipper AES 分支、排除自包含 |
| `:1962` | （在 compress_files 内） | **有密码 → pyzipper AESZipFile(WZ_AES) + ZIP_STORED；否则 zipfile + DEFLATED** |
| `:2058` | `hide_file` | 藏的主流程：选外壳 → 打临时 zip → 按 3 种 type 分支写产物 → finally 删临时 zip |
| `:2093` | `hide_file` / `mp4` 分支 | `[外壳][free头][随机pad][ZIP]`，可用 `output.write` 逐块，进度按 8+256 估上界 |
| `:2137` | 同上 | `padding_size = random.randint(64, 256)`；每份产物哈希不同 |
| `:2158` | `hide_file` / `mkv` 分支 | `mkvmerge -o out cover --attach-file zip --attach-file urandom(8KB)` |
| `:2201` | `hide_file` / `mp4(zarchiver)` 分支 | `ftyp` 后插 `free`，按原子遍历改写偏移，并写噪声块 |
| `:2264-2299` | 同上 | `while pos < len(remaining_data)` 顶层原子遍历（size==0/1、非法 size 的兜底） |
| `:2301-2323` | 同上（**缩进在 while 内**） | 特征码字典 + 两段 `os.urandom` + `add_empty_mdat_box` ← 布局错乱的根源 |
| `:2342` | `get_output_file_path` | 三种模式的输出命名 + 冲突改名 |
| `:2422` | `add_empty_mdat_box` | 写 8 字节 `00 00 00 08 'mdat'` 作为"文件结尾标记" |
| `:2427` | `create_free_atom_header` | `total=8+payload`；`>0xFFFFFFFF` 时改用 `1 + 'free' + u64(total+8)` 的 16B large-size 头 |
| `:2440` | `create_free_atom_with_data` | 头 + 数据（zarchiver 模式用，无 padding） |
| `:3151` | `read_mp4_atoms` | 顺序扫顶层原子（含 large size / size==0 到末尾），只需 `ftyp` 的 offset/size |
| `:3198` | `find_and_update_offsets_in_atom` | 递归派发：`stco`(32b) / `co64`(64b) / 容器 `moov trak mdia minf stbl udta` |
| `:3259` | `update_container_atom_offsets` | 容器内子原子遍历并递归调用上面那个 |
| `:3296` | `extract_from_free_atom` | 读端：找 `free 且 size>1024`，整段载荷写临时 .zip 交给 pyzipper/zipfile |
| `:3018` | `extract_with_offset_correction` | 读端（zarchiver）：跳过 `ftyp` 后找载荷**以 `PK\x03\x04` 开头**的 `free` |
| `:2658 / :2833 / :2925` | `_try_mp4_direct_extraction` / `_extract_with_zipfile` / `_extract_with_pyzipper` | 读端（普通模式）：直接把整个 MP4 当 ZIP 打开（吃 prepend 修正） |
| `:2502` | `reveal_file` | 读端主流程：7z 优先，再按扩展名/模式排队依次试各方法，快照对比决定是否删原件 |
| `:521` | `build_temp_archive_path` | 临时 zip = `<输入父目录>/<basename>_hidden_<N>.zip`（防自包含） |
| `:539` | `sanitize_path` | **仅读端**用：去控制字符/全角空格、替换 `<>:"\|?*`、处理 CON/PRN 等保留名 |
| `:670 / :1178 / :1185 / :604 / :618` | `check_size_and_duration` / `check_file_size_and_duration` / `get_warning_text` / `get_video_duration` / `get_cover_video_files_info` | 大小-时长合理性提醒链（hachoir 读时长） |
| `:1759` | `_resolve_hide_conflict_path` | 隐写输出重名自动改名 + 批次内占位 |
| `:2450` | `Steganographier.load_passwords` | 读密码本（**ASCII 过滤**，见 Q5 的坑） |
| `:3475 / :3790-3842` | `run_cli` / `__main__` CLI | `-t` 只允许 mp4/mkv；默认输出/外壳/密码本的兜底规则 |

---

## 六、实测证据（可复现）

环境：Debian，ffmpeg 9.0.1（lavfi testsrc 生成 3 秒 mpeg4 外壳 13899 B），7z，`pip install pyzipper`（0.4.0），源码以 `exec` 加载、只打桩 `pyzipper/hachoir/natsort/tkinterdnd2`（复现脚本见 `refs/analysis/sg_hide_probe.py`）。

| 实验 | 命令/断言 | 结果 |
|---|---|---|
| 普通模式布局 | 解析产物原子链/字节 | `13899 + 8 + 195 + 853 = 14955` ✓，PK 在 14102/14520，尾部为 ZIP 注释 |
| 普通模式可播 | `ffmpeg -v error -i out_plain.mp4 -f null -` | **rc=0，无错误输出** |
| 普通模式可解 | `7z l out_plain.mp4` | `Type = zip, Offset = 14102, Physical Size = 853` → 改后缀解压成立 |
| ZArchiver 布局 | 用外壳字节定位各原子 | `J1=13330, J2=15383, J3=13332`，合计 42045 = 56826−14781 ✓ |
| ZArchiver 偏移 | `stco` 原 `[44]` | 产物 `[926] = 44+882`，真实 mdat 载荷在 14256 → **差 13330（=J1）** |
| ZArchiver 可播 | 非 faststart / faststart 两种外壳 | `moov atom not found` / `header damaged`（rc=69）→ **不可播** |
| ZArchiver 可解 | `7z x out_z.mp4` | `Type = zip, Offset = 36, Everything is Ok` ✓ |
| 本程序回环 | 三种产物跑 `reveal_file` | 全部成功：普通→"MP4文件末尾ZIP提取"、zarchiver→"MP4 ZArchiver模式提取"、带密码→zipfile/pyzipper ✓ |
| 密码产物 | 中心目录 extra | `0199070002004145030000` → AE-2 / AES-256 / stored ✓；`7z x -p<正确密码>` → Everything is Ok |
| 中文文件名 | 条目 flag | `0x800`（UTF-8）✓，`cn/中文文件.txt`、`cn/sub/空格 文件名.bin` 正常 |
| 噪声位置 | AST dump `hide_file` | `Assign head_signatures`、两次 `output.write(random...)`、`add_empty_mdat_box` 均落在 `While` 节点 body 内 |
| 读端 pyzipper 硬依赖 | 在 `import pyzipper` 失败的解释器下跑探针（脚本打印 `[warn] pyzipper 未安装 → 打桩`） | 同一条 zarchiver 产物的回环仍报成功，但命中方法从 `MP4 ZArchiver模式提取` **静默**变成 `MP4文件末尾ZIP提取（WinRAR兼容）` |

复跑方式（两分钟出结果）：

```bash
python3 /home/user/hermes/yshjy/refs/analysis/sg_hide_probe.py            # 用临时工作目录
python3 /home/user/hermes/yshjy/refs/analysis/sg_hide_probe.py --work /tmp/sgprobe
python3 /home/user/hermes/yshjy/refs/analysis/sg_hide_probe.py --work /tmp/sgprobe --password 'P@ssw0rd'
```
> 探针会打印外壳/产物的原子链、7 种特征码命中位置、`stco` 前后值、`ffmpeg` 解码返回码、`7z` 的 `Type/Offset/Physical Size`，以及本程序读端的回环结果与方法命中顺序。

---

## 七、对我们的启示（写 Rust 版打包/解包时必须注意）

**A. 打包（write）侧**

1. **普通模式是"零解析追加"**：`copy(cover)` → `free(hdr+pad+zip)`。Rust 实现只需一次流式拷贝 + 一个 8B/16B 头，不需要任何 MP4 解析。目标文件必须与外壳**逐字节可复现**地前置，否则播放器行为不可预期。
2. **`free` 头的大小语义要精确**：标准形式 `size = 8 + payload`（含自身 8B）；`size == 1` 时必须改用 `1 + 'free' + u64(size_including_16B_header)`。解包端两种都要认。
3. **随机 pad 是兼容性关键**：普通模式必须保留 64~256B 随机前缀，(a) 让原子内不以 `PK` 开头，(b) 让产物哈希每次不同（作者明确写了"哈希随机化"，README 也承诺"同一个输入每次产物哈希不同"）。可选再加 `random.randint(0,3)` 个随机字节做尾填充。
4. **ZArchiver 模式请按"注释意图"实现，不要照抄现状**：噪声/尾标记应写在**原子循环之外、文件末尾**（`[ftyp][free(hidden)][其余原子原样][噪声×2 + 空mdat]`），并保持 `stco/co64 += len(free_atom)`——这样既保留"从头扫描可解"的特性，又不破坏播放。若照抄现状（噪声逐原子插入、`stco` 不补偿噪声），产出的 MP4 在 ffmpeg/VLC 系解码器上直接报废。
5. **改写偏移的覆盖面**：至少 `stco`（u32）与 `co64`（u64），递归容器 `moov/trak/mdia/minf/stbl/udta`；**更强**地还应该处理 `tfhd`（fMP4）、`sidx`、`saio`。对 fMP4/CMAF 外壳建议直接拒绝（Python 版会静默生成坏文件）。
6. **ZIP 侧**：条目顺序、mtime、压缩级别都随机化（反指纹/反去重）；EOCD 前写日志式注释（SHA-256 + 时间戳 + 时间戳哈希）；目录输入要保留顶层目录名；**打包时必须排除输出/临时压缩包自身**（Python 版这里炸过 7.25GB）。
7. **密码**：与 pyzipper 对齐就是 **AES-256 / AE-2 / method 0 (stored)** + `flag_bits |= 0x1`，文件名走 UTF-8 标志位 `0x800`；`zip` crate 原生不支持 AES，需要 `zip` + `aes`/`winzip-aes`（或 `rc-zip`、`sevenz-rust` 类库）实现，否则我们生成的"加密包"Windows 端打不开。不要复刻"密码本 ASCII 过滤"那个 bug。
8. **临时/中间文件**：放在输入目录**之外**，用后即删；(名字形如 `<basename>_hidden_<N>.zip`)

**B. 解包（read）侧 —— 我们的探测器应当覆盖的组合**

9. 判据优先级建议（对应 Python 版的方法队列）：
   1. **尾部 ZIP**：从文件尾找 EOCD → 中心目录 → 用 `concat`/prepend 修正偏移（等价 `zipfile`/7z/WinRAR 的"改后缀解压"）；这一条同时覆盖普通模式与"外壳本来就是 zip"的情况。
   2. **头部 `free` 原子**：`ftyp` 之后，`free` 且 `size > 8`；载荷以 `PK\x03\x04` 开头 → zarchiver 模式（无 pad）；载荷不以 `PK` 开头但内部 64~256B 后出现 `PK\x03\x04` → 普通模式的 free（Python 版此路径要求 `size > 1024`，小载荷会漏，我们不必照抄这个门槛）。
   3. **mkv 附件**：读 Attachments，附件 1 = ZIP（附件 2 是 8KB 垃圾）。
   4. 兜底：在文件尾 2MB 范围内广搜 `PK\x03\x04` / `7z` / `Rar!` 签名并尝试偏移修正解压。
10. **不要被噪声块骗到**：尾部会同时存在 **7 种伪造的压缩包特征码**（RAR4/RAR5/7z/ZIP/GZIP/BZIP2/XZ）各一到两枚，且后面跟着随机字节。我们必须用"签名 + 能否成功解析头/中心目录"双重判定，**绝不能只靠签名**。另外每份噪声后跟 8B 空 `mdat`，这也是个可用的形态特征（`00 00 00 08 'mdat'` 出现在文件中段/末尾）。
11. **容错解析**：噪声会让 Python 版的正规解析器在第 2 个原子处断链；我们的原子遍历遇到非法 size（`<8`、越界、随机 ASCII）时必须**放弃按 atom 走**并切到"签名扫描 + ZIP 结构自校验"策略，而不是直接失败。
12. **密码候选**：GUI 输入 > 密码本（TAB 第一列）> 空密码；AES 包必须用 AES 解压器重试，普通包用标准 ZIP；对中文密码用 UTF-8 编码字节（与写端一致），但要意识到 WinRAR 对非 ASCII 密码的行为受其本地编码设置影响，属于已知互操作风险。
13. **输出落盘**：解出的成员名要清洗（去控制字符、替换 Windows 非法字符、处理 `CON/PRN/COM1` 等保留名、限制长度），且**只有确实写出文件后才删原隐写文件**（Python 版用"前后快照对比"解决，值得借鉴）。

---

## 八、意外发现（隐藏用法 / 非显然机制）

1. **【最重要】噪声写入语句被过度缩进进 `while` 循环**（L2301-2323 缩进 32 空格，而循环体就是 32；该行注释甚至缩进到了 56 空格）。四个步骤的编号注释（1 写 ftyp、2 写 free、3 写剩余原子、4 加随机特征码）和"添加 MP4 文件的**结尾标记**"都表明原意是"循环结束后整体追加一次"。实际效果：**每个顶层原子后都追加一份噪声**，把 `mdat` 推到 `stco` 记录不到的位置 → 本模式产物视频不可播（ffmpeg 实测）。同时 `offset_adjustment` 只等于 `free` 原子长度，把噪声也忘了。
2. **同一份代码里存在两种"ZIP 前置垃圾"策略且互斥**：普通模式有 64~256B 随机 pad（→ 只能从尾部找 EOCD），zarchiver 模式零 pad（→ 只能从头扫 `PK`）。Python 的读端也用两条不同的代码路径分别处理，`extract_with_offset_correction` 那句 `free_data.startswith(b'PK\x03\x04')` 是硬判据。我们写探测器时必须两路都试。
3. **读端的 3 个隐含门槛**：`extract_from_free_atom` 要求 `free.size > 1024`（小载荷漏检）；`extract_with_offset_correction` 遇到 `size <= 8` 的原子会 `continue`（实际只前进 8 字节，把随机数据当原子解析）；`read_mp4_atoms` 遇到 `size==1` 时 `offset` 记为 `tell()-16`。这些都是我们实现时可以做得更稳的地方。
4. **`mp4(zarchiver)` 在 CLI 里根本选不到**：`-t/--type` 的 `choices=['mp4','mkv']`（L3740），只有 GUI 的下拉框能触发该模式；文档也只在 README 第 74 行一句带过。
5. **`modules/` 与 `tools/` 里有三个"配套武器"**：`hash_modifier.exe`（GUI 按钮 → `start_hash_modifier`，逻辑是"在文件尾贴 1 个随机字节改 MD5"，专门用于打炸链后的补档/传火）、`captcha_generator.exe`（GUI 按钮）、以及捆绑的 `7z.exe` / `mkvmerge.exe` / `mkvextract.exe` / `mkvinfo.exe`。前两个是隐写流程之外的独立工具。
6. **`modules/PW.txt` 是"读端字典"而不是写端产物**：写端从不生成密码文件、也不生成任何说明书/口令提示。密码本支持 `解TMD压` 的格式（TAB 第一列），但被 `encode('ascii','ignore')` 阉割了非 ASCII 密码。
7. **`sanitize_path` 只作用于解包，不作用于打包**：写进 ZIP 的 arcname 是原始名（中文会带 UTF-8 标志位 `0x800`），解出时才做 Windows 非法字符/保留名清洗。
8. **进度条估算里写死了 256**：`estimated_overhead = 8 + 256`（pad 上限），说明普通模式"每次最多多写 264 字节"是作者明确的心智模型——也侧面证明噪声本来不属于这个流程。
9. **自包含膨胀事故**：`compress_files` 的 `collect_zip_entries` 专门排除压缩包自身，`build_temp_archive_path` 专门把临时 zip 放到输入目录之外。CHANGELOG v1.3.11 记录了实测"0.7 秒 1MB → 42MB，曾涨到 7.25GB"，触发条件是"输入路径以分隔符结尾"（PowerShell 目录名补全）。**我们的 Rust 版必须同样排除输出自身，并且对尾部分隔符输入做 `normpath`**。
10. **`randint(64, 256)` 含端点、`randint(5, 10) KB` 每份噪声两段**，因此 zarchiver 产物体积 = 外壳 + ZIP + 约 3×(10~20KB)（原子数越多噪声越多，本例外壳 4 个原子 → 3 份噪声 ≈ 42KB，相对 14KB 外壳放大了 3 倍——这也是个可用的指纹特征）。
11. **默认外壳是 Rick Astley 的《Never Gonna Give You Up》**（10.8MB，`cover_video/` 里唯一的文件）：作者用它演示"看起来就是一段正常 MV"。
12. **读端"方法队列 + 静默回退"会掩盖失败**：`reveal_file` 依次尝试 `7zip → mp4_trailing → mp4_zarchiver → free_atom → mkv_attachment`，任一成功即停，失败只写日志。于是"某个方法坏了"在用户体验上等于"没问题"——本仓库里 `extract_with_offset_correction` 对 `pyzipper` 的硬依赖就是这样被掩盖的（探针在无 pyzipper 的解释器下实测：同一条 zarchiver 产物的读端从 `MP4 ZArchiver模式提取` 静默换成 `MP4文件末尾ZIP提取`，结果都报"成功"）。我们做 Rust 版时应当**记录并展示实际命中的方法**，而不是只报一个布尔值。
13. **`startswith(b'PK\x03\x04')` 是 zarchiver 读端的唯一判据**：这意味着如果我们（或第三方工具）在该模式生成的 `free` 原子前面加了哪怕 1 字节，Python 版就再也认不出来（只能靠 7z/zipfile 的尾部路径救）。我们自己的探测器不要复用这个脆弱判据。
