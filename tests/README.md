# yshjy / unbox 测试样本集

> 生成日期：2026-10-16　生成脚本：`tests/make_samples.py`（可重跑）　机器可读结果：`tests/results.json`
> 样本目录：**`/home/user/hermes/yshjy/tests/data/`** —— 共 **35 个文件 / 14.3 MB**，单文件最大 2.77 MB

本文件逐样本说明：**文件名 / 大小 / 造法 / 期望结构 / 用什么能解开 / 实测结果 / 已知坑**。
下面所有「实测」数字都来自 `results.json`（最后一次生成时跑出来的真实值，非手写）。

---

## 0. 一句话总结

样本分四组：**A 隐写者（真源码生成）**、**B 手工/其它工具风格**、**C 普通压缩包（含加密/分卷）**、**D 异常损坏包**。
其中 **A4b、B4、D2a、D2b、C8/C9/C10 是真正有牙齿的样本**；A1/A2/A3、B1、B3、C1 这类是「宽路径」样本。
最能暴露我们工具缺陷的是 **A4b（尾部噪声 >64KB，尾部 EOCD 窗口失效）** 和 **D2b（前置 2.3MB 垃圾，7-Zip 自己都失败）**，理由见 §10。

---

## 1. 怎么跑 / 怎么重新生成

```bash
# 全套重生成 + 逐样本验证（约 6~12 秒，覆盖重建 tests/data）
python3 /home/user/hermes/yshjy/tests/make_samples.py

# 只验证已有样本、不重新生成
python3 /home/user/hermes/yshjy/tests/make_samples.py --verify-only

# 只做某几组
python3 /home/user/hermes/yshjy/tests/make_samples.py --only A,B
```

生成 / 验证需要 **Hermes 自带 Python（`~/.hermes/tools/python-3.14.7.../bin/python3`）**，因为只有它装有 `pyzipper`
（`/usr/bin/python3` 没有，隐写者加密模式与读端 zarchiver 路径都需要它）。`mkvmerge` 位置可分别用环境变量
`MKVMERGE` / `MKVEXTRACT` / `MKVINFO` 覆盖。

---

## 2. 环境与工具（本机实测版本）

| 用途 | 工具 | 版本 / 路径 |
|---|---|---|
| 造外壳视频 | ffmpeg | 7.1.5（`/usr/bin/ffmpeg`） |
| 造/验压缩包 | 7-Zip | 25.01（`/usr/bin/7z`） |
| zip 分卷 / ZipCrypto | Info-ZIP zip | 系统 `zip`（`-X` 关掉 extra 字段以保可复现） |
| tar 系 | GNU tar 1.35 + xz/zstd/bzip2/gzip | 系统 |
| **隐写者 mkv 模式所需 mkvmerge** | MKVToolNix | **92.0**，见下 |
| 隐写者 AES | pyzipper | 0.4.0（Hermes Python 3.14.7） |
| 隐写者源码 | SteganographierGUI | `refs/SteganographierGUI/Steganographier.py`（v1.3.11，3847 行） |

**⚠️ mkvmerge 的坑（本机无 root）**：系统没装 MKVToolNix，`sudo` 需要密码，隐写者自带的 `tools/mkvmerge.exe` 是 Windows 二进制跑不了。
解决办法（脚本已内置，探测目录见 `MKVTOOLS_DIRS`）：

```bash
apt-get download mkvtoolnix libebml5 libmatroska7 libflac14 libogg0 libdvdread8t64 libgmp10   # 无需 root
dpkg-deb -x *.deb ~/.local/share/yshjy-tools/mkvtoolnix/                                       # 解包成可执行目录
# 运行时要带 LD_LIBRARY_PATH=<prefix>/usr/lib/x86_64-linux-gnu（脚本会自己按 mkvmerge 路径推导）
```
**已经就位**：`~/.local/share/yshjy-tools/mkvtoolnix/usr/bin/{mkvmerge,mkvextract,mkvinfo}`（v92.0，32MB，放在 scratch 之外，避免被缓存清理删掉）。
脚本的 `MKVTOOLS_DIRS` 会依次找这个目录 → `~/.hermes/cache/scratch/mkvtools/usr/bin` → 系统 `PATH`。
脚本会把 `Steganographier.mkvmerge_exe` 指向它（真跑 mkvmerge，不是伪造 MKV）。
**离线重造此样本需要先重复上面两步**；否则 A5 会生成失败（其余样本不受影响）。
（另：官方 AppImage 走代理只有 ~26 KB/s，不推荐。）

---

## 3. 载荷与素材（`tests/assets/`，不是样本）

| 素材 | 说明 |
|---|---|
| `payload/` | 载荷：`说明.txt`、`中文目录/文件1.txt`、`中文目录/sub/空格 文件名.bin`、`random.bin`、`empty.txt`。**含中文名/中文子目录/空格名/空文件**，未压缩共 412,000 B。`random.bin` 与 `空格 文件名.bin` 各 200KB 不可压缩随机数（保证分卷真被切开）。 |
| `payload.zip` | 上面的普通 ZIP（B 组当「内芯」用）。 |
| `cover/cover.mp4` | 3 秒 testsrc，mpeg4，13,900 B，原子链 `ftyp/free/mdat/moov`。 |
| `cover/cover_fast.mp4` | 3 秒，`+faststart`（`ftyp/moov/free/mdat`）。 |
| `cover/cover2.mp4` | 2 秒 smptebars，4,607 B（给 A2 换一个外壳）。 |
| `cover/cover3.mp4` | `cover.mp4` + 4 个空 `free` 原子（顶层原子变多 → 隐写者 zarchiver 噪声变多，**用来突破 64KB 尾部窗口**）。 |
| `cover/cover_big.mp4` | 5 秒 320×240，mdat 85 KB（B4 用，使 ZIP 之后仍有 >64KB 数据）。 |
| `cover/cover.png` / `cover.jpg` | 96×54 一帧，B3 用。 |

**可复现性**：载荷用固定随机种子生成，并把所有文件的 **mtime/atime 钉死**（`FIXED_MTIME=1750000000`）。
不钉 atime 的话 Info-ZIP 的 UT extra 字段会把 atime 写进包，同一份载荷两次跑出来的 zip 字节就不一样（实测 `.z01` 会差几个字节）。
zip 命令统一加 `-X`（不写 extra 属性）进一步保证稳定。

---

## 4. 样本总表

> 大小列是最后一次生成的值。A 组与 C8/C9/C10/C10b 含随机数，大小/哈希每次重跑会变（见 §12）。
> 「能解」= 实测过的工具与返回码：`7z x` rc=0、`7z t` rc=0、`ffmpeg -f null -` rc=0、`mkvextract`、`zipfile/pyzipper`。

| 文件 | 大小 (B) | 造法 | 期望结构 | 用什么能解（实测） |
|---|---|---|---|---|
| `A1_steg_modeA_plain.mp4` | 424,759 | 隐写者真源码 `hide_file`，模式 `mp4`，无密码 | `[cover.mp4 13900B][free头8B][随机pad 64~256B][ZIP]`，ZIP 尾接文件尾 | `7z x` rc=0（5 文件/412,000B）、`7z t` rc=0、ffmpeg rc=0、python zipfile OK |
| `A2_steg_modeA_ascii_pwd.mp4` | 417,824 | 同上，外壳 cover2，密码 `P@ssw0rd!2026` | 同上，ZIP 为 **AES-256/AE-2** | `7z x -p` rc=0、pyzipper OK；ffmpeg rc=0；**错密码 rc=2** |
| `A3_steg_modeA_cn_pwd.mp4` | 468,387 | 同上，外壳 faststart，**中文密码 `demo-password`** | 同上（ZIP 内条目乱序、时间戳随机） | `7z x -pdemo-password` rc=0、pyzipper OK；ffmpeg rc=0 |
| `A4_steg_modeB_zarchiver.mp4` | 467,676 | 模式 `mp4(zarchiver)` | `[ftyp28][free(内含ZIP，无pad)][原原子链，stco 只补了 free 长度][尾部噪声 J1..J3]`；ZIP 起点 = 文件第 **36** 字节 | `7z x` rc=0；**ffmpeg rc=183（视频已写坏：moov atom not found）** |
| `A4b_steg_modeB_tailnoise_gt64k.mp4` | 544,590 | 同上，外壳换成 cover3（原子多 → 噪声多） | 同上，但**尾部噪声 >115KB（实测 116~134KB）> 64KB** | `7z x` rc=0；但 **python zipfile 打不开（BadZipFile）**，必须走 free 原子路径 |
| `A5_steg_mkv_mode.mkv` | 438,121 | 模式 `mkv`（真 mkvmerge 92.0） | MKV，**2 个附件**：附件1=ZIP、附件2=8KB 随机 | `mkvextract attachments` rc=0 → `out1`(≈410.7KB,Type=zip) / `out2`(8192B) |
| `A6_oldstyle_bare_append.mp4` | 436,724 | **手工复刻老版模式A**（报告 §3.1） | `[cover][裸ZIP][假签名+5~10KB随机]×2[假mdat][空mdat]` | `7z x` rc=0、ffmpeg rc=0、EOCD 距文件尾 12,332B |
| `B1_tail_append_cat.mp4` | 424,410 | `cat cover.mp4 payload.zip`（copy /b 式） | `[cover 13900B][ZIP]`，无 pad、无 free | `7z x` rc=0、ffmpeg rc=0、zipfile OK |
| `B2_apate.mp4` | 410,542 | apate 规则手工构造 | `[面具头28B][原文件剩余][逆序的原始头 28B][小端 N=28]`，**不是 ZIP** | 只能靠 apate 还原逻辑：末 4B N=28 → 逆序还原 → 得到完整 ZIP（zipfile OK） |
| `B3_png_tail.png` | 410,975 | `cat cover.png payload.zip` | PNG `IEND` 之后追加 ZIP | `7z x` rc=0；IEND 之后确有数据 |
| `B3_jpg_tail.jpg` | 413,332 | `cat cover.jpg payload.zip` | JPEG `FF D9` 之后追加 ZIP | `7z t` rc=0；EOI 之后确有数据 |
| `B4_free_atom_interior.mp4` | 496,784 | 手工构造**正确版** zarchiver | `[ftyp][free(内含ZIP，无pad)][free8][mdat 85054][moov]`，**stco 精确 +410,518** | `7z x` rc=0、**ffmpeg rc=0（可播！）**；python zipfile 打不开（ZIP 后面还有 86KB） |
| `C1_plain.zip` | 411,028 | `7z a -tzip` | 普通多文件 ZIP | `7z t/x` rc=0（5 文件） |
| `C2_plain.7z` | 410,299 | `7z a -t7z` | 普通 7z | `7z t/x` rc=0 |
| `C3_plain.tar` | 419,840 | `tar -cf` | 普通 tar | `tar -tf` rc=0（8 成员，含目录项） |
| `C4_plain.tar.gz` | 410,427 | `tar -czf` | gzip 压缩 tar | `tar -tzf` rc=0 |
| `C5_plain.tar.xz` | 412,388 | `tar -cJf` | xz 压缩 tar | `tar -tJf` rc=0 |
| `C6_plain.tar.zst` | 410,101 | `tar --zstd -cf` | zstd 压缩 tar | `tar --zstd -tf` rc=0 |
| `C7_plain.tar.bz2` | 412,789 | `tar -cjf` | bzip2 压缩 tar | `tar -tjf` rc=0 |
| `C5b_single.xz` | 144 | `xz -6 -c 说明.txt` | **单流** xz（非 tar） | `xz -dc` rc=0（520B） |
| `C6b_single.zst` | 77 | `zstd -19` | **单流** zstd | `zstd -dc` rc=0（520B） |
| `C7b_single.bz2` | 157 | `bzip2 -9 -c` | **单流** bzip2 | `bzip2 -dc` rc=0（520B） |
| `C8_zipcrypto.zip` | 410,924 | `zip -e -P zipcrypto123` | **ZipCrypto**（flag bit0=1，**无** 0x9901 extra） | `7z x -p` rc=0；`unzip -P` rc=0；错密码 rc=2 |
| `C9_aes256.zip` | 411,278 | `7z a -tzip -mem=AES256 -pAES256asciiPwd` | **AES-256/AE-2**：local header method=99、extra `0199070002004145030000`（version=2, vendor=AE, strength=3→AES-256） | `7z x -p` rc=0、pyzipper OK；错密码 rc=2 |
| `C10_pwd_cn.7z` | 410,339 | `7z a -t7z -pdemo-password2026` | 7z AES-256（**文件名未加密**） | `7z x -p中文密码` rc=0；错密码 rc=2 |
| `C10b_pwd_cn_mhe.7z` | 410,368 | 同上 `-mhe=on` | 7z AES-256 **+ 头加密（文件名也藏起来）** | `7z x -p中文密码` rc=0；错密码 rc=2 |
| `C11_split.7z.001` + `.002` | 307,200 + 103,099 | `7z a -v300k` | 7z 分卷，**裸字节切分**（`.002` 无签名） | `7z t/x C11_split.7z.001` rc=0（5 文件）；必须两卷都在 |
| `C12_split.z01` + `.zip` | 307,200 + 103,588 | `zip -s 300k -r` | ZIP 分卷（`.z01` 在前，`.zip` 收尾带 CD） | `7z t/x C12_split.zip` rc=0（5 文件） |
| `D1_truncated.zip` | 410,210 | 普通 ZIP 去掉尾部 300B | **EOCD 丢失**，CD 残缺，5 个 local header 完好 | `7z x` rc=2 但**仍解出 5 文件**（自带 local header 扫描）；python zipfile **打不开**；本地 rescue 逻辑解出 5/5 |
| `D2a_eocd_cd_offset_tampered.zip` | 410,510 | 只把 EOCD 的 `cd_offset` 从 410,128 改成 2,765,328（**+2355200**） | CD 能列出，**成员数据读不出** | python zipfile：能 list，`read()` 报 `OSError Errno 22`；`7z x` rc=2（仍解出 5 文件）；rescue 解出 5/5 |
| `D2b_prefixed_2355200.zip` | 2,765,702 | 2,355,200B 垃圾 + 完整 ZIP（**统一偏移差**，对应我们 8.7GB 实测症状） | 所有 offset 差 2,355,200 | **`7z x` rc=2 / 0 个文件（7-Zip 也失败！）**；python zipfile OK（自动 concat）；本地 rescue 解出 5/5；`concat` 算得 2,355,192、`CD@concat = 504b0102` ✓ |
| `D3_missing_volume.7z.001` | 307,200 | 切成 2 卷后删掉 `.002` | 缺卷的 7z | `7z t` rc=2：`Cannot open the file as [7z] archive / Unexpected end of archive` |
| `D4_corrupt_member.zip` | 410,510 | `random.bin` 的**数据段 XOR 16 字节**（CD 完好） | 解压能完成但 CRC 不符 | `7z t` rc=2 `CRC Failed : payload/random.bin`；`7z x` rc=2 **但仍写出 5 个文件**（其中 random.bin 内容错）；python `testzip()` → `payload/random.bin` |

---

## 5. A 组详解（隐写者真源码，非仿造）

**造法**：`make_samples.py` 用 `exec` 加载真源码（`refs/SteganographierGUI/Steganographier.py`，CRLF→LF、切掉 `__main__`），
只对本机缺失的 `hachoir` / `natsort` 打桩，然后直接调 `Steganographier.hide_file(...)`。
**产物字节全部来自真代码，没有任何逻辑重写**。

**A1/A2/A3 实测（模式 A）**：
- `tail_math.concat`（= EOCD 位置 − size_cd − offset_cd）**与「第一个 local header 的绝对位置」完全相等**（实测分别为 ~13.9K / ~4.8K / ~55.3K；每次因 64~256B 随机 pad 略有浮动）→ 结构就是 `[外壳][free头][pad][ZIP]`。
- EOCD 距文件尾固定 **217 B**（= 22 + 195B 注释；注释内容是「SHA-256 + 时间戳 + 时间戳哈希」，注释长度随名字长度变）。
- ffmpeg 解码 **rc=0**（视频照常播放）——因为模式 A 一个字节都不改外壳。
- 7z 能当普通 zip 打开并整包解出（`7z x` rc=0，5 文件 / 412,000 B）。
- A2/A3 用 pyzipper 验证：进包需要密码，错密码 rc=2。
- **A3 的中文密码是写端直接传的 UTF-8 密码**（`password.encode('utf-8')`），`7z x -pdemo-password` 能解 → 说明中文密码在「用户直接输入」这条路上是通的。
  **坑**：隐写者读端密码本 `load_passwords` 会 `encode('ascii','ignore')` 把非 ASCII 密码静默削掉，所以**密码本里的中文密码永远失败**；我们做密码候选时不能复刻这个 bug。

**A4 / A4b 实测（模式 B / zarchiver）**：
- ZIP 起点固定在**文件第 36 字节**（ftyp 28 + free 头 8），ZIP 内部**无 pad**，原子载荷第一个字节就是 `PK\x03\x04`。
- **ffmpeg 两种外壳都 rc=183（不可播）**：非 faststart 报 `moov atom not found`。这就是报告里那个「噪声写入被过度缩进进 `while` 循环」的 bug 的现场证据。
- `A4` 的尾部噪声实测 **约 63~66 KB**（3 个噪声块），**正好压在 64KB EOCD 搜索窗口的临界点上** →
  有时「从尾部找 EOCD」能蒙到、有时蒙不到（两次重跑分别命中 `MP4文件末尾ZIP提取` 和 `MP4 ZArchiver模式提取`）。
- `A4b` 换用 cover3（顶层原子 8 个 → 7 个噪声块），尾部噪声 **约 116~134 KB**，**python zipfile 直接 `BadZipFile`**，
  只能靠「遍历顶层原子找 free」——这一条是稳定、可重复的硬样本。

**A5 实测（mkv）**：`mkvinfo` 看到 2 个附件，MIME 分别是 `application/zip` 与 `application/octet-stream`；
`mkvextract attachments f.mkv 1:out1 2:out2` → `out1` ≈410.7KB（Type=zip）、`out2` 8,192 B。**附件 1 才是载荷，附件 2 是 8KB 混淆垃圾**。

**A6 说明**：隐写者 1.3.11 已经**不再**生成「裸 ZIP 尾部追加」的形态（它改成了 free 原子），
但线上还有大量老版产物，所以按报告描述手工复刻了一份：`[外壳][裸ZIP][假签名+随机]×2[假 mdat][空 mdat]`。
它和 A1 的区别：A1 的 ZIP 前面有 free 头 + 随机 pad，A6 是裸接；两者 EOCD 都在文件尾附近（A6 距尾 12,332 B）。

---

## 6. B 组详解（手工 / 其它工具）

- **B1（copy /b）**：`concat`=13,900 = 外壳大小，逐字节符合 `[外壳][ZIP]`。ffmpeg rc=0、7z rc=0、python zipfile OK。这是全网「图种」教程的标准形态。
- **B2（apate）**：按已逆向的规则构造 —— `[面具头(取 cover.mp4 前 28B 的 ftyp)][原文件剩余][逆序的原始头 28B][小端 int32 N=28]`。
  - 验证：读末 4B 得 **N=28**；`[len-4-N, len-4)` 逆序后是 **`50 4b 03 04 ...`（合法 ZIP 头）**；逆序还原后是**完整可解 ZIP**（zipfile OK，5 成员）。
  - **关键点**：apate 产物**不是 ZIP**，任何「改后缀 / 找 EOCD」的办法都没用。
  - **实测反证**：用隐写者自己的读端 `reveal_file` 跑 B2 → 依次试了 `7-Zip高速解压 / MP4文件末尾ZIP提取 / MP4 ZArchiver模式提取 / MP4 free原子方法`，**全部失败、一个文件都没写出**。这正是我们相对隐写者的差异化点。
- **B3（PNG/JPG 尾部追加，PolyZip/图种风格）**：`IEND` / `FF D9` 之后确实还有数据；`7z t/x` rc=0 能整包解出。
  ⚠️ 隐写者读端对 PNG 也能"成功"（它用的是与格式无关的尾部路径），所以别以为只有 MP4 才走这条路。
- **B4（手工 free 原子内嵌，正确实现）**：原子链实测 `ftyp(28) → free(410,518) → free(8) → mdat(85,054) → moov(1,176)`，`stco` 精确加了 410,518。
  - **ffmpeg rc=0 —— 可正常播放**。与 A4/A4b 对照，直接证明「zarchiver 模式的坏文件是那个缩进 bug 造成的，不是这个布局本身的问题」。
  - ZIP 之后还有 86 KB 数据 → **python zipfile 失败**，必须走 free 原子扫描；隐写者读端命中 `MP4 ZArchiver模式提取`。

---

## 7. C 组详解（普通包）

- C1~C7 覆盖 ZIP/7z/TAR/tar.gz/tar.xz/tar.zst/tar.bz2，全部 `rc=0` 且能解出 5 个载荷文件（412,000 B）；
  tar 系看到 **8 个成员**（含 4 个目录项）。
- C5b/C6b/C7b 是**单流** xz/zstd/bzip2（不是 tar），用来覆盖「单文件压缩」这条解压路径（解出 520 B 的 `说明.txt`）。
- **C8 ZipCrypto**：local header `flags=0x809`（bit0=加密位，method=8 deflate，**没有 0x9901 extra**）→ 传统 PKWARE 加密。
  错密码 rc=2；正确密码 `7z x` rc=0、`unzip -P` rc=0。**我们的 zip 实现必须开 `legacy-zip` 才能解它。**
- **C9 AES-256 zip**：local header `method=99`、`flags=0x1`、extra `01 99 07 00 | 02 00 | 41 45 | 03 | 0000`
  → `0x9901`、version=`0x0002`（**AE-2**）、vendor=`AE`、strength=`0x03`（**AES-256**）、real_method=0/8。
  pyzipper 能进包（8 成员）；7z 也能解。⚠️ 注意 7-Zip 把 `0x9901` 只写在 **local header**，中心目录里没有——
  只读 CD 的解析器会误判成「未加密」。
- **C10 / C10b 7z 加密**：中文密码 `demo-password2026`；`-mhe=on` 版本连文件名都加密（不输密码连清单都看不到）。
  两个都实测 `7z x -p中文密码` rc=0、错密码 rc=2。
- **C11 7z 分卷**：`.001`(307,200) + `.002`(103,099)，**`.002` 连签名都没有、就是裸字节续接**。
  `7z t/x C11_split.7z.001` rc=0（注意：**只有 .001 是入口**，点 .002 会报错）。
- **C12 ZIP 分卷**：`.z01` + `.zip`，`.zip` 是最后一卷（含 CD/EOCD）。`7z x C12_split.zip` rc=0。

---

## 8. D 组详解（异常/损坏包 —— 救援模式用）

- **D1 截断 ZIP**（砍掉尾部 300B → EOCD 没了）：`7z t` rc=2 `Open Errors: 1`；python `zipfile` **直接 BadZipFile**；
  但 **5 个 local header 与数据都完好**，用「按 local header 顺序扫描 + CRC32 校验」的救援逻辑**解出 5/5**。
  ⚠️ 实测 `7z x` 虽然 rc=2，**但照样写出了 5 个文件** —— 说明 7-Zip 内部就在做 local header 救援；
  **我们的正确行为应该是「解出来 + 明确告知报错/不完整」，而不是因为 rc≠0 就当作彻底失败。**
- **D2a EOCD 的 cd_offset 被改**（410,128 → 2,765,328，+2,355,200）：这是**「目录能列、数据读不出」**的形态。
  python zipfile 能 `namelist()`，但 `read()` 报 `OSError: [Errno 22] Invalid argument`；`7z t` rc=2 `Sub items Errors: 5`。
  救援逻辑（local header 扫描）解出 5/5。**这是我们「CD 偏移不可信」时最该修的场景。**
- **D2b 前置 2,355,200B 垃圾的完整 ZIP**（= 我们实测的 8.7GB 案例形态）：
  - **`7z x` rc=2、解出 0 个文件** —— **7-Zip 对 2.3MB 级前置垃圾也是失败的**（它找不到自洽的 CD 位置）。
  - python zipfile **能**打开并读出正确内容（它用 `concat = EOCD位置 − size_cd − offset_cd` 自动补偿）。
  - 我们脚本独立算出的 `concat = 2,355,192`，且 `CD@concat = 504b0102`（`PK\x01\x02`）✓ —— 验证了「统一偏移修正」这条路是对的。
- **D3 缺分卷**：`7z t` rc=2，报 `Cannot open the file as [7z] archive / Unexpected end of archive`。
  ⚠️ 7-Zip 的措辞是「归档结尾异常」而不是「需要下一分卷」，**用户拿到的提示词和真实原因不一致**——我们的报错要更准（缺卷 vs 损坏）。
- **D4 内部成员损坏**：`random.bin`（200KB 不可压缩随机数，deflate 里是 stored block）被 XOR 16 字节。
  - 现象：**解压能完成、长度不变、但 CRC 不符**。`7z t` rc=2 `CRC Failed : payload/random.bin`；`7z x` rc=2 **但仍然写出 5 个文件**（其中 `random.bin` 内容错误，必须提醒用户）。
  - 测试点：工具**必须报 CRC 错误**，不能静默当成功。

---

## 9. 密码汇总（写进 README 供人工复现）

| 样本 | 密码 |
|---|---|
| `A2_steg_modeA_ascii_pwd.mp4` | `P@ssw0rd!2026` |
| `A3_steg_modeA_cn_pwd.mp4` | `demo-password`（中文，UTF-8） |
| `C8_zipcrypto.zip` | `zipcrypto123` |
| `C9_aes256.zip` | `AES256asciiPwd` |
| `C10_pwd_cn.7z` / `C10b_pwd_cn_mhe.7z` | `demo-password2026`（中文） |

---

## 10. 哪些样本最能暴露我们的缺陷（按优先级）

1. **`A4b_steg_modeB_tailnoise_gt64k.mp4`** —— **头号杀手**。
   尾部噪声 >115 KB，**「从文件尾 64KB 找 EOCD」这条最省事、最常用的路径直接失效**（python zipfile 报 BadZipFile）。
   必须真的去**遍历顶层原子、找 `free` 原子、在原子体内搜 `PK\x03\x04`**。
   而且 ZIP 前面**没有 pad**（第 36 字节就是 `PK`），所以「原子体内搜签名」也要处理「原子载荷第一个字节就是 PK」。
   *（`A4` 是它的弱化版：噪声 63~65KB 卡在窗口边界，时灵时不灵——**不要只拿 A4 测**，会误判成"过了"。）*
2. **`D2b_prefixed_2355200.zip`** —— **连 7-Zip 都解不出**（rc=2，0 文件）。
   唯一出路是正确实现 `concat = EOCD位置 − size_cd − offset_cd` 并把每个 local header / CD 项偏移补上（或整段切掉前缀后再解析）。
   这正是我们从 8.7GB 真实文件里踩出来的那个「CD 偏移差 2355200」。
3. **`B2_apate.mp4`** —— 隐写者自己都解不了（读端 4 种方法全灭）。要做「末 4B 小端 N + `[len-4-N,len-4)` 逆序验 magic」的独立判据。
4. **`D2a_eocd_cd_offset_tampered.zip`** —— 「能列目录、读数据报 Errno 22」的形态。**任何信任 `cd_offset` 的解析都会在这里挂**，必须能退化到 local header 扫描。
5. **`B4_free_atom_interior.mp4`** —— 同一套 free 原子逻辑，但**要求我们既解得出、又不能把合法封面视频判成损坏**（它 ffmpeg rc=0）；同时它可用来验证我们改写/校验 `stco` 的正确性（δ=410,518）。
6. **`C9_aes256.zip` / `C8_zipcrypto.zip`** —— 加密判定：`0x9901` extra **只在 local header**（C9），ZipCrypto 只有 `flags` bit0（C8）。两种都要能识别并分别走 AES / 传统解密。
7. **`A5_steg_mkv_mode.mkv`** —— MKV 附件里**有两个**，附件 2 是 8KB 垃圾；盲目取「第一个附件」能过，但取「最大的附件的 MIME」之类启发式会踩坑，必须解析 Attachments 元素 + 附件名/MIME。
8. **`D4_corrupt_member.zip`** —— 「解压成功但 CRC 不符 + 文件已写出」的三态，最容易写成「静默当成功」。
9. **`D1_truncated.zip` / `D3_missing_volume.7z.001`** —— 报错语义：**缺卷 ≠ 损坏**，且 `7z x` 在 rc≠0 时**可能已经解出了一部分**（D1/D2a/D4 都是 rc=2 但写出 5 个文件）。

---

## 11. 可信度警告 / 已知坑（踩过的）

1. **别信 `7z -slt` 的 `Offset` 字段**。伪装包上它给的值没有意义（例：A1 报 205,190，而真实 ZIP 起点约 14,050）。
   判偏移请用 `concat = EOCD位置 − size_cd − offset_cd`（我们脚本就是这么算的）。
2. **`7z` 的 rc 不能当「有没有解出东西」的判据**。D1/D2a/D4 都是 `rc=2` 但**写出了全部 5 个文件**。
   必须**对比输出目录前后快照**（隐写者的 `snapshot_output_files` 思路是对的）。
3. **隐写者读端的 7-Zip 优先路径在本机必然失败**：它找的是 `application_path/tools/7z.exe`（Windows 二进制，文件确实存在但没有 wine），
   所以 `reveal_file` 的日志里 `7-Zip高速解压` 永远不命中。**这是我们探针环境的产物，不是样本的性质。**
4. **A4 的 64KB 判定不稳定**（噪声 63~65KB 浮动），只有 A4b 是稳定的硬样本。别用 A4 做回归判定。
5. **A 组与 C8/C9/C10/C10b 的字节每次重跑都不同**（隐写者的随机 pad/条目乱序/时间戳注释、ZipCrypto 的随机加密头、AES 的随机盐/IV）。
   A 组**大小**也会有 ±200B（A1/A2/A3）到 ±40KB（A4/A4b）的浮动 → **断言要写在结构层面（EOCD 距离、concat、PK 位置、附件数），不要写死 sha256/文件大小。**
6. **B3 的 PNG/JPG 不要以为要专门写图片解析**：它和 MP4 尾部追加是同一件事（尾部找 EOCD），但**判「IEND/EOI 之后是否还有数据」**对识别「这是伪装图」很有用。
7. **A5 依赖外部 mkvmerge**（见 §2）。没有它，A5 无法重新生成（已有文件仍可用）。

---

## 12. 可复现性（重跑两次对比实测）

| 类别 | 字节可复现？ |
|---|---|
| C1~C7、C5b/C6b/C7b、C11、C12、D1、D2a、D2b、D3、D4、B1~B4 | ✅ **完全一致**（24/35 个文件按 sha256 逐字节相同） |
| A1~A6 | ❌ 每次不同（隐写者内部随机化；mkvmerge 也会写随机 UID） |
| C8 / C9 / C10 / C10b | ❌ 每次不同（ZipCrypto 随机加密头 / AES 随机盐与 IV） |

造这 24 个稳定文件的关键：固定载荷随机种子 + **同时钉死 mtime 和 atime** + `zip -X`。
（只钉 mtime 是不够的：Info-ZIP 的 UT extra 会写 atime，导致 `.z01` 字节漂移。）

---

## 13. 未覆盖 / 有意不做

- **隐写者模式 B 的 faststart 外壳变体**：报告里提过（`header damaged`, rc=69），本轮只做了非 faststart 的 A4/A4b 与「正确版」B4。
- **真实大文件**（>4GB / ZIP64 EOCD64 + locator）：本轮全部样本 ≤ 2.8MB，**ZIP64 tail 路径、`0xFFFFFFFF` 占位符、EOCD64 locator 的分块扫描都没有样本覆盖**（这是已知缺口，建议后续补 1~2 个 ZIP64 样本）。
- **RAR 样本**：项目定的是「只解不压」，本机也造不出 RAR（无 rar 工具、不做打包），**C 组里没有 RAR**。
- **RAR 分卷**（`.part1.rar/.r00`）、**`.7z.000` 开头分卷**、**文件名插字伪装**（`movie.7z.00删1`）：属解压器功能而非隐写，本轮未造。
- **像素/DCT/LSB 级隐写**（FileImgSwap 的 `.fis.png`、VidCrypt 等）：无标准格式，明确不支持，不造样本。
- **A5 之外的 MKV 变体**（附件名不带 `.zip`、WebM 外壳）：未造。
- **`-mhe=on` 之外 7z 的「头加密 + 分卷」组合**：未造。

**入 git 建议**：`tests/.work/`（临时目录）应 ignore；`tests/data/`（15MB，其中 11 个文件每次重跑字节都不同）与 `tests/assets/`（1MB，可重建）是否入库自行决定，反正 `make_samples.py` 能一键重造。

原始分析依据见 `refs/analysis/`（`steganographier-hide.md`、`fake-archive-ecosystem.md`、`01-格式总矩阵.md` 等）。
