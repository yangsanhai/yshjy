# SteganographierGUI「解除隐写（reveal）」实现深度分析

- 分析对象：`/home/user/hermes/yshjy/refs/SteganographierGUI/Steganographier.py`
- 版本：`VERSION = 1.3.11`，单文件 3847 行，CRLF 行尾，git HEAD `c2c6eaa`
- 取证方式：**纯静态阅读**（源码 + 本机 CPython 3.13 的 `zipfile` 标准库源码交叉印证）。
  本次分析未能运行任何命令（执行被拒），因此凡是涉及"运行时具体表现"的结论都标注了代码依据；
  未标注者均为可直接从源码断言的逻辑。
- 相关外部依赖：`pyzipper==0.3.6`（AES 支持）、系统自带 `zipfile`、`tools/7z.exe`、
  `tools/mkvinfo.exe` + `tools/mkvextract.exe`、密码本 `modules/PW.txt`（仓库内为 1 字节占位空行）。

---

## 1. 概要

`reveal_file()`（L2502）是唯一的解除隐写入口，GUI 批量、CLI 单文件、CLI 目录批量三条路径最终都汇聚到它。
它的骨架是一张**按文件扩展名 + 用户"工作模式"拼出来的方法优先级表**，然后**顺序试错、第一个返回 True 就停止**：

| 优先级 | 方法名 | 适用载体 | 实质 |
|---|---|---|---|
| 最高（若有 `tools/7z.exe`） | `7zip` | 任意 | 交给 7-Zip 两步解压，自己不定位偏移 |
| 视模式/扩展名 | `mp4_trailing` | mp4/m4v/mov | **把整个伪装文件当 ZIP 打开**（依赖 zipfile 的 concat 偏移修正） |
| 视模式/扩展名 | `mp4_zarchiver` | mp4/m4v/mov | 扫顶层 atom，找 `free` 原子且 payload 以 `PK\x03\x04` 开头 |
| 视模式/扩展名 | `free_atom` | mp4/m4v/mov | 用另一套 atom 解析器找第一个 `free && size>1024` 的原子，整段读出来解 |
| 仅 mkv/webm | `mkv_attachment` | mkv/webm | `mkvinfo` 找附件名 → `mkvextract` 抽附件 → 若是 `.zip` 再解一层 |

两个核心设计取向，值得先记住：

1. **它自己不计算"真身偏移"，而是把偏移问题外包**——给 zipfile / 7-Zip（二者都能从文件尾反向扫 EOCD 并自动修正前置字节），
   或者干脆"从 free 原子 payload 里精确切出那一段字节"。
2. **它对"成功"的最终仲裁不是方法返回值，而是输出目录的文件快照差异**（L2634）：
   报了成功但输出目录没多出任何文件 → 判定为假成功，**保留原件**。这是整个流程里唯一的落地校验。

---

## 2. 提取决策流程（总图）

```
reveal_file(input_file_path, password, type_option_var, delete_original=True, auto_rename)
│
├─[0] 组密码候选 password_list  =  [用户密码(非空时)] + PW.txt 全部 + ['']（去重仅针对 ''）
│
├─[1] output_dir = dirname(input)（空则 '.'）
│     files_before = snapshot_output_files(output_dir, exclude=input)
│     （快照排除目录名以 '7z_' / '7z_unzip_' 开头的中间目录，L2474/L2488）
│
├─[2] ext = splitext(lower(input))[1]
│     extraction_methods = []
│     if 存在 tools/7z.exe:            extraction_methods.insert(0,'7zip')   ← 永远第一
│
│     ┌ ext ∈ {.mp4,.m4v,.mov}
│     │   type_option=='mp4'          → [mp4_trailing, mp4_zarchiver, free_atom]
│     │   type_option=='mp4(zarchiver)'→ [mp4_zarchiver, mp4_trailing, free_atom]
│     │   其它（None/'mkv'/空）        → [mp4_zarchiver, mp4_trailing, free_atom]  ← 默认 zarchiver 优先
│     ├ ext ∈ {.mkv,.webm}
│     │   （不区分 type_option）      → [mkv_attachment]
│     └ 其它扩展名（"未知格式"）
│         type_option=='mkv'          → [mkv_attachment, mp4_zarchiver, mp4_trailing, free_atom]
│         否则                        → [mp4_zarchiver, mp4_trailing, free_atom, mkv_attachment]
│
├─[3] for (method) in extraction_methods:            ← 顺序试，成功即 break
│       try: 派发到具体实现；整方法异常 → log 后 continue
│       if success: successful_method = ...; break
│
└─[4] 结果仲裁
      if success and snapshot(after) == files_before:
            log「方法报告成功但没有新文件，保留原件以免数据丢失」   ← 不删
      elif success:
            delete_original ? os.remove(input) : 保留
      else:
            log 4 条猜测：不含隐写数据 / 密码错误 / 文件损坏 / 不支持的隐写方法
      （函数无返回值 —— 见「意外发现 2」）
```

---

## 3. 每种方法的判定条件与伪代码

### 3.1 `7zip` — `_extract_with_7zip`（L2707，优先级最高）

**判定条件（全部为真才继续）**：`os.path.exists(self._7z_exe)`；step1 子进程 `returncode == 0`；temp 目录里能找到 `.zip/.7z/.rar`；step2 每个子压缩包 `returncode == 0`。

```pseudo
if not exists(7z.exe): return False
temp   = mkdtemp(prefix='7z_', dir=output_dir)     # 失败则退回系统临时目录 '7z_extract_'
enc    = locale.getpreferredencoding(False)        # 中文 Windows = cp936

# ---- 步骤 1：把伪装视频当压缩包打开（无密码参数！）----
r1 = run([7z, 'x', input, '-t#', f'-o{temp}', '-y'], encoding=enc, errors='ignore')
if r1.returncode != 0: return False            # 外层加密 → 必失败（没传 -p）

# ---- 步骤 2：找一层内层压缩包并解压 ----
zips = [f for f in listdir(temp) if f.lower().endswith(('.zip','.7z','.rar'))]   # 一层，不递归
if not zips: return False
for zf in zips:
    out = (mkdtemp(prefix='7z_unzip_', dir=output_dir) if auto_rename else output_dir)
    cmd = [7z, 'x', zf, f'-o{out}', '-y', '-mmt=on'] + (['-p'+pwd] if pwd else ['-p'])
    r2 = run(cmd, encoding=enc, errors='ignore')
    if r2.returncode != 0: return False        # ★ 首个失败即整体返回，不试剩下的 zf
    if out != output_dir: _move_extracted_files(out, output_dir); rmtree(out)
return True                                    # 只看 returncode，不校验是否真写出文件
finally: rmtree(temp, ignore_errors=True)
```

要点与坑：
- **步骤 1 没有密码**。因此"加密载体 + 机器上装了 7z.exe"时这一方法必然失败（returncode≠0），
  随后靠 `mp4_trailing` 用密码兜住。也意味着 `-mmt=on`（多线程）只作用在内层。
- **两步 = 多解一层**：如果被隐藏的载荷本身就是个压缩包，7-Zip 路径会把载荷"解到底"，
  用户拿到的是载荷**内部**的文件；而 zipfile 路径只解一层，用户拿到的是**载荷压缩包本身**。
  也就是说"机器上有没有 `tools/7z.exe`"会**改变输出语义**。这是最容易踩的兼容性坑。
- `-t#` 的作用是让 7-Zip 把伪装视频当压缩包打开（7-Zip 自己会从尾部反向扫 EOCD；
  这是"改后缀直接解压"能成立的原因），本工具不参与定位。
- `returncode` 是唯一的成功判据，**没有产物校验**；"解到临时目录后搬运失败"这类情况靠 `snapshot` 兜底。

### 3.2 `mp4_trailing` — `_try_mp4_direct_extraction`（L2658）→ `_extract_with_zipfile`（L2833）/ `_extract_with_pyzipper`（L2925）

**判定条件**：没有任何格式判定——**把整个文件直接当 ZIP 打开**，成败完全由 `zipfile`/`pyzipper` 决定。

```pseudo
for pwd in password_list:
    pwd_bytes = pwd.encode('utf-8') if pwd else None      # 空密码 → None
    try: if _extract_with_zipfile(input, pwd_bytes, size): return True
    except (BadZipFile, RuntimeError, ValueError): log('可能是AES加密')
    try: if _extract_with_pyzipper(input, pwd_bytes, size): return True
    except (...): continue
return False

# _extract_with_zipfile（与 _extract_with_pyzipper 同构，仅 ZipFile/AESZipFile 之差）
with open(input,'rb') as f, ZipFile(f,'r') as z:
    z.setpassword(pwd_bytes)                    # 空密码时跳过
    names = z.namelist()
    if names:                                   # ★ 唯一的"密码预验证"
        first = 首个不以 '/' 结尾的成员
        if first: z.open(first).read(1024)      # 读 1KB；Bad password → return False
    for name in names:
        clean = sanitize_path(name)
        if not clean: continue                  # 成员名被清空 → 跳过该成员
        if name.endswith('/'): makedirs(...); continue
        makedirs(dirname); resolved = _resolve_conflict_path(path)
        with z.open(name) as src, open(resolved,'wb') as dst:
            while chunk := src.read(32MB): dst.write(chunk)   # 32MB 缓冲 + 进度回调
        # 写失败只有 except (OSError, IOError): log + continue
    return True                                 # ★ 即使成员全被跳过也返回 True
```

要点与坑：
- `namelist()` 为空时**跳过密码验证直接 return True**（空包假成功），靠 `snapshot` 仲裁。
- 密码验证只覆盖"第一个非目录成员"；后面成员的密码错误会在写入时抛 `RuntimeError`，
  而循环里只捕获 `(OSError, IOError)` → 异常冒泡 → 该方法对该密码整轮失败，且**已落盘的部分文件不会清理**。
- 单个成员 CRC 错误同样会冒泡 → 整个密码轮次失败（其它完好成员也拿不到）。
- 目录项用 `name.endswith('/')` 判定；成员名走 `sanitize_path`（见 3.7）。

### 3.3 `mp4_zarchiver` — `extract_with_offset_correction`（L3018）

**判定条件（很硬）**：
1. 文件 ≥ 12 字节；
2. 偏移 0 处 8 字节头里 `atom_type == 'ftyp'`（即**假定 ftyp 在文件开头**，并用其 size 直接 `seek(size)` 跳过）；
3. 顶层 atom 遍历中遇到 `free` 且 `size > 8`；
4. 该 free 原子 payload **以 `b'PK\x03\x04'` 开头**。

```pseudo
header = read(12); if len<12: return False
if type(header[0:8]) != 'ftyp': return False          # 不是 MP4 结构直接放弃
f.seek(ftyp_size)
while True:
    pos = f.tell(); hdr = f.read(8)
    if len(hdr) < 8: break                            # 到 EOF
    size, type = unpack(hdr)
    if type == 'free':
        if size <= 8: continue                        # ★ continue 而非 seek(pos+size)：位置错位隐患
        data = f.read(size - 8)                       # ★ 整段读入内存（GB 级 = OOM 风险）
        if data[:4] == b'PK\x03\x04':
            for pwd in password_list:
                try: pyzipper.AESZipFile(BytesIO(data)).setpassword(...); _extract_zip_members(...)
                     return True
                except Exception: continue            # 无预验证
            return False                              # 所有密码失败
        else:                                          # 签名嗅探（仅日志，不做实际处理）
            Rar! / 7z\xbc\xaf'\x1c / \x1f\x8b → log "找到X格式数据，但当前只支持ZIP格式"
            都不匹配 → log "free原子中的数据不是已知的压缩格式"
        # 继续 while，找下一个 atom
    else:
        if size <= 8: log '原子大小异常'; break
        f.seek(pos + size)                            # 正常跳到下一个顶层 atom
return False
```

要点与坑：
- 这是**唯一**要求 `payload[0:4] == PK\x03\x04` 的路径。而本工具自己的 `mp4`（WinRAR）模式在 free 原子里
  先垫 64–256 字节随机数据（L2137），所以**该模式文件的 free payload 不以 PK 开头 → 本方法对它无效**，
  靠 `mp4_trailing` 兜住。→ Rust 版判定不能这么写（见第 6 节）。
- 本方法**不修改/不校验任何 stco/co64 偏移**，只把 free 原子里的字节当独立 ZIP 处理，因此原始视频流偏移不受影响。
- `continue` 未重定位是真实隐患：`size <= 8` 时文件指针停在 pos+8，下一轮解析就会错位。

### 3.4 `free_atom` — `extract_from_free_atom`（L3296，配 `read_mp4_atoms` L3151）

**判定条件**：顶层 atom 链里存在**第一个** `type=='free' and size>1024` 的原子。

```pseudo
atoms = read_mp4_atoms(file)                 # 支持 size==1（64位）+ size==0（到EOF）两种形式
target = 第一个 type=='free' and size>1024 的 atom
if not target: return (False, '未找到包含数据的free原子')
raw = read(offset+header_size, size-header_size)      # ★ 整段读入内存
写临时文件 raw.tmp.zip
for pwd in password_list:
    try: AESZipFile(tmp); if not namelist(): continue; _extract_zip_members(...) → (True,'... (AES)')
    except (BadZipFile, RuntimeError): pass
    try: ZipFile(tmp);   if not namelist(): continue; _extract_zip_members(...) → (True,'... (Standard)')
    except (BadZipFile, RuntimeError): pass
return (False, '所有密码尝试失败')
finally: unlink(tmp)
```

要点与坑：
- 与 3.3 的判定条件**不同**：这里不看 payload 首字节，因此对"free 里垫了随机前缀"的 `mp4` 模式也有效
  （依赖解压器自身的 concat 修正）。
- `size > 1024` 太宽松：合法的、播放器用来对齐的普通 `free` 填充原子也满足 → 会被误选，
  然后失败且**不再尝试后续其它 free 原子**（无回退搜索）。
- `read_mp4_atoms` 对 `size < 8` 没有下限校验（`size=0` 例外），异常 size 会导致 seek 异常位置；
  它也不递归容器、不做偏移修正。

### 3.5 `mkv_attachment` — `_try_mkv_extraction`（L3392）

**判定条件**：`mkvinfo` 输出里能解析出附件名；`mkvextract attachments <file> 1:<out>` 成功。

```pseudo
# 附件名：在 mkvinfo 输出里找含 "MIME" 的行 → 取它【上一行】按 ':' 分割的第 2 段 → 取最后一个空白 token
attachments_name = 上述解析；解析失败 → return False
out = _resolve_conflict_path(dirname(input)/attachments_name)
run([mkvextract, 'attachments', input, f'1:{out}']); 失败 → return False
if out 以 '.zip' 结尾:
    for pwd in password_list:
        try: AESZipFile(out, 'r', compression=ZIP_DEFLATED, encryption=WZ_AES)   # ★ 读模式传写参数，无效
             setpassword(pwd.encode()); _extract_zip_members(...)
             os.remove(out); return True
        except RuntimeError: continue          # 无预验证，已写出的成员残留
    return False                               # ★ 已抽出的 .zip 不再删除，留在输出目录
else:
    return True                                # 附件不是 zip：直接交付附件本体
```

要点与坑：
- 附件索引硬编码为 `1:`（隐写时 zip 是第 1 个附件，随机 8KB 是第 2 个）；若封面 mkv 本身已有附件，索引会错。
- `mkvinfo` 输出固定按 `utf-8` 解码、无 `errors=`，非 UTF-8 输出会抛异常被外层吞掉 → 表现为"没有可提取的附件"。
- 附件名解析依赖"MIME 行上方一行"的位置假设，非常脆弱。

### 3.6 只在隐写侧用、reveal 侧完全没调用的函数

`find_and_update_offsets_in_atom`（L3198）/ `update_container_atom_offsets`（L3259）实现 `stco`/`co64` 表项的
整体平移（容器递归：`moov→trak→mdia→minf→stbl→udta`），**仅在 `mp4(zarchiver)` 隐写时**（L2291）用于抵消插入 free 原子造成的偏移。
reveal 侧从不调用它们——因为 reveal 只取原子内部载荷，不重排文件，原视频偏移天然仍然有效。
**对 Rust 版的含义**：如果我们要做"剥离隐藏数据、还原出可播放的原视频"，就必须自己实现这套偏移修正。

### 3.7 `sanitize_path`（L539）— 成员名清洗

依次：`\xa0 → 空格`；删除 Unicode 类别以 `C` 开头的字符（控制/格式字符，如 `\u202c`、`\u200b`）；
`strip()` + `strip('\u3000')`；`<>:"|?*` → `_`；按 `/` 分段，每段 `strip('. ')`，Windows 保留名（CON/PRN/AUX/NUL/COM1-9/LPT1-9）加下划线；
`rstrip('.')`；超 255 截断。

**未做路径穿越防护**：`../../x` 的每个 `..` 段会被 `strip('. ')` 清成空串，join 后得到 `//x`，
在 POSIX 下 `os.path.join(dir, '//x')` 返回 `'//x'`（绝对路径）→ 存在写到目标目录之外的隐患（Zip Slip）。
（此条为对 Python 语义的静态推断，本次未运行验证。）

---

## 4. Python `zipfile` 为什么能把整份伪装的 mp4 当 ZIP 打开？（问题 3 的硬核答案）

依据本机 `/usr/lib/python3.13/zipfile/__init__.py`（Python 3.13 已把 `zipfile` 拆成包）：

**(a) 找 EOCD：`_EndRecData`（L312–369）**
1. `seek(0,2)` 取文件大小；
2. **快路径**：直接读最后 22 字节，若签名是 `PK\x05\x06` 且末尾 `\x00\x00`（无注释）→ 直接当 EOCD；
3. **慢路径**：从 `max(filesize - 65535 - 22, 0)` 起读 65557 字节，`data.rfind(b'PK\x05\x06')` 找注释后的 EOCD；
   找不到 → `return None` → `_RealGetContents` 抛 `BadZipFile("File is not a zip file")`（L1467-1468）。

> 结论：**只要 EOCD 落在文件末尾 64KB + 22 的窗口内，前面有多少字节都无所谓。**
> 本工具写入的 zip 带 comment（SHA-256/时间戳，`compress_files` L1938/1993），所以快路径通常不成立，走慢路径。

**(b) 修正前置偏移：`_RealGetContents`（L1460–1526）**

```python
size_cd   = endrec[_ECD_SIZE]      # 中央目录字节数
offset_cd = endrec[_ECD_OFFSET]    # 中央目录在"ZIP 起点"坐标系里的偏移（写包时是相对 0 的）
# "concat" is zero, unless zip was concatenated to another file
concat = endrec[_ECD_LOCATION] - size_cd - offset_cd    # L1476：EOCD 真实位置 - CD大小 - CD偏移
self.start_dir = offset_cd + concat                     # L1482
...
x.header_offset = x.header_offset + concat              # L1524：每个本地头偏移整体平移
```

`_ECD_LOCATION` 是 `_EndRecData` 附加的"EOCD 在本文件中的真实 seek 偏移"。
于是 **前置数据长度被精确地算成 `concat`，并被无差别的加到每一个本地文件头偏移上**——这正是"整份 mp4 当 ZIP 打开"能成立的机制。
`_EndRecData64`（L252）在此之前已处理 ZIP64 EOCD，所以 ZIP64 也一并生效。

**(c) 对我们写 Rust 版意味着什么**

- **必须自己算偏移。** `zip` crate 的 `ZipArchive::new(reader)` 只从 reader 起点解析 EOCD/CD，
  它**不会**帮你加减 base；把整份 mp4 喂进去会失败或解出错误数据。正确做法二选一：
  1. 自己实现 `_EndRecData` 等价逻辑（尾部 64KB+22 反向扫 `PK\x05\x06`，兼容 ZIP64 的 `PK\x06\x06`/定位器），
     算出 `base = eocd_abs_pos - cd_size - cd_offset`，再 `seek(base)` 之后把 reader 交给 `zip` crate；
  2. 或者解析 CD 后把每个 LFH 偏移自减 `base`（与 CPython 的 `+concat` 等价，符号相反）。
- **必须做"EOCD 必须在尾窗口内"这一前提的显式检查**：模式 B（free 原子内嵌）里 EOCD 后面还跟着
  几 MB～几 GB 的 mp4 数据 + 随机特征码 + `mdat` 盒，**超出 64KB 窗口 → 通用解压器和 7-Zip 都认不出**。
  这就是本工具不得不额外写两条"从 free 原子切片"路径的根本原因。
- 细节：`errors='replace'`/cp437 的成员名解码、`0xFFFFFFFF` 的 ZIP64 哨兵值、EOCD 的 comment 长度字段，
  都要与 CPython 行为对齐，否则会漏掉本工具生成的所有包（它们**一定**带 comment）。

---

## 5. 密码策略（问题 2）、报错与降级（问题 4）、容错提取（问题 5）、中文编码（问题 6）

### 5.1 密码候选与重试顺序

```python
password_list = []
if password: password_list.append(password)     # GUI：仅当用户"动过"密码框才非空（password_modified，L816/1345）
password_list.extend(self.passwords)            # modules/PW.txt（构造时加载一次，L1619）
if '' not in password_list: password_list.append('')   # 空密码永远兜底
```

- 密码本加载（L2450）：`open(password_file, 'r', encoding='utf-8-sig')` 逐行 `split('\t')` 取 **第 1 段**，
  再做 `encode('ascii','ignore').decode('ascii')`。
  - 兼容「解TMD压」式 `密码<TAB>其它` 的密码本格式（README 明确提到）。
  - **代价：密码本里的任何非 ASCII（中文）密码会被静默丢弃。**
- 去重只保证 `''` 唯一：用户密码若同时存在于密码本 → 会被尝试两次（幂等，浪费）。
- 每条密码的验证粒度不同：
  - `7zip`：外层 **无密码**；内层 `-p<password>`（空密码传裸 `-p`）。
  - `mp4_trailing`：先 `zipfile`（只能 ZipCrypto/无加密），再 `pyzipper`（AES）；两者都带 1KB 预验证。
  - `mp4_zarchiver` / `free_atom` / `mkv_attachment`：**无预验证**，直接解到底。
- 密码总是被编码成 **UTF-8 字节**（`password.encode('utf-8')`、`password.encode()`），没有第二种编码候选。

### 5.2 中文密码 / GBK-UTF-8（问题 6）

代码里的原样处理：

| 位置 | 处理 | 后果 |
|---|---|---|
| 密码本 L2453-2458 | `utf-8-sig` 读 + ASCII 过滤 | 中文密码被丢，无法从密码本匹配中文密码 |
| 密码编码 `_try_mp4_direct_extraction` L2669 | `pwd.encode('utf-8')` | 与"本工具自己隐藏的包"一致（隐藏时也是 utf-8，L1965）；与 WinRAR/7-Zip 在中文 Windows 上用 GBK 建的包**不一致** |
| 7z 子进程 L2731/2750 | `encoding=locale.getpreferredencoding(False)`（中文 Windows=cp936）+ `errors='ignore'` | 修的是 **7-Zip 输出文本**的解码（否则 GBK 输出撞 UTF-8 抛 UnicodeDecodeError）；密码本身作为 argv 传，Windows 下走 Unicode API，中文密码能正确到达 7-Zip |
| `mkvinfo` 输出 L3401 | 固定 `encoding='utf-8'`，无 errors | 非 UTF-8 输出 → 异常被吞 → 误报"没有可提取的附件" |
| ZIP 成员名 | 完全依赖 zipfile 默认（UTF-8 标志位或 cp437） | GBK 中文文件名 → mojibake 落地；`sanitize_path` 只能删控制字符，改不了解码结果 |
| 源码注释 L1966-1968 | 保留 `ansi` / utf-8 的试验注释，最终**只启用 utf-8** | 说明作者意识到过编码问题，但没有留 GBK 回退 |

### 5.3 失败/损坏/不支持 的报错与降级

- **单方法异常** → `log` 后 `continue` 下一个方法（L2629-2631）。方法内部普遍 `try/except → return False`，
  所以**异常几乎不会冒泡**。
- **全部失败** → 只打印 4 条"可能原因"（不含隐写数据 / 密码错误 / 文件损坏 / 不支持的隐写方法），
  **不抛异常、不返回状态、不提示用户下一步**。
- **格式不支持**：不是拒绝，而是"未知扩展名 → 尝试所有方法"（L2572-2587）。
- **压缩格式识别**：只对 ZIP 有真实处理；RAR/7z/GZIP 仅识别签名并 log "当前只支持ZIP格式"，**降级为不动**。
- **假成功防护**：唯一一处"结果真伪校验"是 L2634 的 `snapshot_output_files` 前后比对（成员名全被
  `sanitize_path` 过滤、空包、只含目录项、7z 搬运失败等都会命中）→ 保留原件 + 警告。

### 5.4 有没有暴力 / 容错提取（问题 5）

- **没有密码暴力破解**：无字典生成、无掩码/长度枚举、无并行，只有"密码本顺序 + 空密码兜底"。
- **没有分卷、没有 `zip -FF` 式结构修复、没有"从损坏包里抢救部分成员"**。
- 所谓"容错"实际只有三点：
  1. zipfile/pyzipper 的 concat 偏移修正（不必知道真身偏移，预置垃圾字节也能认）；
  2. 7-Zip 自身的"把普通文件当压缩包打开"能力（同样反向扫 EOCD）；
  3. 写文件失败时 `except (OSError, IOError): continue`（跳过单个成员）。
- **反例（重要）**：CRC 不匹配或后面成员的密码错误会让异常冒泡，导致**整个密码轮次失败**，
  即使其它成员是完好的——"数据不完整也能捞出"这条路并没有被实现。

---

## 6. 关键代码位置表

| 位置 | 函数 / 常量 | 一句话作用 |
|---|---|---|
| `Steganographier.py:2450` | `load_passwords` | 读 `PW.txt`（utf-8-sig、TAB 取首段、强制 ASCII），返回密码列表 |
| `Steganographier.py:2474` | `EXTRACT_TEMP_DIR_PREFIXES` | `('7z_','7z_unzip_')`，快照时必须排除的中间目录前缀 |
| `Steganographier.py:2476` | `snapshot_output_files` | 输出目录快照（相对路径→大小+mtime_ns），用于判定"到底有没有写出文件" |
| `Steganographier.py:2502` | **`reveal_file`** | 解除隐写总入口：组密码表→按扩展名/模式排方法→顺序试错→快照仲裁→删/留原件 |
| `Steganographier.py:2658` | `_try_mp4_direct_extraction` | 模式 A：整文件当 ZIP，逐密码 zipfile→pyzipper |
| `Steganographier.py:2707` | `_extract_with_7zip` | 7-Zip 两步解压（外层无密码、内层带密码），系统编码解码子进程输出 |
| `Steganographier.py:2833` | `_extract_with_zipfile` | 标准 zipfile 解压：1KB 密码预验证 + sanitize_path + 32MB 分块写 |
| `Steganographier.py:2925` | `_extract_with_pyzipper` | 同上，改用 `pyzipper.AESZipFile` 以支持 AES |
| `Steganographier.py:3018` | `extract_with_offset_correction` | 模式 B（ZArchiver）：顶层 atom 扫描，要求 free payload 以 `PK\x03\x04` 开头 |
| `Steganographier.py:3151` | `read_mp4_atoms` | 顶层 atom 解析，支持 32 位 / large size(64) / size==0 到 EOF |
| `Steganographier.py:3198` | `find_and_update_offsets_in_atom` | `stco`/`co64` 表项平移（**仅隐写侧调用**） |
| `Steganographier.py:3259` | `update_container_atom_offsets` | 容器原子（moov/trak/mdia/minf/stbl/udta）递归更新偏移（**仅隐写侧**） |
| `Steganographier.py:3296` | `extract_from_free_atom` | 找第一个 `free && size>1024` 原子，整段读出→临时 zip→逐密码 pyzipper/zipfile |
| `Steganographier.py:3392` | `_try_mkv_extraction` | `mkvinfo` 解析附件名 → `mkvextract` 抽 #1 附件 → `.zip` 再解一层 |
| `Steganographier.py:1733` | `_resolve_conflict_path` | 同名自动改名 `name (1).ext` |
| `Steganographier.py:1785` | `_extract_zip_members` | 通用 ZIP 逐成员解压（sanitize + 改名），供 zarchiver/free_atom/mkv 复用 |
| `Steganographier.py:1815` | `_move_extracted_files` | 7z `7z_unzip_` 临时目录 → 输出目录的搬运 |
| `Steganographier.py:539` | `sanitize_path` | 成员名清洗（控制字符/非法字符/保留名/255 截断） |
| `Steganographier.py:1344` | GUI `get_password` | 只有 `password_modified` 为真才把输入框内容当密码 |
| `Steganographier.py:1619` | `__init__` 中 `self.passwords = self.load_passwords()` | 构造时加载密码本，之后除非 GUI 打开密码本否则不再刷新 |
| `Steganographier.py:3646` | `process_hidden_files` | CLI 批量目录：扫 `.mp4/.mkv/.mov/.m4v/.webm` 逐个 `reveal_file` |
| `/usr/lib/python3.13/zipfile/__init__.py:312-369` | `_EndRecData` | 尾部 22 字节快路径 + 尾部 64KB+22 `rfind` 慢路径找 EOCD |
| `/usr/lib/python3.13/zipfile/__init__.py:1460-1526` | `_RealGetContents` | `concat = ECD_LOCATION - size_cd - offset_cd`；`header_offset += concat`（前置字节修正的核心） |

---

## 7. 「对我们的启示」：Rust 版必须实现的判定分支 + 必须避开的坑

### 7.1 必须实现的判定分支（缺一不可）

1. **EOCD 反向定位 + base 计算**（等价 CPython `_EndRecData` + `_RealGetContents`）
   - 快路径：最后 22 字节是否为 `PK\x05\x06`；
   - 慢路径：最后 64KB+22 内 `rfind` EOCD；解析 ZIP64 EOCD/定位器（`PK\x06\x06` / `PK\x06\x07`）；
   - `base = eocd_abs - cd_size - cd_offset`，随后所有 LFH 偏移加 `base`，并对每个 LFH 校验 `PK\x03\x04`。
   - **不要假定 ZIP 从偏移 0 开始**，也不要依赖 `zip` crate 自动修正——它不会。
2. **两条独立探测的伪装布局**
   - A｜尾部拼接：EOCD 落在文件尾 64KB 内即可直接判为模式 A（本工具 `mp4` 模式）；
   - B｜free 原子内嵌：解析顶层 box 链（32 位 + large size + size==0），对每个 `free(size>8)` 的 payload
     在**其内部**搜索 EOCD/`PK\x03\x04`/`PK\x05\x06`/`Rar!`/`7z\xbc\xaf'\x1c`/`\x1f\x8b`/`BZh`/`\xfd7zXZ\x00`；
     **正判定应当是"payload 内存在可解析的 EOCD"，而不是"payload 第一个字节是 PK"**
     ——否则会漏掉本工具 `mp4` 模式里"64–256 字节随机填充 + ZIP"的布局（这正是原实现的漏洞所在）。
3. **密码候选序列与密码本语义**：用户密码 → `PW.txt`（UTF-8-SIG、TAB 首段）→ 空密码兜底；
   并为每个候选派生**编码变体**：`UTF-8` / `GBK(CP936)` / 原始字节（有则用），
   这样既能打开本工具生成的包，也能打开 WinRAR/7-Zip 在中文 Windows 上生成的包。
4. **签名嗅探 + 真正的多格式支持**：把原工具"只识别、不处理"的 RAR/7z/GZIP 至少做能解的分支
   （仓库里已有 `refs/sevenz-rust`、`refs/7-Zip`、`refs/XADMaster` 可参考）。
5. **落地校验再删原件**：解压前后对输出目录做快照（路径→大小+mtime），
   "报成功但无新文件/新文件全为 0 字节"一律视为失败并保留原件。
6. **密码预验证**：解压前先对第一个非目录成员读少量字节（或走 AES 的验证码），验证通过再落盘；
   失败即换下一个候选密码，**不留下半成品**。
7. **成员级容错**：单个成员 CRC 错误/写失败，记录并继续下一个成员，
   最后汇总"成功 N / 失败 M（附名单）"，不要让一个坏成员毁掉整轮。
8. **（可选但推荐）偏移修正能力**：若产品要支持"剥离隐藏数据还原原视频"，
   需实现 `stco`/`co64` 平移 + 容器递归（对应 `find_and_update_offsets_in_atom` / `update_container_atom_offsets`）。

### 7.2 必须避开的坑

- **不要把整个载荷读进内存**：`free_atom` / `mp4_zarchiver` 两条路径都是 `f.read(size-8)` 全读，
  GB 级载荷直接 OOM。Rust 版应保持流式（只读 box 头 + EOCD/CD 索引，按需 seek）。
- **不要假定 box 链干净**：`mp4(zarchiver)` 产物会在 atom 之间插入"随机压缩签名 + 5–10KB 随机字节"
  且因缩进 bug 反复插入（见 8.1），还会出现 `mdat(size=8)` 空盒与尾部垃圾；解析器必须容忍并跳过。
- **不要假定 ZIP 后面没有数据**（模式 B）：任何"从尾部扫 EOCD"的通用解压器在模式 B 上都会失败，
  必须自己先切出正确的字节区间。
- **不要忽略 ZIP 的 comment**：本工具必然写入 comment（SHA-256/时间戳），尾部定位要走慢路径逻辑。
- **不要用 `size < 8` 的 box 尺寸去 seek 后继续解析**（原实现 `continue` 未重定位），
  必须显式跳到 `pos + max(size, 8)`，并对荒谬尺寸设上限。
- **不要只试 UTF-8 密码、也不要只按 cp437/UTF-8 解成员名**：中文场景必须补 CP936，并对成员名做
  编码启发式（UTF-8 标志位 → UTF-8；否则试 CP437/CP936 取"更像中文/更少替换符"的那个）。
- **不要让"解压器是否存在"改变输出语义**：原工具在装了 7z.exe 时会多解一层内层压缩包，
  不装时只解一层（见 3.1）。Rust 版要么固定单层，要么用显式开关表达"递归深度"。
- **不要静默丢弃非 ASCII 密码**（原密码本的做法），也不要静默跳过被清洗掉的成员名而不报告。
- **必须做路径穿越防护**：`sanitize_path` 的 `strip('. ')` 会把 `..` 变成空串进而产生绝对路径
  `//x`（Zip Slip 隐患）。Rust 版要：拒绝绝对路径、剔除 `..` 分量、规范化后断言仍在目标目录内、
  拒绝符号链接成员、限制总路径长度。
- **批量统计要真实**：原 `process_hidden_files` 把"未抛异常"当成功计数（见 8.2），我们的 CLI 必须回报真实成败。

---

## 8. 意外发现

1. **`mp4(zarchiver)` 隐写侧的缩进 bug（真实存在，且影响 reveal 的输入形态）**：
   L2301–2323 的"随机压缩签名 + 5–10KB 随机字节 ×2 + 空 `mdat` 盒"整块代码缩进是 **32 空格**，
   与外层 `while pos < len(remaining_data)` 的循环体同级（L2264 起），而不是循环之后。
   → 每处理**一个**顶层 atom 就追加一次这组垃圾（约 10–20KB + 8 字节）。
   实测影响：`mp4(zarchiver)` 产物体积远大于"封面 + 载荷"的预期，尾部出现多轮签名/垃圾块。
   对我们的意义：这种文件本身就是"脏 box 链"，我们的解析器必须能跳过 `mdat(size=8)` 与随机垃圾到 EOF。
2. **`reveal_file` 没有返回值**：调用方 `process_hidden_files`（L3646）把"不抛异常"计为成功，
   于是 CLI 批量日志里的"成功 N 个"**恒等于处理的文件数**，即使全部失败也是 100% 成功——统计是假的。
3. **free 原子选择只看第一个**：`extract_from_free_atom` 取第一个 `free && size>1024`，
   合法播放器对齐用的 `free` 填充也可能满足；一旦选错就失败且**不回头试后面的 free 原子**。
4. **7-Zip 路径改变用户拿到的东西**：装了 `tools/7z.exe` → 隐藏的压缩包会被多解一层；
   没装 → 只解出压缩包本身。同一份文件在不同机器上产出不同，这是很容易被误报的"行为不一致"。
5. **错密码会留下垃圾**：`free_atom` / `mp4_zarchiver` / `mkv` 三条路径没有密码预验证，
   错密码时会先创建 0 字节/截断的成员文件，失败后不清理。这些半成品**不在快照排除名单里**，
   于是会成为下一次调用的"新增文件"，可能污染后续成功判定（跨调用污染）。
6. **`-t#` 的含义**：代码显式给 7-Zip 传 `-t#`（并在注释里说明两步是为了修中文路径编码问题），
   实质是让 7-Zip 把伪装视频当压缩包打开；这一步完全不依赖本工具的偏移计算。
   （7-Zip 各版本对 `-t#` 的容错略有差异，此处仅按代码意图描述，未实测。）
7. **偏移修正函数在 reveal 侧是死代码**：`find_and_update_offsets_in_atom` 与
   `update_container_atom_offsets` 只服务隐写；reveal 的设计哲学是"只取原子内载荷、绝不重排文件"，
   因此原视频流偏移永远有效——这是一个很值得沿用的简化。
8. **密码本会被 ASCII 过滤**：仓库自带 `modules/PW.txt` 只有 1 个字节（`\n`）；而按 L2458 的逻辑，
   用户往密码本里写中文密码是**无效**的（会被 `encode('ascii','ignore')` 抹掉），
   与 README 宣传的"逐行写好常用密码自动尝试"存在隐蔽偏差。
9. **扩展名与模式的耦合不对称**：`.mkv/.webm` **只**尝试 `mkv_attachment`（外加 7z），
   不会尝试 ZIP 兜底；而 `.mp4/.m4v/.mov` 在 `type_option != 'mp4'` 时默认顺序是
   `zarchiver → trailing → free_atom`，也就是说"默认自动模式"其实**优先假设**了 free 原子内嵌布局。
