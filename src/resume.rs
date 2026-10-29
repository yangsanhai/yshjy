//! 断点续传：把「已经解完的条目」记在磁盘上，下次接着解
//!
//! # 解决什么
//!
//! 14 GB 的包解到 90%（40 分钟）被关机 / 断电 / 用户按取消打断，
//! 以前只能从头再来一遍。现在下次开解时会**跳过已经解完的条目**，
//! 从断的地方接着往下走。
//!
//! # 为什么是「条目级」而不是「字节级」
//!
//! 字节级续传（记住某个文件写到第几个字节，下次从那儿续写）在归档格式里
//! 基本都是假需求：ZIP / 7z / RAR / tar 的条目是独立可寻址或独立可解码的，
//! 真正花时间的是「一个条目一个条目地解」，而不是「单个条目解到一半」。
//! 条目级的另一个好处是**不需要任何格式协作**：库里每个条目解完都会通知我们
//! （`ctx.entry_done`），我们只在那一刻落一行记录，不用碰解码器内部状态。
//!
//! 代价：**最大的那个文件**如果解到一半被打断，它下一次要重解。
//! 这个代价是有意接受的 —— 换来的是「任何格式、任何库、零侵入」。
//!
//! # 记录长什么样
//!
//! 状态文件是 **JSONL（一行一条）**，写在 `<配置目录>/resume/<指纹>.jsonl`：
//!
//! ```text
//! {"t":"head","v":1,"input":"/data/a.zip","size":1234567,"mtime":1698400000,"dest":"/data/a"}
//! {"t":"item","n":"剧集/01.mkv","k":"f","s":734003200,"m":1698400123456789000}
//! {"t":"item","n":"剧集","k":"d","s":0,"m":1698400123456789000}
//! ```
//!
//! 为什么是 append-only：解压中途被打断时文件可能停在半行，
//! 逐行容错读取（坏行跳过）就能保住前面所有记录，不需要写索引、不需要原子替换。
//!
//! # 怎么保证「接着解的文件是对的」
//!
//! 三重保证，缺一条就重解那一条：
//!
//! 1. **包没变**：头行记着包的绝对路径 + 大小 + 修改时间；任一不符 → 整份作废重来
//!    （用户换了包 / 重新下载了同名包，绝不能拿旧记录去跳新包的文件）
//! 2. **文件还在、大小对**：跳过前 `stat` 一遍，文件是不是文件、字节数是否与记录一致
//! 3. **当初真的解完过**：只有解码循环正常走到「原子改名成功」之后才落记录，
//!    中途取消 / 写盘失败的条目**不会**被记成完成（`.yshjy-part` 也不会留正式名）
//!
//! 第 3 条是关键：记录即承诺 —— 记下来的条目，当时一定完整写出了全部字节
//! （ZIP 侧还有「声明大小 vs 实读大小」的读满校验兜底）。
//!
//! # 什么时候不作数
//!
//! - 单流压缩（`.gz` / `.xz` / `.zst` …）：整包就是一个条目，断了只能整体重来
//!   —— 但**解完那次**会被记为完成，下次开解直接跳过（0 秒完成）
//! - 7z 固实压缩：跳过的条目仍要让库把数据解出来（否则后面条目位置对不上），
//!   我们省的只是「写盘」那一步
//! - 用户手动改过解出来的文件（大小也变了）：该条重解

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

/// 状态文件格式版本。改了字段含义就 +1，旧文件会被整份丢掉重来。
const 格式版本: u32 = 1;

/// 条目种类（决定跳过前怎么校验）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum 条目种类 {
    /// 普通文件：校验「是文件 + 字节数一致」
    文件,
    /// 目录：校验「是目录」
    目录,
    /// 符号链接 / 硬链接：只校验「这个路径上有个东西」
    链接,
}

impl 条目种类 {
    /// 落盘的记号（短，省空间）
    fn 记号(self) -> &'static str {
        match self {
            Self::文件 => "f",
            Self::目录 => "d",
            Self::链接 => "l",
        }
    }
}

/// 状态文件里的一行
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
enum 行 {
    /// 第一行：这份记录属于哪个包、解到哪儿
    #[serde(rename = "head")]
    头 {
        /// 格式版本
        v: u32,
        /// 包（绝对路径）
        input: String,
        /// 包大小
        size: u64,
        /// 包修改时间（秒）
        mtime: u64,
        /// 输出目录（绝对路径）
        dest: String,
    },
    /// 一个解完的条目
    #[serde(rename = "item")]
    条目 {
        /// 条目名（归档里的原始相对路径）
        n: String,
        /// 种类记号
        k: String,
        /// 解出来的字节数
        s: u64,
        /// 解出来的文件的修改时间（纳秒，0 = 不记）
        m: u64,
    },
}

/// 一条已完成条目的摘要（校验用，不落盘）
#[derive(Debug, Clone, Copy)]
struct 已完成条目 {
    大小: u64,
}

/// 写这一侧：一条记录一次 `write`，**不攒批、不 fsync**
///
/// # 为什么立刻写（这是踩出来的，别改回攒批）
///
/// 一开始为了省系统调用做成「攒够 64 条或过 2 秒才写一次」。结果实测发现：
/// 小包解到三分之一被 `kill -9`（关窗口 / 崩溃 / 断电的等价物）时，缓冲区里
/// 几十条记录**一条都没落盘** —— 恰恰是最需要续传的场景，续传记录是空的。
/// SIGINT（正常取消）能走到 `Drop` 所以没事，于是「点取消能续、关窗口不能续」，
/// 这是最糟的错配。
///
/// 现在的做法：每完成一个条目就 `write` 一次。数据进了内核 page cache 就跟
/// 进程死活无关了，所以**进程被硬杀也不会丢**；不做 `fsync` 所以断电仍可能丢
/// 最后几条（那几条下次重解，不产生坏文件）。
///
/// 代价：10 万个小文件多约 10 万次 `write`（每次几十微秒，合计 0.1~0.3 秒），
/// 相对于解压本身的开销可以忽略。**不要**为了这点开销换回批量写。
struct 写侧 {
    /// 状态文件
    文件: File,
    /// 本次要写出去的行（通常只有一条）
    待写: Vec<String>,
    /// 上次落盘时间（诊断用）
    上次落盘: std::time::Instant,
}

impl 写侧 {
    /// 把缓冲一次写出去
    fn 落盘(&mut self) {
        if self.待写.is_empty() {
            return;
        }
        // 拼成一次 write：10 万个小文件也只剩一千多次系统调用
        let mut s = String::with_capacity(self.待写.iter().map(|l| l.len() + 1).sum());
        for l in self.待写.drain(..) {
            s.push_str(&l);
            s.push('\n');
        }
        // 写失败就算了：续传记录断一段，下次多重解几个文件，不影响解压本身
        let _ = self.文件.write_all(s.as_bytes());
        // 不 fsync：数据进了 page cache 就跟进程存活无关了；
        // 断电最多丢最后一批（丢的那批下次重解）
        self.上次落盘 = std::time::Instant::now();
    }
}

impl Drop for 写侧 {
    fn drop(&mut self) {
        // 收尾（含用户按取消、批量中途停下）时把缓冲刷出去，
        // 保证「点取消 → 下次接着解」这条路不缺最后那批记录
        self.落盘();
    }
}

/// 一份解压任务的续传记录
///
/// **只要环境可写就一定创建成功**（第一次解压也要记录，否则第一次中断就没法续）。
/// `open` 返回 `None` 只表示「配置目录不可写」这类环境问题 —— 那种情况下照常从头解，
/// 续传是增强功能，任何异常都不该让解压本身失败。
pub struct ResumeLog {
    /// 状态文件路径
    路径: PathBuf,
    /// 写句柄 + 缓冲
    写: Mutex<写侧>,
    /// 已完成条目：名字 → 摘要
    已完成: Mutex<HashMap<String, 已完成条目>>,
    /// 打开时读到的记录条数（给界面显示「上次解到 X 项」）
    原有条数: AtomicU64,
    /// 本次真正跳过的条数（收尾汇报用）
    跳过条数: AtomicU64,
}

impl ResumeLog {
    /// 为一个「包 → 输出目录」组合打开续传记录
    ///
    /// 记录不存在 / 包的指纹变了 / 配置目录不可写 → `None`（**不报错**，静默从头解）。
    pub fn open(input: &Path, dest: &Path) -> Option<std::sync::Arc<Self>> {
        Self::open_in(&crate::config::config_dir(), input, dest)
    }

    /// 同 [`ResumeLog::open`]，但记录根目录由调用方指定
    ///
    /// 存在意义：测试要隔离到临时目录。**不能**靠改 `YSHJY_CONFIG_DIR` 环境变量 ——
    /// 环境变量是进程级的，测试并行跑会互相踩（本 crate `forbid(unsafe_code)`，
    /// 也不允许用 `set_var`）。
    pub fn open_in(
        根目录: &Path,
        input: &Path,
        dest: &Path,
    ) -> Option<std::sync::Arc<Self>> {
        let 绝对包 = 绝对化(input);
        let 绝对目录 = 绝对化(dest);
        let 包大小 = std::fs::metadata(&绝对包).ok()?.len();
        let 包时间 = 修改时间秒(&绝对包).unwrap_or(0);

        let 路径 = 状态文件路径(根目录, &绝对包, &绝对目录);
        if let Some(parent) = 路径.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }

        // ── 读已有记录（坏行 / 半行一律跳过，保住前面的）──
        let mut 已完成: HashMap<String, 已完成条目> = HashMap::new();
        let mut 指纹吻合 = false;
        if let Ok(f) = File::open(&路径) {
            let mut 第一行 = true;
            for line in BufReader::new(f).lines().map_while(std::result::Result::ok) {
                let Ok(行) = serde_json::from_str::<行>(&line) else {
                    continue;
                };
                match 行 {
                    行::头 {
                        v,
                        input: i,
                        size,
                        mtime,
                        dest: d,
                    } => {
                        if 第一行 {
                            指纹吻合 = v == 格式版本
                                && i == 绝对包.to_string_lossy()
                                && size == 包大小
                                && mtime == 包时间
                                && d == 绝对目录.to_string_lossy();
                        }
                    }
                    行::条目 { n, k, s, m: _ } => {
                        if 指纹吻合 {
                            let _ = k;
                            已完成.insert(n, 已完成条目 { 大小: s });
                        }
                    }
                }
                第一行 = false;
            }
        }

        // 指纹不吻合 = 这份记录不是这个包的 / 包被换过了 → 整份作废
        if !指纹吻合 {
            已完成.clear();
        }
        let 原有 = 已完成.len() as u64;

        // ── 打开写句柄；指纹作废时先清空再写新头 ──
        // Windows 上 append 句柄只有 FILE_APPEND_DATA 权限，set_len(0) 会被拒
        // （os error 5，2026-10-30 实测）——续传因此整个静默失效（open 返回 None，
        // 上层当「没有记录」从头解）。所以「截断重来」单独开 truncate 句柄：
        // 仍然是**原地截断不删文件**，原来选 set_len 而非「删了重建」为的是少一次竞态，
        // 这条性质 truncate 同样保住
        let file = if !指纹吻合 {
            let mut f = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&路径)
                .ok()?;
            let head = 行::头 {
                v: 格式版本,
                input: 绝对包.to_string_lossy().into_owned(),
                size: 包大小,
                mtime: 包时间,
                dest: 绝对目录.to_string_lossy().into_owned(),
            };
            写一行(&mut f, &head).ok()?;
            f
        } else {
            let mut f = OpenOptions::new().create(true).append(true).open(&路径).ok()?;
            if !末尾是换行(&路径) {
                // 上次是被硬杀 / 断电打断的：文件末尾留着半行。
                // 不补这个换行，新记录会**粘在坏行后面**，一起变成解不出来的垃圾行
                // （那样丢的是本次第一条记录，虽然只多重解一个文件，但没必要）
                let _ = f.write_all(b"\n");
            }
            f
        };

        Some(std::sync::Arc::new(Self {
            路径,
            写: Mutex::new(写侧 {
                文件: file,
                待写: Vec::new(),
                上次落盘: std::time::Instant::now(),
            }),
            已完成: Mutex::new(已完成),
            原有条数: AtomicU64::new(原有),
            跳过条数: AtomicU64::new(0),
        }))
    }

    /// 上次已经解完多少条（界面提示「接着上次来」用）
    pub fn 原有条数(&self) -> u64 {
        self.原有条数.load(Ordering::Relaxed)
    }

    /// 本次已经跳过多少条
    pub fn 跳过条数(&self) -> u64 {
        self.跳过条数.load(Ordering::Relaxed)
    }

    /// 这条路上有记录吗？（「有记录」≠「一定能跳过」，跳过还要过磁盘校验）
    pub fn 有记录(&self) -> bool {
        self.原有条数() > 0
    }

    /// 记录里现在一共有多少条（原有 + 本次新记的）
    ///
    /// 收尾判断用：「一条都没有」说明这次什么也没解开，记录留着没意义。
    pub fn 已记录条数(&self) -> u64 {
        self.已完成.lock().map(|d| d.len() as u64).unwrap_or(0)
    }

    /// 记录里这个条目声称多大（补进度用；没记过 = `None`）
    pub fn 记录大小(&self, name: &str) -> Option<u64> {
        self.已完成
            .lock()
            .ok()
            .and_then(|d| d.get(name).map(|v| v.大小))
    }

    /// 这个条目能跳过吗？
    ///
    /// 三个条件全满足才算：**记录里有它**、**磁盘上还在**、**大小对得上**。
    /// 判定通过时顺手把「本次跳过」计数 +1（不写盘，纯内存）。
    pub fn should_skip(&self, name: &str, out_path: &Path, 种类: 条目种类) -> bool {
        let 记录大小 = {
            let done = match self.已完成.lock() {
                Ok(d) => d,
                Err(_) => return false,
            };
            match done.get(name) {
                Some(v) => v.大小,
                None => return false,
            }
        };

        let ok = match 种类 {
            条目种类::目录 => out_path.is_dir(),
            条目种类::链接 => std::fs::symlink_metadata(out_path).is_ok(),
            条目种类::文件 => std::fs::metadata(out_path)
                .map(|m| m.is_file() && m.len() == 记录大小)
                .unwrap_or(false),
        };
        if ok {
            self.跳过条数.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    /// 记下一个**真的解完了**的条目
    ///
    /// 调用点必须在「原子改名成功 / 目录建好」之后 —— 中途取消或失败的条目不许记，
    /// 否则下次续传会拿半截文件当成品。重复记录同名条目是幂等的
    /// （ZIP 索引修复后的重试会把同一批条目再跑一遍）。
    pub fn record(&self, name: &str, out_path: &Path, 种类: 条目种类) {
        let mut done = match self.已完成.lock() {
            Ok(d) => d,
            Err(_) => return,
        };
        if done.contains_key(name) {
            return;
        }
        let (大小, mtime) = stat_摘要(out_path, 种类);
        let 行 = 行::条目 {
            n: name.to_string(),
            k: 种类.记号().to_string(),
            s: 大小,
            m: mtime,
        };
        if let Ok(mut w) = self.写.lock()
            && let Ok(s) = serde_json::to_string(&行)
        {
            w.待写.push(s);
            // **立刻 write，不攒批、不 fsync**（实测教训，见文件头「为什么立刻写」）
            w.落盘();
        }
        done.insert(
            name.to_string(),
            已完成条目 { 大小 },
        );
    }

    /// 解压完整成功 → 删掉状态文件（留着只会让下次误跳过）
    pub fn finish(&self) {
        // 先落盘再删：Windows 上「文件还被打开着」删不掉，先刷完缓冲
        if let Ok(mut w) = self.写.lock() {
            w.落盘();
        }
        let _ = std::fs::remove_file(&self.路径);
    }

    /// 扔掉这份记录（解压失败、包本身有问题时用 —— 留着也没有可续的东西）
    pub fn discard(&self) {
        if let Ok(mut w) = self.写.lock() {
            w.待写.clear();
        }
        let _ = std::fs::remove_file(&self.路径);
    }

    /// 状态文件路径（测试 / 诊断用）
    pub fn 路径(&self) -> &Path {
        &self.路径
    }
}

// ───────────────────────────── 内部工具 ─────────────────────────────

fn 写一行(f: &mut File, 行: &行) -> std::io::Result<()> {
    let mut s = serde_json::to_string(行)
        .map_err(|e| std::io::Error::other(format!("序列化续传记录失败: {e}")))?;
    s.push('\n');
    // 不 fsync：数据进了 page cache 就跟进程存活无关了；
    // 断电最多丢最后几条（丢的那几条下次重解，不会产生坏文件）
    f.write_all(s.as_bytes())
}

/// 文件最后是不是一个换行（空文件 / 读不到 = 当作是）
fn 末尾是换行(p: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = File::open(p) else {
        return true;
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return true;
    };
    if len == 0 {
        return true;
    }
    if f.seek(SeekFrom::End(-1)).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    matches!(f.read(&mut b), Ok(1)) && b[0] == b'\n'
}

/// 条目解出来后的大小 / 时间（拿不到就当 0，校验会退化成「只看在不在」）
fn stat_摘要(p: &Path, 种类: 条目种类) -> (u64, u64) {
    match 种类 {
        条目种类::文件 => match std::fs::metadata(p) {
            Ok(m) => (m.len(), 修改时间纳秒(&m)),
            Err(_) => (0, 0),
        },
        _ => (0, 0),
    }
}

#[cfg(unix)]
fn 修改时间纳秒(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    // 用「秒 + 纳秒」拼成一个数：只要前后两次 stat 结果一致就行，
    // 不需要它对应任何真实时间尺度
    m.mtime() as u64 * 1_000_000_000 + m.mtime_nsec() as u64
}

#[cfg(not(unix))]
fn 修改时间纳秒(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn 修改时间秒(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?;
    m.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 相对路径 → 绝对路径（拿不到 cwd 时原样返回，指纹会退化成「相对路径比较」）
#[cfg(unix)]
fn 绝对化(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(d) => d.join(p),
        Err(_) => p.to_path_buf(),
    }
}

/// Windows 版**不许**走 canonicalize：它只在目标存在时成功，且成功时带 `\\?\` 前缀 ——
/// 输出目录第一次跑还不存在、中断后重跑必然已存在，同一条命令会算出两个不同的指纹
/// （哈希不同 → 查的是两份记录），续传永远对不上号（2026-10-30 实测复现）。
/// 所以这里只用纯词法归一：转绝对 + 去前缀 + 分隔符统一，存不存在结果都一样。
#[cfg(windows)]
fn 绝对化(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(d) => d.join(p),
            Err(_) => p.to_path_buf(),
        }
    };
    let s = abs.to_string_lossy();
    // \\?\UNC\server\share 是网络路径的真身形式，去前缀后要补回 \\ 开头
    let 无前缀 = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    };
    PathBuf::from(无前缀.replace('/', "\\"))
}

/// 状态文件路径：`<配置目录>/resume/<包+目录指纹>.jsonl`
///
/// 指纹里同时含**输出目录**：同一个包解到两个不同目录是两份独立的进度，
/// 混在一份记录里会互相误跳过。
fn 状态文件路径(根目录: &Path, 输入: &Path, 输出目录: &Path) -> PathBuf {
    let mut h = Sha256::new();
    h.update(输入.to_string_lossy().as_bytes());
    h.update([0u8]);
    h.update(输出目录.to_string_lossy().as_bytes());
    let d = h.finalize();
    let mut hex = String::with_capacity(16);
    for b in d.iter().take(8) {
        hex.push_str(&format!("{b:02x}"));
    }
    根目录.join("resume").join(format!("{hex}.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试要隔离配置目录，否则会写到用户真实的 ~/.config/yshjy/resume
    ///
    /// 隔离靠 [`ResumeLog::open_in`] 显式传根目录 —— 不碰环境变量，
    /// 所以用例可以并行跑（本 crate `forbid(unsafe_code)`，也改不了环境变量）
    struct 沙箱 {
        目录: PathBuf,
    }

    impl 沙箱 {
        fn 新(名: &str) -> Self {
            let 目录 = std::env::temp_dir().join(format!(
                "yshjy-resume-test-{}-{}-{}",
                名,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&目录).unwrap();
            Self { 目录 }
        }

        /// 记录根目录（相当于生产环境的 ~/.config/yshjy）
        fn 根(&self) -> PathBuf {
            self.目录.join("config")
        }

        /// 打开续传记录（隔离到本沙箱）
        fn 开(&self, 包: &Path, 输出: &Path) -> Option<std::sync::Arc<ResumeLog>> {
            ResumeLog::open_in(&self.根(), 包, 输出)
        }

        fn 包(&self, 内容: &[u8]) -> PathBuf {
            let p = self.目录.join("包.zip");
            std::fs::write(&p, 内容).unwrap();
            p
        }

        fn 输出(&self) -> PathBuf {
            self.目录.join("解压")
        }
    }

    impl Drop for 沙箱 {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.目录);
        }
    }

    #[test]
    fn 没解过时记录是空的_什么都不会跳过() {
        let 沙 = 沙箱::新("empty");
        let 包 = 沙.包(b"fake archive bytes");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        std::fs::write(输出.join("随便.txt"), b"x").unwrap();
        // 第一次解压也要能开（否则第一次中断就没法续），但里面是空的
        let log = 沙.开(&包, &输出).expect("环境可写就该能开");
        assert_eq!(log.原有条数(), 0);
        assert!(!log.有记录());
        assert!(
            !log.should_skip("随便.txt", &输出.join("随便.txt"), 条目种类::文件),
            "没记过的条目一个都不许跳"
        );
    }

    #[test]
    fn discard_能清掉记录() {
        let 沙 = 沙箱::新("discard");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        let log = 沙.开(&包, &输出).unwrap();
        log.record("01.txt", &文件, 条目种类::文件);
        let 路径 = log.路径().to_path_buf();
        log.discard();
        assert!(!路径.exists(), "失败收尾把记录扔掉，别在配置目录里留垃圾");
    }

    #[test]
    fn 记录后重开能跳过同名同大小文件() {
        let 沙 = 沙箱::新("skip");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello world").unwrap();

        {
            let log = 沙.开(&包, &输出).expect("首次应有句柄");
            log.record("01.txt", &文件, 条目种类::文件);
        }

        let log = 沙.开(&包, &输出).expect("有记录应能重开");
        assert_eq!(log.原有条数(), 1);
        assert!(log.should_skip("01.txt", &文件, 条目种类::文件));
        assert!(!log.should_skip("02.txt", &文件, 条目种类::文件), "没记过的不许跳");
        assert_eq!(log.跳过条数(), 1);
    }

    #[test]
    fn 磁盘上文件大小变了就不许跳() {
        let 沙 = 沙箱::新("size");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
        }
        // 用户手改了文件 / 上次其实是半截
        std::fs::write(&文件, "hello world 加长了".as_bytes()).unwrap();
        let log = 沙.开(&包, &输出).unwrap();
        assert!(
            !log.should_skip("01.txt", &文件, 条目种类::文件),
            "大小对不上必须重解，不能拿坏文件凑数"
        );
    }

    #[test]
    fn 磁盘上文件被删了就不许跳() {
        let 沙 = 沙箱::新("gone");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
        }
        std::fs::remove_file(&文件).unwrap();
        let log = 沙.开(&包, &输出).unwrap();
        assert!(!log.should_skip("01.txt", &文件, 条目种类::文件));
    }

    #[test]
    fn 包被换过以后旧记录整份作废() {
        let 沙 = 沙箱::新("swap");
        let 包 = 沙.包(b"archive one");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
        }

        // 同名包、内容变了（重新下载 / 换了个包）
        std::fs::write(&包, b"archive two, totally different length").unwrap();
        let log = 沙.开(&包, &输出).unwrap();
        assert_eq!(log.原有条数(), 0, "包变了必须当新任务解");
        assert!(!log.should_skip("01.txt", &文件, 条目种类::文件));
    }

    #[test]
    fn 不同输出目录是两份独立进度() {
        let 沙 = 沙箱::新("twodest");
        let 包 = 沙.包(b"archive");
        let 甲 = 沙.目录.join("甲");
        let 乙 = 沙.目录.join("乙");
        std::fs::create_dir_all(&甲).unwrap();
        std::fs::create_dir_all(&乙).unwrap();
        let 文件 = 甲.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        {
            let log = 沙.开(&包, &甲).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
        }
        let log乙 = 沙.开(&包, &乙).unwrap();
        assert_eq!(log乙.原有条数(), 0, "换个目录解不能沿用另一个目录的进度");
    }

    #[test]
    fn finish_之后记录被清掉() {
        let 沙 = 沙箱::新("finish");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        let log = 沙.开(&包, &输出).unwrap();
        log.record("01.txt", &文件, 条目种类::文件);
        let 路径 = log.路径().to_path_buf();
        assert!(路径.exists());
        log.finish();
        assert!(!路径.exists(), "解完了就不该留记录，否则下次会误跳过");
    }

    #[test]
    fn 半行损坏不影响前面的记录() {
        let 沙 = 沙箱::新("broken");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        let 路径;
        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
            路径 = log.路径().to_path_buf();
        } // 这里缓冲落盘
        // 模拟「写到一半被硬杀」：文件尾部是半行
        let mut f = OpenOptions::new().append(true).open(&路径).unwrap();
        f.write_all(b"{\"t\":\"item\",\"n\":\"02.txt\",\"k\":\"f\",\"s\":").unwrap();
        drop(f);

        let log = 沙.开(&包, &输出).unwrap();
        assert_eq!(log.原有条数(), 1, "坏行跳过，前面的记录必须保住");
        assert!(log.should_skip("01.txt", &文件, 条目种类::文件));
    }

    #[test]
    fn 半行之后的新记录不会被粘成坏行() {
        let 沙 = 沙箱::新("sticky");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        std::fs::create_dir_all(&输出).unwrap();
        let 文件 = 输出.join("01.txt");
        std::fs::write(&文件, b"hello").unwrap();
        let 路径;
        {
            let log = 沙.开(&包, &输出).unwrap();
            路径 = log.路径().to_path_buf();
        }
        // 只有头行 + 半行垃圾
        let mut f = OpenOptions::new().append(true).open(&路径).unwrap();
        f.write_all(b"{\"t\":\"item\",\"n\":\"0").unwrap();
        drop(f);

        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("01.txt", &文件, 条目种类::文件);
        }
        let log = 沙.开(&包, &输出).unwrap();
        assert_eq!(
            log.原有条数(),
            1,
            "新记录必须另起一行，不能被半行垃圾吞掉"
        );
    }

    #[test]
    fn 目录与链接条目按类型校验() {
        let 沙 = 沙箱::新("kinds");
        let 包 = 沙.包(b"archive");
        let 输出 = 沙.输出();
        let 子目录 = 输出.join("剧集");
        std::fs::create_dir_all(&子目录).unwrap();
        {
            let log = 沙.开(&包, &输出).unwrap();
            log.record("剧集", &子目录, 条目种类::目录);
        }
        let log = 沙.开(&包, &输出).unwrap();
        assert!(log.should_skip("剧集", &子目录, 条目种类::目录));
        // 同一个名字用错的种类问，不能通过（防止文件被当目录跳过）
        assert!(!log.should_skip("剧集", &子目录, 条目种类::文件));
    }

    /// W2 的另一半（2026-10-30 实测复现）：Windows 的 canonicalize 只在目标**存在**时成功
    /// 且带 `\\?\` 前缀 —— 输出目录中断后重跑必然已存在，指纹若跟着变，续传永远对不上号
    #[test]
    fn windows下输出目录存在与否指纹路径一致() {
        let base = std::env::temp_dir().join(format!("yshjy_w2_fp_{}", std::process::id()));
        let 输出 = base.join("输出目录");
        let _ = std::fs::remove_dir_all(&输出);
        let 不存在时 = 绝对化(&输出);
        std::fs::create_dir_all(&输出).unwrap();
        let 存在时 = 绝对化(&输出);
        let _ = std::fs::remove_dir_all(&输出);
        assert_eq!(
            不存在时, 存在时,
            "同一个输出目录，存不存在必须算出同一条指纹路径"
        );
        assert!(
            !不存在时.to_string_lossy().starts_with(r"\\?\"),
            "去前缀后的路径不该再带 \\\\?\\（否则与不存在时的词法路径不同形）"
        );
    }
}
