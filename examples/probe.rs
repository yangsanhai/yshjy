//! 探测/解压示例：把「定位」和「解压」两条链路串起来
//!
//! 用法：
//! ```bash
//! probe <文件>                      # 尾部窗口定位（快）
//! probe --scan <文件>               # 全文件扫描（慢但全）
//! probe --hw <文件>                 # 只看硬件探测与推荐档位
//! probe --rescue <文件>             # 救援模式：不看中央目录，按 Local header 重建
//! probe --extract <文件>            # 解压（自动定位 + 安全落盘）
//! probe --extract --password XXXX --dest 输出目录 <文件>
//! ```

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let has = |k: &str| args.iter().any(|a| a == k);
    let val_of = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let path = match args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--") && !is_value(&args, a))
    {
        Some(p) => p.clone(),
        None => {
            eprintln!(
                "用法: probe [--scan|--hw|--rescue|--extract] [--password X] [--dest 目录] <文件>"
            );
            return ExitCode::from(2);
        }
    };

    let mut f = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("打不开文件: {e}");
            return ExitCode::from(1);
        }
    };
    let size = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("读不到文件信息: {e}");
            return ExitCode::from(1);
        }
    };

    println!("文件      : {path}");
    println!("大小      : {size} 字节 ({:.3} GB)", size as f64 / 1e9);

    // ── 硬件探测 ──
    if has("--hw") {
        let hw = yshjy::hardware::Hardware::detect().with_target(&path);
        println!();
        println!("══════ 硬件探测 ══════");
        println!("  环境摘要   : {}", hw.summary());
        println!("  磁盘类型   : {:?}", hw.disk_kind);
        let rec = hw.recommend_profile(size);
        println!("  推荐档位   : {}（深度 {:?}）", rec.label(), rec.depth());
        println!("  全扫预估   : {} 秒", hw.estimate_full_scan_secs(size));
        if !has("--scan") && !has("--extract") {
            return ExitCode::SUCCESS;
        }
    }

    // ── 定位（尾部窗口）──
    println!();
    let t0 = Instant::now();
    let layout = match yshjy::layout::zip::locate(&mut f, size) {
        Ok(l) => {
            println!("[定位] ✅ 成功（{:?}）", t0.elapsed());
            println!("   ZIP 起点   : {}", l.base);
            println!("   条目数     : {}", l.entries_total);
            println!("   ZIP 长度   : {} 字节", l.data_len());
            Some(l)
        }
        Err(e) => {
            println!("[定位] ❌ {e}");
            None
        }
    };

    if let Some(l) = &layout
        && !has("--extract")
        && let Ok(entries) = yshjy::layout::zip::list_entries(&mut f, l)
    {
        println!("📦 内层内容（{} 项）:", entries.len());
        for e in entries.iter().take(20) {
            let lock = if e.encrypted { "🔒" } else { "  " };
            println!("   {lock} {}  ({} 字节)", e.path, e.size);
        }
        if entries.len() > 20 {
            println!("   … 还有 {} 项", entries.len() - 20);
        }
    }

    // ── 解压 ──
    if has("--extract") {
        let l = match &layout {
            Some(l) => l,
            None => {
                println!();
                println!("[解压] ❌ 没有定位到 ZIP，无法解压（可试 --scan 找找）");
                return ExitCode::from(1);
            }
        };
        let dest = PathBuf::from(val_of("--dest").unwrap_or_else(|| ".".to_string()));
        println!();
        println!("[解压] 目标目录: {}", dest.display());
        let mut opts = yshjy::extract::ExtractOptions::new(&dest);
        opts.password = val_of("--password");
        if let Some(pw) = &opts.password {
            // 密码永远打码
            println!(
                "       密码: {}***（已提供，不回显）",
                pw.chars().take(1).collect::<String>()
            );
        }
        let t1 = Instant::now();
        match yshjy::extract::extract_zip_at(&mut f, l.base, l.data_len(), &opts) {
            Ok(rep) => {
                println!("       ✅ {}（耗时 {:?}）", rep.summary(), t1.elapsed());
                for fe in rep.files.iter().take(10) {
                    let lock = if fe.encrypted { "🔒" } else { "  " };
                    println!("          {lock} {}  ({} 字节)", fe.name, fe.size);
                }
                if rep.files.len() > 10 {
                    println!("          … 还有 {} 个", rep.files.len() - 10);
                }
                for (n, why) in rep.skipped.iter().take(5) {
                    println!("          ⚠ {n}: {why}");
                }
                for w in rep.warnings.iter().take(5) {
                    println!("          ℹ {w}");
                }
            }
            Err(e) => {
                println!("       ❌ 解压失败: {e}");
                return ExitCode::from(1);
            }
        }
    }

    // ── 全文件扫描 ──
    if has("--scan") {
        println!();
        println!("[全文件扫描] 开始…");
        let t1 = Instant::now();
        match yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::default()) {
            Ok(hits) => {
                println!("   完成（{:?}），命中 {} 处", t1.elapsed(), hits.len());
                for h in hits.iter().take(15) {
                    let conf = match h.confidence {
                        yshjy::scan::Confidence::High => "高",
                        yshjy::scan::Confidence::Medium => "中",
                        yshjy::scan::Confidence::Low => "低",
                    };
                    println!(
                        "   [{conf}] {} 偏移 {}  {}",
                        h.format.as_str(),
                        h.offset,
                        h.note
                    );
                }
            }
            Err(e) => println!("   扫描失败: {e}"),
        }
    }

    // ── 救援模式 ──
    if has("--rescue") {
        println!();
        println!("[救援模式] 不看中央目录，按 Local header 顺序重建…");
        let hits =
            match yshjy::scan::scan_whole_file(&mut f, size, &yshjy::scan::ScanOptions::strict()) {
                Ok(h) => h,
                Err(e) => {
                    println!("   扫描失败: {e}");
                    return ExitCode::SUCCESS;
                }
            };
        let start = match hits
            .iter()
            .find(|h| h.format == yshjy::layout::ArchiveFormat::Zip)
        {
            Some(h) => h.offset,
            None => {
                println!("   没找到 ZIP 起点");
                return ExitCode::SUCCESS;
            }
        };
        let t2 = Instant::now();
        match yshjy::rescue::rescue_by_locals(
            &mut f,
            start,
            size,
            &yshjy::rescue::RescueOptions::default(),
        ) {
            Ok(items) => {
                println!(
                    "   ✅ 从偏移 {start} 重建出 {} 个条目（{:?}）",
                    items.len(),
                    t2.elapsed()
                );
                for it in items.iter().take(10) {
                    println!(
                        "      {:>10} 字节 @{:>12}  {}",
                        it.pack_size, it.header_at, it.path
                    );
                }
            }
            Err(e) => println!("   ❌ 救援失败: {e}"),
        }
    }

    // ── 打包（--pack）：把文件/目录藏进封面 ──
    if has("--pack") {
        let out_path = val_of("--out").unwrap_or_else(|| "packed.mp4".to_string());
        let inputs: Vec<std::path::PathBuf> = args
            .iter()
            .skip(1)
            .filter(|a| !a.starts_with("--") && !is_value(&args, a))
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists() && p.is_dir() || p.is_file())
            .collect();
        // 第一个非 flag 参数是"文件"，打包时用户会给多个输入：
        // 这里把除输出之外的所有已存在路径都当输入
        let inputs: Vec<std::path::PathBuf> = if inputs.is_empty() {
            vec![std::path::PathBuf::from(&path)]
        } else {
            inputs
        };

        println!();
        println!("[打包] 输入 {} 项 → {out_path}", inputs.len());
        let mut opts = yshjy::pack::PackOptions {
            inputs: inputs.clone(),
            password: val_of("--password"),
            ..Default::default()
        };
        if has("--tail") {
            opts.layout = yshjy::pack::PackLayout::TailAppend;
        }
        if let Some(c) = val_of("--cover") {
            opts.cover = std::fs::read(&c).ok();
        }
        let mut out_file = match std::fs::File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                println!("   ❌ 建输出失败: {e}");
                return ExitCode::from(1);
            }
        };
        match yshjy::pack::build(&opts, &mut out_file) {
            Ok(rep) => {
                drop(out_file);
                // 探针直接打结构化字段（`PackReport` 不再产句子：文案归外壳）
                println!(
                    "   ✅ 写出 {} 字节（封面 {} + 内容 {}），共 {} 个条目，布局 {:?}",
                    rep.total_size, rep.cover_size, rep.zip_size, rep.entries, rep.layout
                );
                // 落地后自检（不能"以为成功就完事"）
                match yshjy::pack::self_check(std::path::Path::new(&out_path), rep.entries) {
                    Ok(warns) if warns.is_empty() => {
                        println!("   ✅ 自检通过（回读定位 + 条目数一致）")
                    }
                    Ok(warns) => {
                        for w in warns {
                            println!("   ⚠ {w}");
                        }
                    }
                    Err(e) => println!("   ❌ 自检失败: {e}"),
                }
            }
            Err(e) => println!("   ❌ 打包失败: {e}"),
        }
        return ExitCode::SUCCESS;
    }

    ExitCode::SUCCESS
}

/// 判断某个参数是否是另一个参数的值（如 `--dest 目录` 里的"目录"）
fn is_value(args: &[String], candidate: &str) -> bool {
    let flags = ["--dest", "--password", "--out", "--cover"];
    args.iter().enumerate().any(|(i, a)| {
        flags.contains(&a.as_str()) && args.get(i + 1).map(|v| v == candidate).unwrap_or(false)
    })
}
