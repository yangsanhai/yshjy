//! ZIP 基准定位 + 四种喂给 zip crate 的方式对照。
//! 用法: zip_probe <path>
use std::fs::File;
use std::io::Read as _;
use yshjy_spike::*;

fn list_and_head<R: std::io::Read + std::io::Seek>(a: &mut zip::ZipArchive<R>) {
    for i in 0..a.len() {
        match a.by_index(i) {
            Ok(e) => println!(
                "       - [{}] {:?} comp={:?} size={} csize={} crc={:08x} encrypted={}",
                i,
                e.name(),
                e.compression(),
                e.size(),
                e.compressed_size(),
                e.crc32(),
                e.encrypted()
            ),
            Err(err) => println!("       - [{i}] 打不开: {err}"),
        }
    }
    match a.by_index(0) {
        Ok(mut zf) => {
            let mut buf = [0u8; 64];
            match zf.read(&mut buf) {
                Ok(n) => println!("       ↳ entry0 前 {n} 字节 = {}", hex(&buf[..n])),
                Err(e) => println!("       ↳ entry0 读数据失败: {e}"),
            }
        }
        Err(e) => println!("       ↳ by_index(0) 失败: {e}"),
    }
}

fn report<R: std::io::Read + std::io::Seek>(tag: &str, r: R) {
    match zip::ZipArchive::new(r) {
        Ok(mut a) => {
            println!("  ✅ [{tag}] 成功  entries={}", a.len());
            println!(
                "       archive_offset(Δ)={} central_directory_start={} comment_len={}",
                a.offset(),
                a.central_directory_start(),
                a.comment().len()
            );
            list_and_head(&mut a);
        }
        Err(e) => println!("  ❌ [{tag}] 失败: {e}"),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("用法: zip_probe <path>");
    println!("═══ 文件: {path}");
    let info = match detect_zip_base(&path) {
        Ok(Some(i)) => i,
        Ok(None) => {
            println!("  ✗ 自研解析：没找到 EOCD/CD");
            return;
        }
        Err(e) => {
            println!("  ✗ 自研解析出错: {e}");
            return;
        }
    };

    println!("── 自研解析（真实扫描得到）");
    println!(
        "   file_len={} ({:.4} GB)",
        info.file_len,
        info.file_len as f64 / 1e9
    );
    println!(
        "   EOCD @{} (距 EOF {} 字节) comment_len={}",
        info.eocd_off,
        info.file_len - info.eocd_off,
        info.comment_len
    );
    println!(
        "   zip64={} EOCD64 @{:?} locator @{:?} 声明值={:?}",
        info.zip64, info.eocd64_off, info.loc64_off, info.loc_declared_eocd64
    );
    println!(
        "   entries={} first_entry={:?}",
        info.n_entries, info.first_entry_name
    );
    println!(
        "   CD 记录偏移字段={}  真实 first_CDFH={}  CD 结构完整={}",
        info.cd_offset_field, info.first_cdfh_real, info.cd_wellformed
    );
    println!("   Δ_cd    = 真实CD − CD记录值         = {}", info.delta_cd);
    println!("   Δ_eocd  = 真实EOCD64 − locator声明  = {:?}", info.delta_eocd);
    println!("   Δ_entry = 真实LFH − entry.LFH记录值 = {:?}", info.delta_entry);
    let consistent =
        info.delta_eocd == Some(info.delta_cd) && info.delta_entry == Some(info.delta_cd);
    println!(
        "   ► 三者一致 = {consistent}  ⇒ 情形 {}",
        if consistent {
            "A（全局一致平移）"
        } else {
            "C（基准不一致，需要补丁）"
        }
    );

    println!("\n── 方式 A：ZipArchive::new(File)（整文件直接喂，无任何偏移处理）");
    let _ = report("A naive", File::open(&path).unwrap());

    println!("\n── 方式 B：with_config(Config{{archive_offset: Known(Δ_cd)}})");
    let cfg = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(info.delta_cd as u64),
    };
    let _ = report("B Known(Δ_cd)", File::open(&path).unwrap());
    let _ = zip::ZipArchive::with_config(cfg, File::open(&path).unwrap());

    println!("\n── 方式 C：Window 把 [Δ_cd, EOF) 重标定为 0 起点");
    match Window::open_to_end(&path, info.delta_cd as u64) {
        Ok(w) => report("C Window(Δ_cd)", w),
        Err(e) => println!("  ✗ 构造 Window 失败: {e}"),
    }

    println!("\n── 方式 D：8 字节 locator 补丁（PatchWindow，base=0）");
    match (info.eocd64_off, info.loc64_off) {
        (Some(e64), Some(lo)) => {
            let newval = (e64 as i128 - info.delta_cd) as u64;
            println!(
                "   把 @{} 处 8 字节 locator 声明值 {:#x} 改成 {:#x}",
                lo + 8,
                info.loc_declared_eocd64.unwrap_or(0),
                newval
            );
            let patch = (lo + 8, newval.to_le_bytes().to_vec());
            match PatchWindow::open(&path, 0, info.file_len, vec![patch]) {
                Ok(w) => report("D PatchWindow", w),
                Err(e) => println!("  ✗ 构造 PatchWindow 失败: {e}"),
            }
        }
        _ => println!("   （非 ZIP64，无 locator，跳过）"),
    }
}
