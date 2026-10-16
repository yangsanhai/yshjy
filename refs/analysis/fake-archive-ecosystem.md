# 伪装包 / 隐写包 生态调研

> 产出：2026-10-16　调研人：Hermes 子代理（unbox 项目）
> 目标：摸清「伪装成视频/图片的压缩包」的完整生态（造包工具 / 输出变体 / 说明模板花样），
> 为 unbox「什么包都能解」划定兼容边界。
> 已有基础：`refs/analysis/`（隐写者源码分析 6 份）+ `refs/README.md`（8.7GB 实测）。

---

## 〇、结论速览（先看这个）

1. **中文圈的事实标准只有一个：隐写者（SteganographierGUI）**。B 站/南+/绅士仓库/ACG 站的解压教程
   几乎全部照抄它的官方 Wiki。它自己承认的三个模式（mp4 / mkv / mp4(zarchiver)）覆盖了 90% 的真实资源。
2. **中文圈还存在一整类「不是隐写、只是改后缀/改文件名」的伪装箱**——这类才是数量最多的（`movie.7z.001`
   后面加个「删」字、`.zip` 改成 `.jpg` 上传）。已有多个中国开发者做过工具（rozx / TR-Supowe / ccloli /
   Butter-unpacker），**它们是 unbox 的直接竞品，不是造包工具**。
3. **apate 是唯一的「另一套机制」造包工具**，也是隐写者官方明确点名「普通解压软件解不了」的那一个。
   它的格式**已被我们完全逆向清楚**（见 §3.2），**可以兼容**。
4. 国际圈的工具（PolyZip / pdvzip / PolyFlix / gp_disguise / mp4vault…）加起来 30+ 个，但**关键只有 4 类结构**：
   尾部追加、容器原子/分块内嵌、附件轨、像素/采样编码。**前两类必须支持，第三类必须支持，第四类只能识别不能解**。
5. ⚠️ **最大的坑**：隐写者的 zarchiver 模式会**故意往文件里塞假的 RAR/ZIP/GZIP/BZIP2/XZ 签名 + 5~10KB 随机字节**
   （源码 L2301-2320），专门诱导 `binwalk` 式扫描器误报。**我们的扫描器不能「见到签名就当包」，必须以
   EOCD 能否成功打开为准。**

---

## 一、工具盘点（共 **37** 个，分 5 类）

### 1.1 中文圈 · 造包工具（重点）

| # | 名称 | 仓库/来源 | 语言/平台 | 机制一句话 | 输出识别特征 | unbox 能否兼容 |
|---|------|-----------|-----------|-----------|-------------|---------------|
| 1 | **隐写者 SteganographierGUI** | github.com/cenglin123/SteganographierGUI | Python / Win、跨平台 CLI | 见 §3.1，三模式 | 见 §3.1 | ✅ **必须** |
| 2 | **apate** | github.com/rippod/apate | C# / Win | 换文件头 + 尾部存"逆序原始头 + 面具长度" | 末 4 字节 = 面具长度 N；`[len-4-N, len-4)` 反转后是合法 magic | ✅ **可兼容**（格式已完全逆向，§3.2） |
| 3 | **影藏 PolyFlix** | github.com/hcllmsx/PolyFlix | Python+FastAPI，Win/Android 播放器 | MP4 外壳 + ZIP polyglot；外层可 AES-256（pyzipper），内层可 7z；往 `moov` 写 `©cmt` 备注 | 改 `.zip` 可解；moov 内有 `©cmt` 制作信息 | ✅ 同 1 的尾部/free 路径 |
| 4 | **PhantomMan** | github.com/ew46883-collab/PhantomMan-Steganography-… | Python + HTML/JS | **隐写者克隆**（README 明说 inspired by cenglin123），把 ZIP 放进 MP4 的 "hide box"，声称兼容 Bandizip/WinRAR | 同隐写者模式 A | ✅ 同 1 |
| 5 | **FileImgSwap 文图变** | github.com/fzxx/FileImgSwap | 单文件 exe | ①文件数据编码进 PNG 像素；②`-append` 把 zip/rar **追加到 PNG 尾部**（"图种"），产物 `.fis.png`/`.tz.png`；AES-CTR + PBKDF2/HKDF，密码在**同目录同名** `.fisp.txt`/`.fisp.key` | `.tz.png` 尾追加型可直接解；`.fis.png` 是像素编码，需专用解 | ⚠️ 追加型 ✅；像素型 ❌（需其算法） |
| 6 | **坏坏包 NaughtyDamagePack** | github.com/fzxx/NaughtyDamagePack | 单文件 exe | "让压缩包像坏了"——**人为破坏/伪装成损坏的压缩包**（属同一作者的防炸链工具箱） | 解压报损坏 | ❌ 属误导性伪装，非容器 |
| 7 | **鸭鸭图 duck-image-tool** | github.com/missolin/duck-image-tool | 中文，图/视频编解码+合并 | 媒体内容保护，图片/视频编码解码 | 待细查（0 star 新项目） | ⚠️ 低优先 |
| 8 | **copy /b 手工拼接（图种）** | 无仓库，全网教程 | Windows cmd | `copy /b a.jpg + b.zip c.jpg` | JPEG 尾追加 ZIP；改 `.zip` 或用 WinRAR 解 | ✅ **必须**（底层就是尾追加） |
| 9 | **压缩包伪装器** | mydown.com 等下载站 | Win GUI | 选一张图 + 选一个压缩包 → 输出拼接文件 | 同上 | ✅ |
| 10 | **图种生成器（Quicker 动作）** | getquicker.net | Quicker 脚本 | PNG/JPG + 压缩包 | 同上 | ✅ |
| 11 | **二进制→像素图像（教程流派）** | hxcy.top/541610.html、cangku.moe 系列 | 手工/脚本 | 把文件二进制当像素画成"看起来毫无意义的图" | 图片本身是数据；需按其宽高/编码还原 | ❌ 无标准，需用户提供参数 |
| 12 | **8-bit PCM → WAV/FLAC/MKV（教程流派）** | 隐写者 README 附录 A.3 | 手工 | 文件当 8bit PCM 封装成 WAV，再转 FLAC 或混流进 MKV | 音频是噪声；需专用解码 | ❌ 同上 |
| 13 | **mkvtoolnix 手工夹带** | 官方；隐写者 README 附录 A.2 | 跨平台 CLI | `mkvmerge -o out.mkv in.mp4 --attach-file in.7z` | MKV 附件轨 | ✅ **必须**（见 §3.1-C） |
| 14 | **XiangYue 想曰 / FlashErase 闪电擦除** | github.com/fzxx | Win | 文本加密 / 不可恢复擦除（**弱相关**，属同一"防审查"工具箱） | — | ➖ 无关 |

> 来源：[cenglin123/SteganographierGUI](https://github.com/cenglin123/SteganographierGUI)、
> [rippod/apate](https://github.com/rippod/apate)、[hcllmsx/PolyFlix](https://github.com/hcllmsx/PolyFlix)、
> [fzxx/FileImgSwap](https://github.com/fzxx/FileImgSwap)、隐写者 README 附录 A。

### 1.2 中文圈 · 解包工具 / 直接竞品（要研究的对手）

| # | 名称 | 仓库 | 它支持什么 | 对我们的意义 |
|---|------|------|-----------|-------------|
| 15 | **rozx/Complex-unzip-tool-v2** | github.com/rozx/Complex-unzip-tool-v2 | ①**文件名解伪装**（`movie.7z.00删1` → `movie.7z.001`，规则可配 `config/cloaked_file_rules.json`）②分卷重组 `.001/.002`、`.part1`、`.rar/.r00`、`.zip/.z01` ③密码本 `passwords.txt` + 自动记忆 ④**编码自适应 UTF-8/GBK/GB2312/Big5/UTF-16** ⑤递归嵌套解压 ⑥内置 7-Zip 26.03 ⑦失败自动撤销重命名 | ⭐ **最接近我们的对手**。它做了"文件名解伪装 + 编码自适应"，但**不会解隐写包**（不处理 MP4/free atom）。差异化点仍在 |
| 16 | **TR-Supowe/Complex-Unzip-Tool** | github.com/TR-Supowe/Complex-Unzip-Tool | 递归深度解压、多层密码、后缀伪装识别（.pdf/.txt 其实是压缩包）、**分卷智能补全**（用 .002 反推被改名的 .001）、失败隔离 `_failed_archives`、密码库 0_temp/1_all/2-9 | 同上；v2 就是它的衍生版 |
| 17 | **ccloli/baidupan-zip-extract** | github.com/ccloli/baidupan-zip-extract | 专治**百度网盘下载的 >4GB「损坏」ZIP**：CD 里 offset/size 溢出时 **逐个 +2³² 试** 找到真实位置 | ⭐ 这正对应我们实测的「CD 偏移差 2355200 / ZIP64 标志」问题，可直接借鉴 2³² 修补思路 |
| 18 | **MzyCf/Butter-unpacker** | github.com/MzyCf/Butter-unpacker | GUI，检测 `.7z删`/`.zip备份` 等异常后缀→自动修正，调 Bandizip 解压，递归，密码管理 | 说明"异常后缀"是普遍现象 |
| 19 | **解TMD压** | 绅士仓库 cangku.moe/archives/216302（需登录） | Android，**从头扫压缩包**的解压器（隐写者 zarchiver 模式点名兼容它） | 说明"从头扫"是一整类解压器 |
| 20 | **ZArchiver** | ru.zdevs.zarchiver（Android 事实标准） | 从**文件开头**找归档签名 | 隐写者模式 B 就是为它设计的；它**解不了模式 A** |
| 21 | **解压专家 / RAR for Android** | iOS / Android | 手机端主流；隐写者教程推荐 | 说明模板针对它们写 |
| 22 | **Renamio / BlueZip** | Windows | 读文件头判定真实类型、修正伪装扩展名；BlueZip 还修 CP949/Shift-JIS 乱码文件名 | 与我们「扩展名修正」重叠 |

### 1.3 国际 · 造包工具（多格式 polyglot）

| # | 名称 | 仓库 | 外壳 | 数据位置 | 加密 | 识别特征 | 兼容 |
|---|------|------|------|---------|------|---------|------|
| 23 | **PolyZip** | InfoSecREDD/PolyZip | PNG/JPEG/GIF/PDF/BMP/WebP/TIFF/WAV/MP3/FLAC/OGG/AVI/MKV/WebM/FLV/ICO/CUR/ICNS/MP4/MOV/M4A/EXE/DLL/ELF/MSI/TTF/OTF/WOFF（**30 种**） | PNG→**首个 IDAT 内**（并改长度+CRC）；JPEG/GIF/PDF→EOI/trailer/EOF 后；RIFF 系→chunk 声明长度之外；其余→最后一个合法头/帧/原子之后，**并同步修正 ZIP 偏移** | AES-256-GCM + Argon2id（Scrypt 兜底）或密钥文件 | 改 `.zip` 即可解 | ✅ **必须**（覆盖所有"尾追加"变体） |
| 24 | **pdvzip / pdvzip-rs** | CleasbyCode/pdvzip | PNG | ZIP/JAR 放**最后一个 IDAT chunk**；提取脚本放 **iCCP chunk** | 官方**建议不要加密** | `pzip_*.png`，可 `bash ./img.png` 执行 | ✅ 追加/IDAT 型 |
| 25 | **pdvps** | CleasbyCode/pdvps | PNG | 任意文件类型，先加密再嵌入默认 PNG | 有加密 | 同上 | ✅ |
| 26 | **tweetable-polyglot-png** | DavidBuchanan314 | PNG | IDAT 内 DEFLATE 流 | 无 | 改 `.zip` | ✅ 技术源头 |
| 27 | **Polyglot-HTML-ZIP-PNG** | gildas-lormeau | PNG | 三态（HTML/ZIP/PNG） | 无 | — | ⚠️ 低优先 |
| 28 | **gp_disguise** | xob0t/gp-file-hide | BMP / MP4（为 Google Photos） | **尾部追加**：`[媒体][分隔符 "FILE_DATA_BEGIN"][4B 文件名长][文件名][数据]` | ❌ 不加密（README 明说） | 文件里能搜到字符串 `FILE_DATA_BEGIN` | ✅ 尾追加 + 分隔符可达 |
| 29 | **mp4vault** | nostafobic-dev/mp4vault | 任意视频 | **tail-append**（"cat-append steganography"）；记录原始视频字节数当 **Offset Key** | AES-256-CBC（可选） | 需用户提供 offset key；不提供就得靠猜 | ⚠️ 需 offset；可用容器长度反推 |
| 30 | **bfg9k** | JustinTimperio/bfg9k | PNG / 无损 MKV(PNG codec) | 嵌入 + AES | AES | 需专用解 | ❌ 像素/帧级 |
| 31 | **VidCrypt / StegoTool-Pro / StegaVault** | sourabhkv/VidCrypt 等 | 视频 | 字节→灰度像素→视频帧 | AES | 需专用解 | ❌ 像素级 |
| 32 | **stegobox**（DCTMKV / LSBTXTinMKV） | stegobox.swo.moe | MKV | **DCT / LSB** 隐写在视频帧里 | 无 | — | ❌ 像素级 |
| 33 | **OpenPuff**（老牌商业免费） | 官网 | BMP/JPG/MP3/MP4/FLV/PDF | 多重加密隐写（多密码/多层） | 自有多层密码 | 无公开格式 | ❌ 闭源专有 |
| 34 | **OmniHide PRO / FileVeil** | 商业 / freeware | 图/音/视频 | 专有 | 专有 | — | ❌ 闭源 |
| 35 | **NoClue（Android）/ StegnoX 等** | Play/App Store | 图片 | LSB 类 | — | — | ❌ 像素级 |
| 36 | **steghide / outguess / zsteg / binwalk / foremost** | 经典 CTF 工具 | JPG/WAV/任意 | LSB / 嵌入 / 扫描 | steghide 有口令 | binwalk 能看到尾追加 | ➖ **工具**：binwalk/foremost 会误报（见 §5） |
| 37 | **Polyglot 恶意文件流派**（IcedID 等） | 安全研究 | HTML/ZIP/PNG 多态 | 组合 | — | — | ➖ 背景知识 |

---

## 二、输出变体矩阵

### 2.1 外壳类型 × 数据位置

| 外壳 | 数据位置 | 代表工具 | 数据能否被「改后缀」直接解 | 我们的检测入口 |
|------|---------|---------|--------------------------|--------------|
| **MP4/M4V/MOV** | ① 文件**尾部追加**（裸 ZIP，后面还可能有随机签名 + 假 mdat + 空 mdat） | 隐写者旧版模式A、copy /b、mp4vault、gp_disguise | ✅ 改 `.zip`（WinRAR 能解） | 尾部 64KB+22 找 EOCD → 算 concat |
| MP4 | ② **尾部 free 原子**里（前面垫 64~256B 随机，使原子内部不以 PK 开头） | **隐写者 1.3.x 模式A** | ✅ 改 `.zip` | 同上（EOCD 就在文件尾） |
| MP4 | ③ **ftyp 之后的 free 原子**里（改 stco/co64 补偿） | **隐写者模式B（mp4 zarchiver）** | ⚠️ 只能 `-t#` / zarchiver 从头扫；**WinRAR 解不了** | 遍历顶层原子，找 `free` 内含 PK |
| MKV/WebM | **附件轨（Attachment）** | 隐写者 mkv 模式、mkvmerge 手工 | ❌ 改后缀没用，要 `mkvextract attachments` | 解析 EBML，扫 `Attachment` 元素（ID `0x1941A469`） |
| PNG | ②**首个 IDAT 内**（改长度+CRC） | PolyZip | ✅ 改 `.zip` | 从 IDAT 区搜 PK |
| PNG | **最后一个 IDAT chunk** | pdvzip | ✅ 改 `.zip` | 同上 |
| PNG | **iCCP chunk**（脚本）/ 尾部追加 | pdvzip、FileImgSwap `-append` | ✅ 追加型可解；iCCP 是脚本 | 尾部 IEND 之后有数据 → 追加型 |
| JPG/JPEG | **EOI (`FF D9`) 之后追加** | copy /b、PolyZip | ✅ 改 `.zip` | 找 `FF D9` 之后是否还有数据 |
| GIF | **trailer `0x3B` 之后追加** | PolyZip | ✅ | 找 `3B` 之后 |
| PDF | **`%%EOF` 之后追加** | PolyZip | ✅ 改 `.zip` | 找最后一个 `%%EOF` |
| BMP/WebP/WAV/AVI（RIFF 系） | **声明的 chunk 长度之外追加** | PolyZip、gp_disguise(BMP) | ✅ | 读 RIFF size 字段再比对文件长 |
| MP3/FLAC/OGG/M4A | 最后一个帧/元数据之后 | PolyZip | ✅ | 尾部追加通用检测 |
| TIFF | 最后一个合法头之后 | PolyZip、（中文站 `.tif`→改 `.tar`） | ✅ | 同上 |
| EXE/DLL/ELF/MSI/TTF/OTF/WOFF/ICO/CUR/ICNS | 最后一个合法头之后 | PolyZip | ✅ 部分（Windows 资源管理器/unzip 行；WinRAR 可能报损坏） | 尾部追加通用检测 |
| 任意二进制 | apate 式**换头** | apate | ❌ 需 apate / unbox | 末 4 字节长度 + 逆序头校验 |
| WAV/FLAC/MKV | **8-bit PCM 编码** | 教程流派 | ❌ | 只能识别疑似 |
| 任意 | **像素 / DCT / LSB / PVD** | VidCrypt、bfg9k、stegobox、StegaVault | ❌ | 只能识别疑似 |
| MP4 | **moov `©cmt` 元数据**（写制作备注，不装数据） | PolyFlix | — | 用于**识别制作者**，不用于取数据 |

### 2.2 加密方式矩阵

| 加密 | 出现场景 | 我们能解吗 | 备注 |
|------|---------|-----------|------|
| **无** | 大量中文资源、PolyZip 基础用法、gp_disguise | ✅ | — |
| **ZipCrypto（传统 ZIP 加密）** | 中文圈最常见；隐写者临时 zip | ✅ 需 zip-rs 开 `legacy-zip` feature | **必开**，容易漏 |
| **ZIP AES-256（WinZip AE-1/AE-2）** | pyzipper / 7-Zip `-mem=AES256`；PolyFlix 外层 | ✅ | 我们已有 pyzipper 参考路径 |
| **7z AES-256 + SHA-256（可 `-mhe=on` 加密文件名）** | 中文圈 galgame 资源 | ✅（sevenz-rust2） | 密码枚举 121ms/候选，必须限量 |
| **AES-256-GCM + Argon2id** | PolyZip 加密模式 | ⚠️ 需实现 Argon2id，且格式非标准 ZIP | 低优先 |
| **AES-256-CBC + 自定 key 派生** | mp4vault | ⚠️ 需其脚本约定 | 低优先 |
| **AES-CTR + PBKDF2-SHA256/HKDF-SHA256** | FileImgSwap（密码/密钥文件在**同目录同名**） | ❌ 需其算法 | — |
| **AES（PNG/MKV 帧）** | bfg9k 等 | ❌ 像素级 | — |
| **专有多层密码** | OpenPuff / OmniHide | ❌ 闭源 | — |
| **头加密（AES-CTR）** | apate 自称"原始文件头加密" | ✅ 实测源码里只是**逆序**（`Array.Reverse`），不是真加密 → 更好解 | 见 §3.2 |

### 2.3 分卷

| 分卷形态 | 说明 | 我们的策略 |
|---------|------|-----------|
| `.7z.001/.002/…`、`.zip.001` | 最主流 | 拼流成单个 Read+Seek 即可（**7z 分卷是裸字节切分，`.002` 连签名都没有**） |
| `.part1.rar/.part2.rar` | RAR 分卷 | 拼流 |
| `.rar/.r00/.r01` | 老 RAR 分卷 | 拼流 |
| `.z01/.z02 + .zip` | ZIP 分卷 | 拼流（最后一卷是 .zip） |
| `.7z.000` 开头（0 开头） | 少见；Bandizip 会失败，需 7z | 拼流 + 提示 |
| `PC1/PC2` 等异形 | 中文站自创 | 靠**同前缀 + 递增序号**启发式分组 |
| 伪装字符插进卷号 | `movie.7z.00删1`、`movie.7z.001长` | 需**文件名解伪装**（rozx 已有规则表思路） |
| **隐写者不生成分卷** | 资源里的分卷说明都是发布者自己加的 | 分卷逻辑与隐写解包**解耦** |

---

## 三、逐工具技术细节（重点 3 个）

### 3.1 隐写者 SteganographierGUI（事实标准）

> 源码：`refs/SteganographierGUI/Steganographier.py`（3847 行），已由前序小组分析。
> 本轮新增确认：**1.3.x 版本改了模式 A 的写法**（源码注释 L2125-2136 自述）。

**模式 A「mp4」（-t mp4）**
```
老版： [封面 MP4 完整字节] + [裸 ZIP 原文] + [随机压缩包签名 + 5~10KB 随机] × 2 + [假 mdat 盒] + [空 mdat 8B]
新版： [封面 MP4 完整字节] + [free 原子头(8 或 16B)] + [64~256B 随机 padding] + [ZIP]     ← 原子是文件最后一个原子
```
- 新版刻意**让原子内部不以 PK 开头**、且每次哈希不同（绕迅雷 issue #26 的拦截面）。
- **EOCD 就在文件末尾** → 尾部扫描 + concat 偏移修正能通吃老版/新版。
- 兼容性：改 `.zip` → WinRAR6 / 7-Zip / Bandizip7.36+ 可解；zarchiver 解不了。

**模式 B「mp4(zarchiver)」（-t mp4(zarchiver)）**
```
[ftyp] + [free 原子（内含 ZIP）] + [原第 2 个及以后的原子，stco/co64 已补偿]
        + [随机签名×2 + 5~10KB 随机] + [空 mdat]        ← 尾部噪声（源码缩进 bug，实际插在原子链各处）
```
- 目的：让**从头扫描**的解压器（zarchiver、解TMD压）能编到。
- **WinRAR 解不了**；且**视频已被写坏**（前序小组实测 `moov atom not found`）。
- ⚠️ 尾部噪声里塞的是**真的 RAR/ZIP/7Z/GZIP/BZIP2/XZ 头**（源码 L2302-2320）→ binwalk 会误报。
- 兼容性：只能靠 free 原子直读 / `-t#`。

**模式 C「mkv」（-t mkv）**
```
mkvmerge -o out.mkv cover.mp4 --attach-file in.zip --attach-file <随机 8KB>
```
- 装 **两个附件**（ZIP + 随机字节，第二个是混淆用的）。
- 解：`mkvextract attachments`，或我们直接扫 EBML `Attachment` 元素。

**辅助功能（属于"说明花样"的一部分）**
- `modules/PW.txt` 密码本（Tab 分隔，加载时 **`ascii'ignore'` → 中文密码被吞**，已知 bug）。
- **哈希修改**（防炸链牵连）、**验证码生成器**（`tools/captcha_generator.exe`，把提取码转成验证码图防爬）。
- 右键菜单集成。

### 3.2 apate（唯一的"另一套机制"，**已完全逆向**）

> 源码：`rippod/apate` → `apate/Program.cs`（公开，GPL-3）。三种模式：一键伪装（内置 MP4 面具）、面具伪装（自定义面具）、简易伪装（EXE/JPG/MP4/MOV 纯二进制头）。

**伪装（Disguise）实际做的事**（源码直读）：
```
1. 读出原文件的前 N 字节（N = 面具头长度；若原文件比面具还短，就取原文件全长）→ originalHead
2. 把文件前 N 字节覆盖成面具头（maskHead）
3. 在文件**末尾追加**：ReverseByteArray(originalHead)     ← 逆序，不是加密
4. 再追加 4 字节小端 int：maskHead.Length                 ← 长度标记
```
**还原（Reveal）**：
```
读末 4 字节 → N（面具长度）
读 [len-4-N, len-4) → 逆序还原成 originalHead
截断文件末尾 (N+4) 字节 → 把 originalHead 写回文件头
```
**识别特征（可直接实现）**：
- 末 4 字节按 **小端 int32** 解出 `N`；要求 `4 ≤ N ≤ len-4`（且 `N ≤ 306MB`，源码 `maximumMaskLength = INT_MAX/7`）。
- `bytes[len-4-N : len-4]` **反转**后应是**合法文件头**（`PK\x03\x04`、`MZ`、`7z¼¯'`、`Rar!`、`\x89PNG`、`ftyp` 等）→ 命中即判定为 apate 包。
- 误报率极低（两个条件同时满足）。
- **注意**：apate 的产物**不是 ZIP**，所以「改后缀解压」永远不行——这正是隐写者 Wiki 那句话
  「**不能是 apate 生成的，后者必须使用对应软件才能解除隐写**」的由来。
- ✅ **结论：我们完全可以支持 apate 还原**（不需要 apate.exe），这会是一个差异化卖点。

### 3.3 国际 polyglot 三巨头

- **PolyZip**（最全，30 种外壳）：PNG 走 **首个 IDAT 内 + 改长度/CRC**；JPEG/GIF/PDF 走**尾标记之后**；
  RIFF 系走**声明长度之外**；其余走**最后一个合法结构之后**并**同步修正 ZIP CD 偏移**。
  → 它做的"追加后修 CD 偏移"正和我们的 `Δ = EOCD实际位置 − CD 记录偏移` 是同一件事。
- **pdvzip**：PNG 专精，数据在**最后一个 IDAT**、脚本在 **iCCP**；上限 2GB；建议不加密。
- **gp_disguise**：最简单——**尾追加 + 明文字符串分隔符 `FILE_DATA_BEGIN`** + 4 字节文件名长 + 文件名。
  → **检测成本几乎为 0**（尾部搜字符串即可），值得单独支持因为它服务 Google Photos 场景。

---

## 四、「解压说明模板」花样汇总（自动读懂这些 = 用户体验的胜负手）

### 4.1 说明的载体
`说明.txt` / `解压必看.txt` / `readme.txt` / `必读.txt`、**文件夹名本身**、下载页文章正文（"文章底部"）、
百度网盘分享备注、压缩包**注释段（ZIP comment）**、图片（截图/验证码图/二维码）、文件名里的括号。

### 4.2 改后缀要求（花样最多）
| 写法 | 含义 | 我们要做的 |
|------|------|-----------|
| "把 .MP4 改成 .ZIP 再用 WinRAR 解压" | 隐写者模式 A | ✅ 已支持 |
| "改成 .zip 后用 7-Zip 的 **#号模式**打开" | 模式 A/B | ✅ 我们自动做（等价 `-t#`） |
| "MKV 改后缀后 **工具-修复压缩文件** 再解压" | 隐写者 MKV 变体的民间解法 | ⚠️ 我们直接扫附件，无需修复 |
| ".tif / .jpg / .pdf / .exe 都改成 .tar / .zip" | 纯扩展名伪装（ACG港湾：".tif 后续不会解压 → 你改成 .tar 就行了"） | ✅ 按 **magic** 判定，忽略扩展名 |
| ".zi删p" / ".7z删" / ".zip备份" / ".zain" / ".zakms" | **中文插字**或加词以绕审核 | ✅ 需**文件名清洗规则** |
| "movie.7z.00**删**1" | 卷号里插字 | ✅ 同上（rozx 用 `config/cloaked_file_rules.json`） |
| ".downloading" 后缀 | 百度网盘下载未完成 | ✅ 提示用户，或按已完成处理 |
| ISO 也当视频改后缀（"iso 文件与 mp4 文件相同，都可以用本站的方法解压"） | ISO 被当作外壳 | ⚠️ ISO 是镜像不是压缩包 → 支持"剥离"而非解压 |

### 4.3 解压码（密码）的花样
| 写法 | 例子 | 自动提取思路 |
|------|------|------------|
| 明文直给 | "解压码：0721"（galgameg.com 的通用密码就是 `0721`） | 正则 `解压码|提取码|密码|pass(word)?` |
| 文件名里带 | `压缩密码是：123456.txt` → **密码是 123456，不是 123456.txt** | 剥掉扩展名 |
| 压缩包**注释段** | ZIP archive comment | 读 EOCD comment |
| **图片**里 | 截图/手写图/长图 | OCR（可选，OCR 技能已有） |
| **验证码图** | **隐写者自带 `captcha_generator.exe`**——把提取码渲染成验证码图防爬 | OCR |
| **二维码** | 少数站 | 二维码解码 |
| 编码/转换 | base64、hex、"某不知名网友分享" 用 UTF-8 强编码成 `鏌愪笉鐭ュ悕缃戝弸鍒嗕韩` | **必须做 GBK↔UTF-8 互转重试**（隐写者官方 Wiki 明确教的诡计） |
| 通用弱口令 | `0721`、`123456`、站点域名、`www.example.com` | 内置弱口令表 + 站点域名推断 |
| 密码本 | `passwords.txt` / `PW.txt` / `1_all.txt` / `0_temp.txt` | 支持多种命名 + 自动学到的密码回写 |
| 双密码 | 外层+内层不同密码（PolyFlix、FileImgSwap "一图双解"） | 分层尝试 |

### 4.4 分卷说明的花样（芯幻/知乎/南+ 的教程都在讲这些）
- 两类主流：`.part1.rar/.part2.rar`（关键包是 part1）与 `.7z.001/.002`（关键包是 001）。
- **必须全部放同一文件夹、命名前缀完全一致**（教程会强调"前缀只能数字不同"）。
- **只点 001 / part1**，点其他会报"需要下一分卷"。
- 少见坑：**以 000 开头的分卷**（`.7z.000`）→ Bandizip 会失败，要用 7z；
  `PC1/PC2` 等异形；分卷名被插字。
- 报告话术要对应："需要下一分卷" = 缺卷；"文件损坏" = 某卷下载不完整。

### 4.5 其它反复出现的说明元素
- "**不要在网盘在线解压**（会炸档）"——几乎所有中文站都写。
- 双链接（分卷分两个链接放）。
- "改后缀前先打开『显示文件扩展名』"（隐写者 README 反复强调"如不理解，请搜索如何改后缀名"）。
- 提到**网盘分享链接 + 提取码**与**资源验证码**分开给。
- 个别站点提供**在线解隐写网页**（南+ 帖子里出现过 `file.garden/…/restore_mp4_payload.html`）——说明需求真实存在。
- galgame 站常见"MP4 防和谐加密，需先解密还原"（galgameg.com 有专页）。

---

## 五、必须支持的变体清单（按优先级）

### P0 — 不做就等于没做（覆盖 90%+ 真实资源）
| # | 变体 | 依据 | 实现要点 |
|---|------|------|---------|
| 1 | **尾部追加 ZIP/ZIP64（含任意前置外壳）** | 隐写者模式A(老/新)、copy /b、PolyZip 大多数格式、gp_disguise、mp4vault | 尾 64KB+22 找 EOCD；`concat = EOCD − size_cd − offset_cd`；验 `PK\x03\x04`；ZIP64 走 locator |
| 2 | **MP4 free 原子内嵌 ZIP（ftyp 后 / 尾部）** | 隐写者模式B、PolyFlix、PhantomMan | 遍历顶层原子；`free` 体内搜 `PK\x03\x04`；支持 large size 形式（size=1 + 8B） |
| 3 | **MKV/WebM 附件轨** | 隐写者 mkv、mkvmerge 手工夹带 | 解析 EBML，扫 `Attachment`（`0x1941A469`）；也能直接搜 `PK` 兜底 |
| 4 | **忽略扩展名，按 magic 判定真实类型** | 全网所有"改后缀"要求 | 外壳识别 + 内芯识别分离 |
| 5 | **分卷自动串联**（`.001/.part1/.r00/.z01/.000`） | 所有大资源 | 拼流成单个 Read+Seek；同前缀+递增序号启发式分组 |
| 6 | **密码：ZipCrypto + ZIP AES-256 + 7z AES-256（含头加密）** | 中文圈常态 | zip-rs 开 `legacy-zip`；7z 密码枚举**限量** |
| 7 | **中文密码编码互转（GBK/Big5/UTF-8/UTF-16）** | 隐写者官方 Wiki 专门教这个诡计 | 多编码轮试 + 自动纠错 |
| 8 | **嵌套压缩包递归解压** | 资源普遍两层以上 | 层级上限保护 |

### P1 — 做了才叫"什么包都能解"
| # | 变体 | 依据 | 实现要点 |
|---|------|------|---------|
| 9 | **apate 头替换还原** | 隐写者 Wiki 点名"必须用对应软件" | 末 4B 小端 N + `[len-4-N,len-4)` 反转验 magic（§3.2） |
| 10 | **PNG：IDAT 内（首/末）+ iCCP + IEND 后追加** | PolyZip、pdvzip、FileImgSwap | 解析 chunk；IEND(`49 45 4E 44`)后判追加 |
| 11 | **JPEG `FF D9` 后 / GIF `3B` 后 / PDF `%%EOF` 后追加** | PolyZip、图种 | 尾标记定位 |
| 12 | **RIFF 系（BMP/WebP/WAV/AVI）声明长度之外** | PolyZip、gp_disguise | 读 size 字段比对实际长 |
| 13 | **gp_disguise 分隔符格式** | xob0t | 搜 `FILE_DATA_BEGIN` + 4B 名长（成本极低，顺手做） |
| 14 | **文件名解伪装（插字/加词/乱码）** | `.7z删`、`movie.7z.00删1` | 规则表 + 按 magic 反推真实扩展名 |
| 15 | **损坏 ZIP 抢救 + 2³² 偏移修补** | ccloli/baidupan-zip-extract、我们的 8.7GB 实测（CD 差 2355200） | CD 偏移对不上就 `+2³²` 重试；再不行扫 local header 重建目录（= 7-Zip LOCALS-CD-MODE） |
| 16 | **文件名乱码修复（GBK/CP949/Shift-JIS → UTF-8）** | 全网；BlueZip、7z `-mcp=932` | 按 EFS flag / 启发式判编码 |
| 17 | **说明文件/文件名自动解析解压码** | 说明模板花样 | 正则 + 注释段 + 剥离 `.txt` |
| 18 | **分卷缺卷的差异化报错**（"需要下一分卷" vs "损坏"） | 芯幻/知乎教程 | 语义化提示 |

### P2 — 加分项 / 生态服务
19. 图片验证码 / 图片 / 二维码里的解压码 OCR。
20. 弱口令表 + 站点域名推断 + 密码自动学习回写（rozx / TR-Supowe 都有）。
21. **8-bit PCM → WAV/FLAC/MKV** 编码流的识别与还原（教程流派，无标准，需用户给参数）。
22. PolyZip 的 AES-256-GCM + Argon2id 加密容器。
23. 在线还原服务的替代（我们桌面/手机端直接做掉）。

### ❌ 明确无法支持（要写清楚原因，避免用户期待落空）
| 变体 | 原因 |
|------|------|
| **像素级 / DCT / LSB / PVD 隐写**（VidCrypt、bfg9k、stegobox、StegaVault、NoClue、izcoser/stegpy） | 数据藏在**像素或音视频采样值**里，**没有统一格式**；需要该工具自己的算法+参数，甚至需要原始封面才能做差值。**只能识别疑似**（体积异常、熵异常、`zsteg`/`steghide` 式检测）并提示，不能解 |
| **OpenPuff / OmniHide PRO / FileVeil 等闭源专有格式** | 无公开格式；除非逆向（不建议，成本高、法律风险） |
| **二进制→像素图（hxcy 流派）** | 无标准，宽高/编码方式由作者自定，用户不给参数就无从下手 |
| **真正加密后没有密码的包** | 数学上无解（不是我们的问题，任何工具都一样） |
| **RAR 的"创作"** | 许可限制（与 7-Zip 立场一致）：**只解不压** —— 打包端永不生成 RAR（DESIGN.md 已拍板） |
| **`.fis.png`（FileImgSwap 像素编码）/ bfg9k 的 MKV** | 同像素级；除非专门实现其 AES-CTR+HKDF 流程（P2 再议） |

---

## 六、给扫描引擎的判据速查（可直接落地）

### 6.1 尾部追加（最高命中率，先跑）
```
1. 读文件末尾 65557 字节（64KB + 22）
2. 反向搜 EOCD（50 4B 05 06）
   ├─ 有 → ZIP。concat = EOCD_pos − size_cd − offset_cd
   │       局部头位置 = 每个 CD 项 offset + concat，必须命中 50 4B 03 04
   │       ZIP64：先找 locator（50 4B 06 07，正好 20 字节 → 分块扫描要留 20B 重叠！）
   ├─ EOCD 的 cd_offset = 0xFFFFFFFF → ZIP64，走 EOCD64
   └─ 无 → 继续
3. 尾部搜 7z 签名（37 7A BC AF 27 1C）→ 用 32 + NextHeaderOffset + NextHeaderSize 求总长，
   CRC32(12..32) == u32@8 校验（假阳性 ≈ 2⁻⁶⁴）
4. 尾部搜 RAR（52 61 72 21 1A 07 [00|01 00]）、gzip（1F 8B）、bz2（42 5A 68）、xz（FD 37 7A 58 5A 00）
```
### 6.2 容器内嵌（第二步）
```
MP4：从 ftyp 起遍历顶层原子（size u32 + type 4B；size==1 时后跟 8B 大 size）
     → 任一 free 原子体内搜 50 4B 03 04（前面可能有 64~256B 随机，不能只看原子开头）
     → 也别忘了 mdat 之后/文件尾可能还有裸 ZIP（老版）
MKV：解析 EBML（1A 45 DF A3）→ 扫 0x1941A469（Attachment）→ 附件名常是 *.zip/*.7z
PNG：解析 chunk（len u32 + type 4B + data + CRC u32）→ 找 IEND（49 45 4E 44 AE 42 60 82）
     → IEND 之后有数据 = 尾追加；否则在 IDAT/iCCP 体内搜 PK
```
### 6.3 apate 专用判据
```
N = LE_i32(file[-4:])
if 4 <= N <= len-4 and N <= 306*1024*1024:
    head = reverse(file[len-4-N : len-4])
    if head 匹配任一已知 magic: → apate 包（还原 = 换回 head + 截断尾部 N+4）
```
### 6.4 ⚠️ 三个必须防的误报源
1. **隐写者模式B 的假签名**：文件里随机散布 RAR/ZIP/7Z/GZIP/BZIP2/XZ 签名 + 5~10KB 随机
   → **禁止"见签名即认为是归档"**，一律以"能否成功打开/EOCD 自洽"为准。
2. **伪加密/伪损坏 ZIP**（CTF 与"坏坏包"流派）→ 识别 bit flag 而非直接失败。
3. **ZIP64 分卷标志 `cd_offset = 0xFFFFFFFF`**（我们实测的真实文件就是这样）→ 不能当成损坏。

---

## 七、参考链接

**造包工具**
- 隐写者：https://github.com/cenglin123/SteganographierGUI ｜ Wiki「普通解压软件解除隐写的方法」：https://github.com/cenglin123/SteganographierGUI/wiki/
- apate：https://github.com/rippod/apate （源码 `apate/Program.cs`）
- PolyFlix 影藏：https://github.com/hcllmsx/PolyFlix
- PhantomMan：https://github.com/ew46883-collab/PhantomMan-Steganography-Tool-Hide-ZIP-Files-in-MP4-Videos-by-NightSubmarine
- FileImgSwap / 坏坏包：https://github.com/fzxx/FileImgSwap ｜ https://github.com/fzxx/NaughtyDamagePack
- 鸭鸭图：https://github.com/missolin/duck-image-tool
- PolyZip：https://github.com/InfoSecREDD/PolyZip
- pdvzip / pdvps：https://github.com/CleasbyCode/pdvzip ｜ https://github.com/CleasbyCode/pdvps
- gp_disguise：https://github.com/xob0t/gp-file-hide
- mp4vault：https://github.com/nostafobic-dev/mp4vault
- bfg9k：https://github.com/JustinTimperio/bfg9k

**解包竞品 / 工具**
- rozx/Complex-unzip-tool-v2：https://github.com/rozx/Complex-unzip-tool-v2
- TR-Supowe/Complex-Unzip-Tool：https://github.com/TR-Supowe/Complex-Unzip-Tool
- ccloli/baidupan-zip-extract：https://github.com/ccloli/baidupan-zip-extract
- MzyCf/Butter-unpacker：https://github.com/MzyCf/Butter-unpacker
- 解TMD压（绅士仓库，需登录）：https://cangku.moe/archives/216302

**中文圈教程 / 说明模板样本**
- 鲲 Galgame 论坛《关于 mp4 格式隐藏加密方式的随笔》：https://www.kungal.com/topic/2708
- 南+《隐写者：把资源嵌入 MP4 文件的隐写工具》转载：https://www.south-plus.net/simple/index.php?t2296206.html
- 芯幻《【电子扫盲课】解压教程 v1.0》：https://xhcyv.com/95260
- 小权的探险日记《【PC/安卓/IOS】如何解压本站的资源》：https://www.nsxsb.com/225
- ACG港湾《不会解压的看这个文章》：https://www.acggw.one/24893.html
- GAL 导航（MP4 防和谐解密指引）：https://galgameg.com/
- 知乎《如何解压分卷.001 分卷和 rar 分卷》：https://zhuanlan.zhihu.com/p/532962394
