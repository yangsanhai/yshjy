# Ark + FileRoller 拆解：桌面文件管理器集成（补拆 §10.2 第 4 项）

> 来源：`refs/外部-归档软件/Ark/`（KDE，17 MB，C++/Qt6，clone `--depth 1`）、
> `refs/外部-归档软件/FileRoller/`（GNOME，11 MB，C/GTK4，**44.7**，commit `8ca93d77` 2026-10-06）
> 读完范围：`Ark/kerfuffle/archiveinterface.h`、`Ark/plugins/clizipplugin/kerfuffle_clizip.json.cmake`、
> `FileRoller/nautilus/nautilus-fileroller.c`、`FileRoller/src/` 子类清单。
> 拆解日期：2026-10-28。
> **为什么拆它们**：`docs/GUI-PLAN.md:603` 只有一句「桌面集成学 File Roller / Ark」，
> FileRoller 之前**全仓零提及**。这条线（右键菜单 / 默认打开 / 通知）是多平台适配之后的事，
> 但结论要现在记下，免得将来重新调研。

---

## 1. 两个项目的后端模型：都在「甩锅给外部程序」，只是方式不同

| | Ark（KDE） | FileRoller（GNOME 44.7） |
|---|---|---|
| 后端形态 | **插件**（`KPluginMetaData` 发现，`.so` 动态加载） | **每个命令行工具一个 GObject 子类**（`fr-command-7z.c` 等 **21 个**）|
| 插件/子类目录 | `plugins/{clizip,cli7z,clirar,cliarj,cliunarchiver,libzip,libarchive}/` | `src/fr-command-*.c`（zip/7z/rar/tar/cpio/iso/lha/arj/rpm/dpkg/ace/alz/lrzip/jar…）|
| 核心抽象 | `Kerfuffle::ReadOnlyArchiveInterface`（`kerfuffle/archiveinterface.h:29`） | `FrCommand`（`G_DEFINE_TYPE_WITH_PRIVATE`，`fr-command.c:109`） |
| 必实现的虚函数 | `list()`、`testArchive()`（`archiveinterface.h` 里是纯虚 `= 0`） | `fr_command_list` / `extract` / `add` …（GObject vtable） |
| 正在发生的变化 | — | **新增 `fr-archive-libarchive.c`**：官方在把 CLI 后端逐步换成**内建 libarchive** |

> **对我们最重要的一条**：FileRoller 这个「一格式一外部 CLI」的老架构，现在正被
> `fr-archive-libarchive.c` 替换。也就是说 **GNOME 官方也在走「内建引擎」这条路** ——
> 我们 `src/` 里那 24k 行自研解码器不是「重复造轮子」，是业界终局形态。
> 这条可以拿去回答「为什么不直接调 unzip CLI」的质疑（我们的答案还有一条更硬的：
> 纯伪装包、假签名、内嵌封面这些场景 unzip/7z 根本不认）。

---

## 2. ⭐ Ark 的插件元数据：**能力声明跟着格式走**

`Ark/plugins/clizipplugin/kerfuffle_clizip.json.cmake`（CMake 配置时生成 `.json`）摘录：

```json
{
  "KPlugin": { "Name": "Info-zip plugin", "MimeTypes": ["application/x-java-archive", "application/zip"] },
  "X-KDE-Kerfuffle-ReadOnlyExecutables": ["zipinfo", "unzip"],
  "X-KDE-Kerfuffle-ReadWrite": true,
  "X-KDE-Kerfuffle-ReadWriteExecutables": ["zip"],
  "X-KDE-Priority": 160,
  "application/zip": {
    "CompressionLevelDefault": 6, "CompressionLevelMin": 0, "CompressionLevelMax": 9,
    "CompressionMethodDefault": "Deflate",
    "CompressionMethods": { "BZip2": "bzip2", "Deflate": "deflate", "Store": "store" },
    "Encryption": true, "EncryptionMethodDefault": "ZipCrypto", "EncryptionMethods": ["ZipCrypto"],
    "SupportsTesting": true
  }
}
```

四个设计点：

1. **能力声明是数据，不是代码**（JSON 里按 MIME 分开写）。UI 读 JSON 就能决定
   「这个格式能不能设压缩级别 0–9 / 能不能加密 / 能不能测试」——**不用问后端**。
2. **`X-KDE-Priority: 160`** 解决「同一 MIME 有多个插件」的竞争：分高的优先。
   我们没这个问题（只有一份引擎），但**我们 `caps` 的粒度可以学它**：
   现在 `caps` 是全局一份（`docs/PROTOCOL.md`），而 Ark 是**按格式**声明能力。
3. **`EncryptionMethods` 是列表 + `...Default`**：能力是集合，默认是其中一个。
   我们的加密支持现在是布尔值（能不能设密码），没有「支持哪几种加密算法」这层。
4. **`SupportsTesting: true`** —— 「能不能测试包完整性」是一个独立能力位。我们有
   `--no-checksum`（打包侧校验和），但解压侧「测试完整性」这个动作**没有对外选项**。
   `docs/CLI-SPEC.md` 里没有 `--test`。这是 Ark 提醒我们的一个缺口（低优先，记着）。

---

## 3. ⭐ FileRoller 的 Nautilus 扩展：**菜单项按「能不能成功」裁剪**

`nautilus/nautilus-fileroller.c` 的 `get_file_items`（约 `:230-300`）：

```c
if (all_archives && can_write) {
    item = nautilus_menu_item_new ("NautilusFr::extract_here",
            g_dcgettext ("file-roller", "Extract Here", LC_MESSAGES), ...);
} else if (all_archives && !can_write) {
    item = nautilus_menu_item_new ("NautilusFr::extract_to",
            g_dcgettext ("file-roller", "Extract To…", LC_MESSAGES), ...);
}
```

逻辑：**先算「当前目录我能不能写」**（`nautilus_file_info_can_write(parent)`），
- 能写 → 给「解压到此处」（一键，不弹窗）
- 不能写 → 只给「解压到…」（弹目录选择框）

**这就是「别给用户一个必然失败的按钮」的教科书实现。** 用户在 `/usr/share` 里右键，
看到的是「解压到…」而不是点了报错的「解压到此处」。

还有一层：`all_archives` 是遍历所有选中文件算出来的（`file_mime_info.is_archive`），
**混选了非压缩包就不给菜单项**。

---

## 4. 我们现在的桌面集成是什么样（对照用）

| 项 | 我们（0.9.28） | Ark | FileRoller |
|---|---|---|---|
| 默认打开 | ✅ `gui/linux/yshjy.desktop` 认领 **14 种 MIME**（zip/x-7z/vnd.rar/x-rar/tar/gzip/x-xz/zstd/bzip2/brotli/lz4…） | 插件按 MIME 竞争 | Nautilus 模块 |
| 双击打开的是谁 | `Exec=/usr/lib/yshjy/gui/yshjy_gui`（GUI） | Ark | FileRoller |
| 右键集成 | `scripts/install-nautilus.sh` → 装进 **`~/.local/share/nautilus/scripts/`**（右键→**脚本**子菜单） | KDE 服务菜单 | **原生菜单项**（顶层，不在「脚本」里） |
| 菜单是否按可写性裁剪 | ❌ 没判断 | — | ✅ |
| 混选非压缩包时 | 脚本收到啥就试啥 | — | ✅ 全选都是压缩包才出现 |
| 能力按格式声明 | 🟡 `caps` 全局一份 | ✅ 按 MIME | — |

**差距的实质**：Nautilus 的「脚本」子菜单是**二级菜单**，用户要先右键 → 找「脚本」→ 再点。
FileRoller 是**一级菜单项**，右键直接看到「解压到此处」。这需要写 Nautilus 扩展（C/Python 的
`NautilusMenuProvider`），不是 shell 脚本能做的 —— 工作量比现在的脚本大一个数量级。

**这条不急着做**，但要知道差距在哪：现在我们的右键入口藏得比 FileRoller 深一层。

---

## 5. 可抄 / 不抄

| # | 内容 | 判定 |
|---|---|---|
| 1 | **菜单项按「目标目录可写」裁剪** | 🔴 现在就能用：`install-nautilus.sh` 的脚本里可以先测 `-w`，不可写时弹的提示改成「请选择其他目录」（`gui/linux/nautilus-script-yshjy`） |
| 2 | 能力按**格式**声明（不是全局一份） | 🟡 等 `docs/PROTOCOL.md` 的 `caps` 升版时考虑；现在全局够用 |
| 3 | 「测试完整性」作为独立能力 + `--test` 选项 | 🟢 记进候选功能，进 `docs/TODO.md` 表尾 |
| 4 | Nautilus 原生菜单项（一级） | ⏸ 工作量太大，需要 C/Python 扩展；等有人抱怨「藏太深」再做 |
| 5 | Ark 的插件架构（动态加载 `.so`） | ❌ 不抄 —— 我们单引擎，插件化只会增加攻击面（与「常用版依赖越少越好」冲突） |
| 6 | FileRoller 的「一 CLI 一子类」 | ❌ 不抄 —— 官方自己都在往内建 libarchive 迁 |

---

## 6. 顺手记下的事实（省得重查）

1. **FileRoller 44.7 有 21 个 `fr-command-*` 子类**，其中 `fr-command-jar.c` 直接继承 zip
   （`G_DEFINE_TYPE (FrCommandJar, fr_command_jar, fr_command_zip_get_type ())`）——
   「同格式不同扩展名」用继承复用，这一点和我们「一个解码器认多种扩展名」等价。
2. **Ark 的 `ReadOnlyArchiveInterface` 有 `isMultiVolume()` / `numberOfVolumes()`** ——
   分卷是一等公民（我们也是，`--split` 与分卷识别都在）。KDE 里这两个是基类方法，
   每个插件都要回答。我们的 `caps` 里可以考虑暴露「本包是分卷，共 N 卷」。
3. Ark 的 `archiveinterface.h:26-28` 定义了 `PossiblyMaliciousArchiveError`
   （自定义错误码，基于 `KJob::UserDefinedError + 1`）——**「可能恶意」是一类独立错误**，
   不是普通失败。我们对应的是 `src/safety.rs` 的炸弹限额询问，方向一致。
