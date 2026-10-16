//! 解压引擎 —— 把「定位」与「解码」彻底解耦
//!
//! # 设计（综合参考项目调研的结论）
//! - **我们负责定位**：自研的 `layout` / `scan` / `rescue` 层算出压缩包在文件里的**真实偏移**
//!   （这是市面工具的短板：它们靠中央目录或扩展名猜，遇到伪装包就废）
//! - **库负责解码**：成熟的 `zip` crate 负责 deflate / ZipCrypto / AES
//! - **桥接**：`io::Window` 把「文件 + 偏移」虚拟成一个干净的 ZIP 文件交给库
//!
//! # 为什么不是一条龙交给库
//! 实测：`zip` crate 自己找 EOCD 要扫完 8.7 GB（113 秒）；
//! 我们把偏移算好再喂给它只要 **69 微秒**（快约 1600 倍）。
//! 这条路线也对应 zip2 源码里那个思路：**不必自己写解压器，
//! 把定位修好，让成熟的解码器干活**。
//!
//! # 安全（对齐调研报告「对比-安全与健壮性」的 P0 项）
//! - 路径穿越：走 `safety::resolve_safe`（防 `../`、盘符、保留名、ADS）
//! - 解压炸弹：走 `safety::ExtractBudget`（压缩比 + 总量双闸）
//! - 危险文件：`safety::is_risky_executable` 只警告不阻断（用户自己决定）

use crate::error::{Error, ErrorCode, Result};
use crate::io::Window;
use crate::safety::{self, ExtractBudget, Limits};
use std::fs;
use std::io::{Read, Seek};
use std::path::PathBuf;

/// 同名文件处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overwrite {
    /// 跳过已存在
    Skip,
    /// 覆盖
    Replace,
    /// 自动改名（`name (1).ext`）—— 默认，最不容易误删用户数据
    #[default]
    Rename,
}

/// 解压参数
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// 输出目录
    pub dest: PathBuf,
    /// 密码（`None` = 不试密码；只解未加密的条目）
    pub password: Option<String>,
    /// 同名处理
    pub overwrite: Overwrite,
    /// 只解这些条目（`None` = 全解）；支持目录前缀匹配
    pub selected: Option<Vec<String>>,
    /// 安全限值
    pub limits: Limits,
}

impl ExtractOptions {
    /// 以输出目录构造默认参数（不覆盖、不解密、全选）
    pub fn new<P: Into<PathBuf>>(dest: P) -> Self {
        Self {
            dest: dest.into(),
            password: None,
            overwrite: Overwrite::default(),
            selected: None,
            limits: Limits::default(),
        }
    }
}

/// 一个成功写出的文件
#[derive(Debug, Clone)]
pub struct ExtractedFile {
    /// 归档内的原始名字
    pub name: String,
    /// 实际落盘路径
    pub out_path: PathBuf,
    /// 大小
    pub size: u64,
    /// 是否加密
    pub encrypted: bool,
}

/// 解压报告 —— **结构化结果，而不是一个 bool**
///
/// 对齐 7-Zip 的「结果分级」思路（`ZipHandler.cpp:1246-1251` 的多个 *_Error 标志）：
/// 部分成功也是成功，必须把「哪些没成、为什么」如实交给上层。
#[derive(Debug, Default)]
pub struct ExtractReport {
    /// 成功写出的文件
    pub files: Vec<ExtractedFile>,
    /// 被跳过的条目（名字, 原因）
    pub skipped: Vec<(String, String)>,
    /// 非致命的提示（如"解出了可执行文件"）
    pub warnings: Vec<String>,
    /// 累计写出的字节数
    pub bytes_written: u64,
    /// 是否因为限额被中止
    pub budget_hit: bool,
}

impl ExtractReport {
    /// 是否一个文件都没解出来
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
    /// 是否「全部干净成功」（没有跳过、没有警告）
    pub fn is_clean(&self) -> bool {
        self.skipped.is_empty() && self.warnings.is_empty() && !self.budget_hit
    }
    /// 一句话摘要
    pub fn summary(&self) -> String {
        let mut s = format!(
            "解出 {} 个文件 / {:.1} MB",
            self.files.len(),
            self.bytes_written as f64 / 1_048_576.0
        );
        if !self.skipped.is_empty() {
            s += &format!("，跳过 {} 个", self.skipped.len());
        }
        if !self.warnings.is_empty() {
            s += &format!("，{} 条提示", self.warnings.len());
        }
        if self.budget_hit {
            s += "（触发安全限额，已中止）";
        }
        s
    }
}

/// 选中判断：`selected` 为 `None` 表示全选；
/// 否则条目名等于某项、或以某项 + `/` 开头（目录前缀）都算选中。
fn is_selected(selected: &Option<Vec<String>>, name: &str) -> bool {
    match selected {
        None => true,
        Some(list) => list.iter().any(|s| {
            let s = s.trim_end_matches('/');
            name == s || name.starts_with(&format!("{s}/"))
        }),
    }
}

/// 同名时生成不冲突的名字：`a.txt` → `a (1).txt`
fn unique_path(path: &std::path::Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for i in 1..10_000 {
        let name = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let cand = parent.join(name);
        if !cand.exists() {
            return cand;
        }
    }
    path.to_path_buf()
}

/// **解压指定区域里的 ZIP**
///
/// - `r`：源（一般传 `&mut File`）
/// - `base`：ZIP 数据在源里的起始偏移（来自 `layout::zip::locate` 或 `scan`）
/// - `len`：ZIP 数据长度（从 `ZipLayout::data_len()` 拿）
pub fn extract_zip_at<R: Read + Seek>(
    r: R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
) -> Result<ExtractReport> {
    // 把「文件 + 偏移」虚拟成一个独立的小 ZIP 文件
    let window = Window::new(r, base, len)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("window: {e}")))?;

    let mut archive = zip::ZipArchive::new(window)
        .map_err(|e| Error::new(ErrorCode::NotArchive).context(format!("zip open: {e}")))?;

    // 先列出「打包后总大小」，给炸弹防护做基准
    let packed_total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index_raw(i).ok().map(|f| f.compressed_size()))
        .sum::<u64>()
        .max(1);
    let mut budget = ExtractBudget::new(opts.limits.clone(), packed_total);

    fs::create_dir_all(&opts.dest)
        .map_err(|e| Error::new(ErrorCode::Internal).context(format!("mkdir dest: {e}")))?;

    let mut report = ExtractReport::default();

    for i in 0..archive.len() {
        // 取名字（先不解码数据）
        let (name, is_dir, _size, encrypted, out_path_result) = {
            let f = match archive.by_index_raw(i) {
                Ok(f) => f,
                Err(e) => {
                    report
                        .skipped
                        .push((format!("#{i}"), format!("读条目失败: {e}")));
                    continue;
                }
            };
            // 用我们自己的四级回退链解码 —— 不能直接用 zip 的 `name()`：
            // 它在没有 UTF-8 标志位时会按 CP437 解，中文名会变成乱码
            let raw_name = crate::layout::zip::decode_name(f.name_raw(), false);
            let is_dir = f.is_dir();
            let _size = f.size();
            let encrypted = f.encrypted();
            let safe = safety::resolve_safe(&opts.dest, &raw_name);
            (raw_name, is_dir, _size, encrypted, safe)
        };

        if !is_selected(&opts.selected, &name) {
            continue;
        }

        // 路径安全检查
        let mut out_path = match out_path_result {
            Ok(p) => p,
            Err(e) => {
                report
                    .skipped
                    .push((name.clone(), format!("路径不安全被拒: {e}")));
                continue;
            }
        };

        if is_dir {
            if let Err(e) = fs::create_dir_all(&out_path) {
                report
                    .skipped
                    .push((name.clone(), format!("建目录失败: {e}")));
            }
            continue;
        }

        // 同名处理
        if out_path.exists() {
            match opts.overwrite {
                Overwrite::Skip => {
                    report
                        .skipped
                        .push((name.clone(), "已存在，按策略跳过".into()));
                    continue;
                }
                Overwrite::Rename => out_path = unique_path(&out_path),
                Overwrite::Replace => {}
            }
        }

        if let Some(parent) = out_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report
                .skipped
                .push((name.clone(), format!("建父目录失败: {e}")));
            continue;
        }

        // 危险可执行文件：警告但不阻断（用户自己决定跑不跑）
        if safety::is_risky_executable(&name) {
            report
                .warnings
                .push(format!("{name}：可执行文件，已解出但请谨慎运行"));
        }

        // ── 真正解码 ──
        let mut file = match if encrypted {
            match &opts.password {
                Some(pw) => archive
                    .by_index_decrypt(i, pw.as_bytes())
                    .map_err(|e| format!("解密失败（密码可能不对）: {e}")),
                None => Err("条目已加密，但没有提供密码".to_string()),
            }
        } else {
            archive
                .by_index(i)
                .map_err(|e| format!("打开条目失败: {e}"))
        } {
            Ok(f) => f,
            Err(msg) => {
                report.skipped.push((name.clone(), msg));
                continue;
            }
        };

        // 流式写盘 + 边写边记限额（防炸弹）
        let mut out = match fs::File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                report
                    .skipped
                    .push((name.clone(), format!("创建失败: {e}")));
                continue;
            }
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        let mut read_err = None;
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = budget.record(n as u64) {
                        read_err = Some(format!("安全限额中止: {e}"));
                        break;
                    }
                    if let Err(e) = std::io::Write::write_all(&mut out, &buf[..n]) {
                        read_err = Some(format!("写盘失败: {e}"));
                        break;
                    }
                    written += n as u64;
                }
                Err(e) => {
                    read_err = Some(format!("读取失败: {e}"));
                    break;
                }
            }
        }

        match read_err {
            Some(msg) => {
                report.skipped.push((name.clone(), msg));
                // 清理半成品（对齐 unar「失败即删输出文件」的做法）
                drop(out);
                let _ = fs::remove_file(&out_path);
            }
            None => {
                report.bytes_written += written;
                report.files.push(ExtractedFile {
                    name,
                    out_path,
                    size: written,
                    encrypted,
                });
            }
        }
    }

    Ok(report)
}

/// **带自动修复的解压**
///
/// 先按正常路径解；如果索引对不上（很多伪装包会故意改坏中央目录偏移），
/// 就让 [`crate::repair`] 分析出「错在哪几个字节」，用 [`crate::io::patch::PatchWindow`]
/// 在**内存里**改对后重试 —— 磁盘文件一个字节都不动。
///
/// 返回 `(报告, 修复说明)`；修复说明为 `None` 表示一路顺利、不需要修。
///
/// 对齐调研结论：坏包多数是「数据完好、索引写错」，修索引比重新实现解压算法划算得多。
pub fn extract_zip_auto<R: Read + Seek>(
    r: &mut R,
    base: u64,
    len: u64,
    opts: &ExtractOptions,
) -> Result<(ExtractReport, Option<String>)> {
    // ① 先走正常路径
    match extract_zip_at(&mut *r, base, len, opts) {
        Ok(rep) => Ok((rep, None)),
        Err(first_err) => {
            // ② 尝试分析索引问题
            let analysis = match crate::repair::analyze(r, base, len) {
                Ok(a) => a,
                Err(_) => return Err(first_err),
            };
            let plan = match analysis {
                crate::repair::RepairAnalysis::Repairable(p) => p,
                _ => return Err(first_err),
            };

            // ③ 在内存里打补丁后重试（PatchWindow 自身已把 [base,base+len) 虚拟化，
            //    所以再喂给 extract 时 base=0、len=窗口长度）
            let patched = crate::io::patch::PatchWindow::new(&mut *r, base, len, plan.patches)
                .map_err(|e| {
                    Error::new(ErrorCode::Internal).context(format!("建补丁窗口失败: {e}"))
                })?;
            let mut patched = patched;
            let rep = extract_zip_at(&mut patched, 0, len, opts)?;
            Ok((rep, Some(plan.reason)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// 造一个内存 ZIP（含中文名 + 目录）
    fn make_zip() -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.add_directory("sub/", opt).unwrap();
        w.start_file("sub/中文.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, "hello 中文 content".as_bytes()).unwrap();
        w.start_file("plain.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, b"plain").unwrap();
        w.finish().unwrap().into_inner()
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("yshjy_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn 解压内存_zip_到目录() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("basic");

        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert_eq!(rep.files.len(), 2, "应解出 2 个文件，实际: {:?}", rep.files);
        assert_eq!(rep.skipped.len(), 0, "不该有跳过: {:?}", rep.skipped);
        let names: Vec<_> = rep.files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"sub/中文.txt"), "中文名要正确: {names:?}");
        assert!(names.contains(&"plain.txt"));
        // 内容对
        let txt = fs::read_to_string(dest.join("sub/中文.txt")).unwrap();
        assert_eq!(txt, "hello 中文 content");
        assert!(rep.bytes_written > 0);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 偏移窗口_能从伪装包中间解出() {
        let data = make_zip();
        // 前面塞 3KB 假视频头，后面塞 2KB 垃圾
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\x00\x00\x00\x18ftypmp42");
        buf.extend(std::iter::repeat_n(0xAAu8, 3000 - 8));
        let base = buf.len() as u64;
        let len = data.len() as u64;
        buf.extend_from_slice(&data);
        buf.extend(std::iter::repeat_n(0u8, 2000));

        let dest = scratch_dir("offset");
        let mut cur = Cursor::new(buf);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, base, len, &opts).unwrap();
        assert_eq!(rep.files.len(), 2);
        assert!(rep.is_clean());
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 路径穿越被拒() {
        // 手工造一个带 ../ 的 ZIP
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        w.start_file("../../etc/evil.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, b"pwned").unwrap();
        let data = w.finish().unwrap().into_inner();
        let len = data.len() as u64;

        let dest = scratch_dir("slip");
        let mut cur = Cursor::new(data);
        let opts = ExtractOptions::new(&dest);
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();

        assert!(rep.files.is_empty(), "路径穿越必须被挡住");
        assert_eq!(rep.skipped.len(), 1);
        assert!(
            rep.skipped[0].1.contains("不安全") || rep.skipped[0].1.contains("拒"),
            "跳过原因应说明是安全问题: {:?}",
            rep.skipped
        );
        // 确认宿主机上没多出文件
        assert!(!PathBuf::from("/etc/evil.txt").exists());
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 选择性提取只解指定条目() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("select");
        let mut cur = Cursor::new(data);
        let mut opts = ExtractOptions::new(&dest);
        opts.selected = Some(vec!["plain.txt".to_string()]);
        let rep = extract_zip_at(&mut cur, 0, len, &opts).unwrap();
        assert_eq!(rep.files.len(), 1);
        assert_eq!(rep.files[0].name, "plain.txt");
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 同名按策略处理() {
        let data = make_zip();
        let len = data.len() as u64;
        let dest = scratch_dir("dupe");
        // 先解一次
        let mut c1 = Cursor::new(data.clone());
        extract_zip_at(&mut c1, 0, len, &ExtractOptions::new(&dest)).unwrap();
        // 再解一次：Rename 应产生 "plain (1).txt"
        let mut c2 = Cursor::new(data);
        let rep = extract_zip_at(&mut c2, 0, len, &ExtractOptions::new(&dest)).unwrap();
        let names: Vec<_> = rep
            .files
            .iter()
            .filter_map(|f| f.out_path.file_name())
            .collect();
        let joined = format!("{names:?}");
        assert!(joined.contains("(1)"), "自动改名应产生 (1): {joined}");
        let _ = fs::remove_dir_all(&dest);
    }

    /// 用真实加密样本（zig -P 造的 ZipCrypto / 7z AES256 造的 AES）验证解密通路
    fn 试加密样本(path: &str, pw: &str, tag: &str) {
        if !std::path::Path::new(path).exists() {
            eprintln!("样本 {path} 不存在，跳过");
            return;
        }
        let data = fs::read(path).unwrap();
        let len = data.len() as u64;
        let dest = scratch_dir(tag);

        // ① 不给密码 → 全部跳过，且原因要说清是"缺密码"，不能崩
        let mut c1 = Cursor::new(data.clone());
        let rep = extract_zip_at(&mut c1, 0, len, &ExtractOptions::new(&dest)).unwrap();
        assert!(
            rep.files.is_empty(),
            "{path}: 没密码不该解出东西: {:?}",
            rep.files
        );
        assert_eq!(
            rep.skipped.len(),
            2,
            "{path}: 两个条目都该被跳过: {:?}",
            rep.skipped
        );
        assert!(
            rep.skipped.iter().all(|(_, why)| why.contains("密码")),
            "{path}: 跳过原因必须说清是缺密码: {:?}",
            rep.skipped
        );

        // ② 给对密码 → 全部解出，内容必须对
        let mut c2 = Cursor::new(data);
        let mut opts = ExtractOptions::new(&dest);
        opts.password = Some(pw.into());
        let rep2 = extract_zip_at(&mut c2, 0, len, &opts).unwrap();
        assert_eq!(
            rep2.files.len(),
            2,
            "{path}: 给对密码应解出 2 个: {:?}",
            rep2.skipped
        );
        assert_eq!(
            fs::read_to_string(dest.join("locked.txt")).unwrap(),
            "top secret content"
        );
        assert_eq!(
            fs::read_to_string(dest.join("plain.txt")).unwrap(),
            "not secret"
        );

        // ③ 给错密码 → 不能崩，要如实报告
        let data3 = fs::read(path).unwrap();
        let mut c3 = Cursor::new(data3);
        let mut opts3 = ExtractOptions::new(&dest);
        opts3.password = Some("wrong-password".into());
        let rep3 = extract_zip_at(&mut c3, 0, len, &opts3).unwrap();
        assert!(
            rep3.files.is_empty(),
            "{path}: 错密码不该解出: {:?}",
            rep3.files
        );
        assert!(!rep3.skipped.is_empty(), "{path}: 错密码应如实报告失败");

        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn zipcrypto_加密包可解() {
        试加密样本("tests/data/zipcrypto_secret.zip", "secret", "enc_zipcrypto");
    }

    #[test]
    fn aes_加密包可解() {
        试加密样本("tests/data/aes_secret.zip", "secret", "enc_aes");
    }

    #[test]
    fn 自动修复_偏移被改错的包也能解() {
        // 造一个"数据好、但 EOCD 里 CD 偏移被改错"的包（伪装包常见手法）
        let data = b"repair me!";
        let crc = crate::scan::crc32(data);
        let mut z = Vec::new();
        z.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&9u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(b"fixed.txt");
        z.extend_from_slice(data);
        let cd_start = z.len() as u64;
        z.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        // 版本(2)+需求版本(2)+标志(2)+方法(2)+时间(2)+日期(2) = 12 字节
        z.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&crc.to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&(data.len() as u32).to_le_bytes());
        z.extend_from_slice(&9u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(&0u32.to_le_bytes());
        z.extend_from_slice(b"fixed.txt");
        let cd_size = z.len() as u64 - cd_start;
        z.extend_from_slice(b"PK\x05\x06");
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&0u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(&(cd_size as u32).to_le_bytes());
        z.extend_from_slice(&12345u32.to_le_bytes()); // ← 故意写错的偏移
        z.extend_from_slice(&0u16.to_le_bytes());

        let len = z.len() as u64;
        let dest = scratch_dir("repair");
        let mut cur = Cursor::new(z);
        let rep = extract_zip_auto(&mut cur, 0, len, &ExtractOptions::new(&dest)).unwrap();
        assert!(rep.1.is_some(), "应当报告发生了修复");
        assert_eq!(
            rep.0.files.len(),
            1,
            "修复后应解出文件: {:?}",
            rep.0.skipped
        );
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn 选中判断支持目录前缀() {
        let sel = Some(vec!["sub".to_string()]);
        assert!(is_selected(&sel, "sub/a.txt"));
        assert!(is_selected(&sel, "sub/deep/b.txt"));
        assert!(!is_selected(&sel, "other/c.txt"));
        assert!(is_selected(&None, "任意"));
    }
}
