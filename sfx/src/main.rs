//! 杨三海解压 —— 自解压存根（Windows / Linux 同一份源码）
//!
//! 双击后把自己尾部藏着的 ZIP 解到旁边的新文件夹，再打开目录。
//!
//! # 文件结构（打包端 `src/pack/mod.rs` 的 `build_sfx` 负责写）
//! ```text
//! [ 存根 ][ ZIP 数据 ][ 尾标 24 字节 ]
//!                        ├─ "YSHSFX01"  8 字节
//!                        ├─ zip_offset  u64 小端
//!                        └─ zip_size    u64 小端
//! ```
//! 用显式尾标而不是「让 zip 库自己找」：zip 的中央目录偏移是相对 ZIP 起点的，
//! 从文件中间开始读时，库会 seek 到错误位置。尾标把偏移写死，谁也别猜。
//!
//! # 第 23 批（P23-5）改了什么
//! - **支持解压码**：加密包不再直接拒绝 —— 提示输入（静默模式读 `YSHJY_SFX_PASSWORD`）
//! - **有进度显示**：逐条打百分比（这就是「进度窗口」；控制台比自绘 GUI 可靠）
//! - **不再用 MessageBox 弹窗**：改成控制台交互 —— Linux 上能真测，Windows 上双击
//!   会自动开控制台窗口，中文靠 `SetConsoleOutputCP(65001)` 保证不乱码
//!
//! # 故意不做的
//! - **不删自己**：解压完留着，用户想再解一次也行
//! - **不写注册表 / 不创建快捷方式**：那是安装程序的事，本程序只解压

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 尾标魔数 + 24 字节固定长度（8 + 8 + 8）
const 尾标: &[u8; 8] = b"YSHSFX01";
const 尾标长度: u64 = 24;

fn main() {
    设控制台编码();
    if let Err(e) = 运行() {
        // 错误走 stderr：控制台照样能看到，脚本 / 测试也能单独抓到
        eprintln!("\n❌ {e}");
        收尾();
        std::process::exit(1);
    }
}

fn 运行() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("找不到自己：{e}"))?;
    let (偏移, 长度) = 读布局(&exe)?;
    let 目标 = 唯一目录(&exe);

    println!("杨三海自解压包");
    println!("将解压到：{}", 目标.display());
    if !确认("现在开始？(y/N) ") {
        println!("已取消。");
        收尾();
        return Ok(());
    }

    let 数量 = 解压(&exe, 偏移, 长度, &目标)?;
    打开目录(&目标);
    println!("\n✅ 完成：{数量} 个文件");
    println!("   {}", 目标.display());
    收尾();
    Ok(())
}

/// 读尾部 24 字节，拿到 ZIP 段的起点和长度
fn 读布局(exe: &Path) -> Result<(u64, u64), String> {
    let mut f = File::open(exe).map_err(|e| format!("打不开自己：{e}"))?;
    let 总长 = f
        .metadata()
        .map_err(|e| format!("读不了文件信息：{e}"))?
        .len();
    if 总长 < 尾标长度 {
        return Err("这不是杨三海自解压包（文件太短）".into());
    }
    f.seek(SeekFrom::End(-(尾标长度 as i64)))
        .map_err(|e| format!("定位尾部失败：{e}"))?;
    let mut b = [0u8; 24];
    f.read_exact(&mut b)
        .map_err(|e| format!("读尾部失败：{e}"))?;
    if &b[0..8] != 尾标 {
        return Err("这不是杨三海自解压包".into());
    }
    let 偏移 = u64::from_le_bytes(b[8..16].try_into().unwrap());
    let 长度 = u64::from_le_bytes(b[16..24].try_into().unwrap());
    if 偏移.checked_add(长度).and_then(|x| x.checked_add(尾标长度)) != Some(总长) {
        return Err("自解压包被改动过（长度对不上），请重新拿一份".into());
    }
    Ok((偏移, 长度))
}

/// 解到「exe 同目录 / 同名文件夹」，重名就加序号
fn 唯一目录(exe: &Path) -> PathBuf {
    let 父 = exe.parent().unwrap_or_else(|| Path::new("."));
    let 主名 = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "解压结果".into());
    let 首选 = 父.join(&主名);
    if !首选.exists() {
        return 首选;
    }
    for i in 1..10_000 {
        let 候选 = 父.join(format!("{主名} ({i})"));
        if !候选.exists() {
            return 候选;
        }
    }
    父.join(format!(
        "{主名} ({})",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ))
}

/// 只读存根文件里 [基址, 基址+长度) 这一段，对 zip 库表现为「一个独立文件」
struct 偏移读取器 {
    文件: File,
    基址: u64,
    位置: u64,
    长度: u64,
}

impl 偏移读取器 {
    fn new(mut 文件: File, 基址: u64, 长度: u64) -> io::Result<Self> {
        文件.seek(SeekFrom::Start(基址))?;
        Ok(Self {
            文件,
            基址,
            位置: 0,
            长度,
        })
    }
}

impl Read for 偏移读取器 {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let 剩余 = self.长度.saturating_sub(self.位置);
        if 剩余 == 0 {
            return Ok(0);
        }
        let 可读 = (buf.len() as u64).min(剩余) as usize;
        let n = self.文件.read(&mut buf[..可读])?;
        self.位置 += n as u64;
        Ok(n)
    }
}

impl Seek for 偏移读取器 {
    fn seek(&mut self, 从: SeekFrom) -> io::Result<u64> {
        let 新 = match 从 {
            SeekFrom::Start(x) => x as i64,
            SeekFrom::Current(x) => self.位置 as i64 + x,
            SeekFrom::End(x) => self.长度 as i64 + x,
        };
        if 新 < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek 越界"));
        }
        let 新 = 新 as u64;
        self.文件.seek(SeekFrom::Start(self.基址 + 新))?;
        self.位置 = 新;
        Ok(新)
    }
}

fn 解压(exe: &Path, 偏移: u64, 长度: u64, 目标: &Path) -> Result<usize, String> {
    let 文件 = File::open(exe).map_err(|e| format!("打不开自己：{e}"))?;
    let 读取 = 偏移读取器::new(文件, 偏移, 长度)
        .map_err(|e| format!("定位压缩数据失败：{e}"))?;
    let mut 归档 = zip::ZipArchive::new(读取)
        .map_err(|e| format!("压缩数据读不了（包可能坏了）：{e}"))?;

    // 先探有没有加密条目：只要有，就在**写盘之前**把解压码要到
    //
    // 必须用 `by_index_raw`（不校验密码的接口）：`by_index` 碰到加密条目会直接
    // 返回 `Password required` —— 用它做检测永远探不到「有密码」（实测踩到）。
    let 有加密 = (0..归档.len()).any(|i| {
        归档
            .by_index_raw(i)
            .map(|e| e.encrypted())
            .unwrap_or(false)
    });
    let 密码: Option<String> = if 有加密 { Some(问密码()?) } else { None };

    // 密码先验一次再建目录：验证 = 只读第一个加密条目的开头（zip 库在这里做校验），
    // 不验就建目录的话，密码错会留下一个空文件夹（实测踩到）。
    if let Some(pw) = &密码 {
        let 第一条加密 = (0..归档.len())
            .find(|&i| 归档.by_index_raw(i).map(|e| e.encrypted()).unwrap_or(false));
        if let Some(i) = 第一条加密 {
            归档
                .by_index_decrypt(i, pw.as_bytes())
                .map_err(|_| "解压码不对（或这个包用的加密方式自解压不支持）".to_string())?;
        }
    }

    fs::create_dir_all(目标).map_err(|e| format!("建不了文件夹 {}：{e}", 目标.display()))?;

    let 总数 = 归档.len();
    let mut 数量 = 0usize;
    for i in 0..总数 {
        let mut 条目 = match &密码 {
            Some(pw) => 归档.by_index_decrypt(i, pw.as_bytes()).map_err(|_| {
                "解压码不对（或这个包用的加密方式自解压不支持）".to_string()
            })?,
            None => 归档
                .by_index(i)
                .map_err(|e| format!("第 {} 个条目读不了：{e}", i + 1))?,
        };
        // enclosed_name 已经拦掉 ../ 和绝对路径；再兜一层，宁可跳过也不写出去
        let Some(相对) = 条目.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let 输出 = 目标.join(&相对);
        if 条目.is_dir() {
            fs::create_dir_all(&输出)
                .map_err(|e| format!("建不了文件夹 {}：{e}", 输出.display()))?;
            // zip 9 的 name() 返回 Result<Cow<str>, _>，取不到就显示空
            打进度(i + 1, 总数, &条目.name().map(|c| c.to_string()).unwrap_or_default());
            continue;
        }
        if let Some(父) = 输出.parent() {
            fs::create_dir_all(父).map_err(|e| format!("建不了文件夹 {}：{e}", 父.display()))?;
        }
        let mut 写 =
            File::create(&输出).map_err(|e| format!("写不了 {}：{e}", 输出.display()))?;
        io::copy(&mut 条目, &mut 写)
            .map_err(|e| format!("写 {} 时出错：{e}", 输出.display()))?;
        let _ = 写.flush();
        数量 += 1;
        打进度(数量, 总数, &条目.name().map(|c| c.to_string()).unwrap_or_default());
    }
    if 总数 > 0 && !静默() {
        println!();
    }
    Ok(数量)
}

/// 进度行（同一行刷新：这就是双击后用户能看见的「进度窗口」）
fn 打进度(当前: usize, 总数: usize, 名: &str) {
    if 静默() {
        return;
    }
    let 百分比 = if 总数 == 0 {
        100
    } else {
        当前 * 100 / 总数
    };
    let 短名: String = 名.chars().take(36).collect();
    print!("\r[{:>3}%] {}/{}  {:<36}", 百分比, 当前, 总数, 短名);
    let _ = io::stdout().flush();
}

/// 测试时设 `YSHJY_SFX_SILENT=1`：不问确认、不刷进度、不等回车
fn 静默() -> bool {
    std::env::var_os("YSHJY_SFX_SILENT").is_some()
}

fn 确认(提示: &str) -> bool {
    if 静默() {
        return true;
    }
    print!("{提示}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    if io::stdin().read_line(&mut s).is_err() {
        return false;
    }
    matches!(s.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// 要解压码：静默模式从环境变量取（测试 / 批量场景），否则从控制台读
fn 问密码() -> Result<String, String> {
    if 静默() {
        return std::env::var("YSHJY_SFX_PASSWORD").map_err(|_| {
            "这个包有解压码：静默模式下请用环境变量 YSHJY_SFX_PASSWORD 提供".to_string()
        });
    }
    println!("这个包有解压码，请输入后回车（输入会显示在屏幕上）：");
    let mut s = String::new();
    io::stdin()
        .read_line(&mut s)
        .map_err(|e| format!("读不到输入：{e}"))?;
    let pw = s.trim_end_matches(['\r', '\n']).to_string();
    if pw.is_empty() {
        return Err("没输入解压码".into());
    }
    Ok(pw)
}

/// 双击场景：跑完别让窗口一闪就消失，等用户看一眼
fn 收尾() {
    if 静默() {
        return;
    }
    println!("\n按回车关闭这个窗口…");
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
}

/// Windows 控制台默认代码页是 GBK，Rust 打的是 UTF-8 → 中文会乱码。
/// 切成 65001（UTF-8）就正常了。
#[cfg(windows)]
fn 设控制台编码() {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleOutputCP(cp: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

#[cfg(not(windows))]
fn 设控制台编码() {}

#[cfg(windows)]
fn 打开目录(p: &Path) {
    if std::env::var_os("YSHJY_SFX_NO_OPEN").is_some() {
        return;
    }
    let _ = std::process::Command::new("explorer").arg(p).spawn();
}

#[cfg(not(windows))]
fn 打开目录(p: &Path) {
    // 测试时设 YSHJY_SFX_NO_OPEN=1，别真去弹文件管理器
    if std::env::var_os("YSHJY_SFX_NO_OPEN").is_some() {
        return;
    }
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}
