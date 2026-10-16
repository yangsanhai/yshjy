# refs —— unbox 参考代码库

> 目的：研究清楚「别人怎么造的、怎么解的」，再决定我们怎么写。
> 建立：2026-10-16　共 6 个项目 / 314 MB

---

## 一、目录一览

| 项目 | 语言 | 体积 | 为什么拉它 | 对我们的价值 |
|------|------|------|-----------|-------------|
| **SteganographierGUI** | Python | 111M | ⭐ 就是造这类包的工具 | **解法说明书**（藏法+解法全在里面） |
| **zip2** (zip-rs) | Rust | 141M | 我们打算用的 ZIP 库 | API + 实现参考 |
| **7zip** | C++ | 16M | `-t#` 扫描模式的祖师爷 | 扫符号分段逻辑 + LZMA |
| **XADMaster** (unar) | ObjC/C++ | 12M | 多格式「嗅探识别」标杆 | 格式识别策略 |
| **PeaZip** | Pascal | 31M | 跨平台压缩 GUI | 界面/交互参考 |
| **sevenz-rust** | Rust | 3.1M | 我们打算用的 7z 库 | 7z 解压实现 |

---

## 二、⭐ 核心成果：隐写者源码分析（我们已知的全部真相）

### 2.1 它有**两种**藏法（必须都支持！）

**模式 A：普通 MP4（尾部拼接）**
```
[封面视频: ftyp + free + mdat + moov] + [ZIP 压缩数据] + [尾部 8 字节空 mdat]
```
- 尾部那个「空 mdat」（`00 00 00 08 mdat`）是**故意加的**，让系统/播放器仍认它是 MP4
- ZIP 目录表里的偏移是**相对于 ZIP 数据起点**的 → 标准解压器一读就判定「损坏」
- **WinRAR 能解**，因为它支持「prepended data」（前面多出字节的容错）
- **Python zipfile 也能解**，因为 `_RealGetContents` 会自动计算 concat 偏移
- Linux 的 `unzip`/`7z` 直接开**会失败** ← 用户卡的就是这里

**模式 B：ZArchiver 兼容（free atom 内嵌）**
- ZIP 数据被塞进 MP4 的 **free atom** 里（`create_free_atom_with_data`）
- 支持 >4GB（large size 形式：size 字段写 1，后跟 8 字节真实大小）
- 从**文件开头**扫描的工具（zarchiver / 解TMD压）能解，WinRAR 反而解不了
- 源码见 `extract_with_offset_correction()` (L3018)

### 2.2 关键函数（对我们 = 反向说明书）

| 函数 | 行号 | 作用 |
|------|------|------|
| `add_empty_mdat_box` | 2422 | 尾部写 8 字节空 mdat ← 今晚文件末尾那个 |
| `create_free_atom_header` | 2427 | free atom 头（含 >4GB 处理） |
| `create_free_atom_with_data` | 2440 | 用 free atom 装载隐藏数据 |
| `compress_files` | 1891 | 压缩（7z / zip） |
| `hide_file` | 2058 | 藏（主流程） |
| `reveal_file` | 2502 | 解（主入口） |
| `_try_mp4_direct_extraction` | 2658 | 模式A：直接拿整个 mp4 当 ZIP 打开 |
| `_extract_with_zipfile` | 2833 | 标准 ZIP 路径 |
| `_extract_with_pyzipper` | 2925 | AES 加密 ZIP 路径 |
| `_extract_with_7zip` | 2707 | 7z 两步解压（修中文路径编码） |
| `extract_with_offset_correction` | 3018 | 模式B：free atom 提取 |
| `read_mp4_atoms` | 3151 | 读 MP4 atom 结构 |
| `find_and_update_offsets_in_atom` | 3198 | 修改 atom 内部偏移 |
| `update_container_atom_offsets` | 3259 | 修改容器 atom 偏移 |
| `load_passwords` | 2450 | 密码本加载（modules/PW.txt） |

### 2.3 其他情报
- 密码本：`modules/PW.txt`，每行一个密码（Tab 分隔），加载时过滤非 ASCII
- 它自己承认两种模式互斥：ZArchiver 模式生成的文件 WinRAR 解不了，反之亦然
- 中文密码坑（官方 Wiki）：7z / Bandizip 对 GBK 密码要转成 UTF-8 才能解，WinRAR 不用

---

## 三、我们自己的实测结论（2026-10-16，真实 8.7GB 文件）

```
文件结构: ftyp(28) + free(8) + mdat(115807419) + moov(3440576)
          = 119248031 字节 ← 到这里为止是「封面视频」
之后:     ZIP64 数据 8592555306 字节
再之后:   14354 字节（含尾部空 mdat）
```

- ZIP64 目录表 `cd_offset = 0xFFFFFFFF`（分卷标志），且偏移数值与真实位置**差 2355200**
  → 标准工具判定「结构损坏」直接放弃
- **反推公式**：`ZIP 起点 = CD 实际位置 − ZIP64EOCD 里记录的 cd_offset`
  → 并用该处必须是 `PK\x03\x04` 做自校验
- 提取出的分卷 SHA-256 与 EOCD 注释里的指纹**完全一致** ✓（说明反推正确）
- 交叉验证：7-Zip `-t#` 扫描模式分段结果 = `119248031 / 8592555306 / 14354`，与手算一字不差 ✓

---

## 四、看代码的入口建议

### 想理解「怎么解」→ SteganographierGUI
```bash
cd refs/SteganographierGUI
# 主逻辑全在单个文件里（3847 行）
grep -nE "^    def " Steganographier.py
# 从 reveal_file (L2502) 往下读提取路径
```

### 想写 Rust ZIP → zip2
```bash
cd refs/zip2
ls src/
# 重点：它如何处理 EOCD / ZIP64 / prepend 数据
```

### 想理解「扫符号分段」→ 7zip
```bash
cd refs/7zip
# -t# 扫描模式的实现（C++）
grep -rn "Scan" CPP/7zip/Archive/ | head -30
```

### 想理解「多格式嗅探」→ XADMaster
```bash
cd refs/XADMaster
# 各种格式的识别与解压句柄（CS*Handle）
```

---

## 五、待补

- [ ] 7-Zip `-t#` 扫描模式的 C++ 实现定位（它怎么判定"这里有一段归档"）
- [ ] zip2 对「prepend 数据 / 偏移异常」的处理方式（决定我们要不要自己算偏移）
- [ ] sevenz-rust 对「AES 加密 7z + 分卷」的支持程度
- [ ] 隐写者 `hide_file` 完整流程（打包端要兼容它的输出）
- [ ] 其他造包工具：apate、`copy /b` 手工拼的
