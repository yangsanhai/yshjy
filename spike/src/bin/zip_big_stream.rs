//! 8.6 GB 伪装 ZIP 的流式解压验证：只读前 N 字节，测量峰值内存。
//! 用法: zip_big_stream <path> [front_bytes]
use std::fs::File;
use std::io::Read as _;
use std::time::Instant;
use yshjy_spike::*;

fn vm_hwm_kb() -> String {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if line.starts_with("VmHWM") || line.starts_with("VmRSS") {
            print!("");
            if line.starts_with("VmHWM") {
                return line.trim().to_string();
            }
        }
    }
    "?".into()
}

fn main() {
    let path = std::env::args().nth(1).expect("用法: zip_big_stream <path> [bytes]");
    let want: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(64 * 1024 * 1024); // 默认 64 MiB

    let t0 = Instant::now();
    let info = detect_zip_base(&path).unwrap().expect("没找到 ZIP 结构");
    println!(
        "自研定位: Δ_cd={} Δ_eocd={:?} Δ_entry={:?}  耗时 {:?}  内存 {}",
        info.delta_cd,
        info.delta_eocd,
        info.delta_entry,
        t0.elapsed(),
        vm_hwm_kb()
    );
    let consistent =
        info.delta_eocd == Some(info.delta_cd) && info.delta_entry == Some(info.delta_cd);
    println!("三 Δ 一致 = {consistent}");

    println!("\n[A] ZipArchive::new(File::open) 直接开（不做任何偏移处理）");
    let t = Instant::now();
    let f = File::open(&path).unwrap();
    match zip::ZipArchive::new(f) {
        Ok(mut a) => {
            println!(
                "  ✅ 成功  entries={} offset(Δ)={} central_directory_start={}  耗时 {:?}  内存 {}",
                a.len(),
                a.offset(),
                a.central_directory_start(),
                t.elapsed(),
                vm_hwm_kb()
            );
            for i in 0..a.len() {
                let e = a.by_index(i).unwrap();
                println!(
                    "   - [{}] {:?} comp={:?} size={} csize={} encrypted={}",
                    i,
                    e.name(),
                    e.compression(),
                    e.size(),
                    e.compressed_size(),
                    e.encrypted()
                );
            }
            println!("\n[B] 流式读取 entry0 前 {} 字节（不落盘、不整包读入）", want);
            let (got, first64, elapsed) = {
                let t = Instant::now();
                let mut zf = a.by_index(0).unwrap();
                let mut got = 0usize;
                let mut buf = vec![0u8; 1024 * 1024];
                let mut first64 = Vec::new();
                while got < want {
                    let want_now = (want - got).min(buf.len());
                    match zf.read(&mut buf[..want_now]) {
                        Ok(0) => break,
                        Ok(n) => {
                            if first64.len() < 64 {
                                let take = (64 - first64.len()).min(n);
                                first64.extend_from_slice(&buf[..take]);
                            }
                            got += n;
                        }
                        Err(e) => {
                            println!("  读取出错: {e}");
                            break;
                        }
                    }
                }
                (got, first64, t.elapsed())
            };
            let _ = SEVENZ_SIG;
            println!(
                "   读出 {} 字节，耗时 {:?}  内存 {}",
                got,
                elapsed,
                vm_hwm_kb()
            );
            println!("   前 64 字节 = {}", hex(&first64));
            println!(
                "   是否 7z 签名(37 7a bc af 27 1c) 打头: {}",
                first64.starts_with(&SEVENZ_SIG)
            );

            println!("\n[C] by_index_seek（随机访问）能力测试");
            match a.by_index_seek(0) {
                Ok(mut z) => match std::io::Seek::seek(&mut z, std::io::SeekFrom::Start(1024)) {
                    Ok(p) => println!("   ✅ 可 seek 到 {}", p),
                    Err(e) => println!("   ❌ seek 失败: {e}"),
                },
                Err(e) => println!("   ❌ by_index_seek 失败: {e}"),
            }
            println!("\n最终峰值内存 {}", vm_hwm_kb());
        }
        Err(e) => println!("  ❌ 失败: {e}"),
    }
}
