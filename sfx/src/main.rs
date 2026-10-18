//! 杨三海解压 —— Windows 自解压存根
//!
//! 一个极小的 exe：双击后把自己尾部藏着的 ZIP 解到旁边的新文件夹，再打开目录。
//!
//! # 文件结构（打包端 `src/sfx.rs` 负责写）
//! ```text
//! [ 存根 exe ][ ZIP 数据 ][ 尾标 24 字节 ]
//!                                     ├─ "YSHSFX01"  8 字节
//!                                     ├─ zip_offset  u64 小端
//!                                     └─ zip_size    u64 小端
//! ```
//! 用显式尾标而不是「让 zip 库自己找」：zip 的中央目录偏移是相对 ZIP 起点的，
//! 从文件中间开始读时，库会 seek 到错误位置。尾标把偏移写死，谁也别猜。
//!
//! # 故意不做的
//! - **不弹进度窗口**：初版只保证「双击能解出来」；进度条等主界面那套 UX 稳了再说
//! - **不支持解压码**：加密包直接提示用主程序打开（存根不做密码输入，免得成钓鱼模板）
//! - **不删自己**：解压完留着，用户想再解一次也行

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 尾标魔数 + 24 字节固定长度（8 + 8 + 8）
const 尾标: &[u8; 8] = b"YSHSFX01";
const 尾标长度: u64 = 24;

fn main() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            提示(&format!("找不到自己：{e}"), true);
            return;
        }
    };

    let (偏移, 长度) = match 读布局(&exe) {
        Ok(v) => v,
        Err(e) => {
            提示(&e, true);
            return;
        }
    };

    let 目标 = 唯一目录(&exe);
    if !确认(&format!(
        "把内容解压到：\n{}\n\n现在开始？",
        目标.display()
    )) {
        return;
    }

    提示(
        "正在解压，完成后会自动打开文件夹。\n大文件可能要几分钟，这期间窗口不会动，别急。",
        false,
    );

    match 解压(&exe, 偏移, 长度, &目标) {
        Ok(数量) => {
            打开目录(&目标);
            提示(
                &format!("解压完成：{数量} 个文件\n\n{}", 目标.display()),
                false,
            );
        }
        Err(e) => 提示(&format!("解压失败：\n{e}"), true),
    }
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

/// 只读 exe 里 [基址, 基址+长度) 这一段，对 zip 库表现为「一个独立文件」
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

    fs::create_dir_all(目标).map_err(|e| format!("建不了文件夹 {}：{e}", 目标.display()))?;

    let mut 数量 = 0usize;
    for i in 0..归档.len() {
        let mut 条目 = 归档
            .by_index(i)
            .map_err(|e| format!("第 {} 个条目读不了：{e}", i + 1))?;
        if 条目.encrypted() {
            return Err(
                "这个包带解压码，自解压程序暂时不支持。\n请用「杨三海解压」打开它并输入解压码。"
                    .into(),
            );
        }
        // enclosed_name 已经拦掉 ../ 和绝对路径；再兜一层，宁可跳过也不写出去
        let Some(相对) = 条目.enclosed_name() else {
            continue;
        };
        let 输出 = 目标.join(&相对);
        if 条目.is_dir() {
            fs::create_dir_all(&输出)
                .map_err(|e| format!("建不了文件夹 {}：{e}", 输出.display()))?;
            continue;
        }
        if let Some(父) = 输出.parent() {
            fs::create_dir_all(父).map_err(|e| format!("建不了文件夹 {}：{e}", 父.display()))?;
        }
        let mut 写 = File::create(&输出)
            .map_err(|e| format!("写不了 {}：{e}", 输出.display()))?;
        io::copy(&mut 条目, &mut 写)
            .map_err(|e| format!("写 {} 时出错：{e}", 输出.display()))?;
        let _ = 写.flush();
        数量 += 1;
    }
    Ok(数量)
}

#[cfg(windows)]
fn 弹窗(文本: &str, 标题: &str, 图标: u32) -> i32 {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            ty: u32,
        ) -> i32;
    }
    let t: Vec<u16> = 文本.encode_utf16().chain(std::iter::once(0)).collect();
    let c: Vec<u16> = 标题.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 图标) }
}

/// 非 Windows（开发机上 cargo check）只打到 stderr，方便调试
#[cfg(not(windows))]
fn 弹窗(文本: &str, 标题: &str, _图标: u32) -> i32 {
    eprintln!("[{标题}] {文本}");
    6
}

/// MB_ICONERROR = 0x10 / MB_ICONINFORMATION = 0x40 / MB_YESNO|MB_ICONQUESTION = 0x24
/// 测试时设 `YSHJY_SFX_SILENT=1`：不弹窗、确认当「是」（wine 里也能自动跑）
fn 静默() -> bool {
    std::env::var_os("YSHJY_SFX_SILENT").is_some()
}

fn 提示(文本: &str, 错误: bool) {
    if 静默() {
        eprintln!("{}", if 错误 { "错误" } else { "提示" });
        eprintln!("{文本}");
        return;
    }
    弹窗(文本, "杨三海解压", if 错误 { 0x10 } else { 0x40 });
}

fn 确认(文本: &str) -> bool {
    if 静默() {
        return true;
    }
    弹窗(文本, "杨三海解压", 0x24) == 6
}

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
