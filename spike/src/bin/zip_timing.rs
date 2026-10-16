//! 精确定时：自研定位 vs zip crate 的 Detect / Known 两条路径。
//! 用法: zip_timing <path>
use std::fs::File;
use std::io::Read as _;
use std::time::Instant;
use yshjy_spike::*;

fn main() {
    let path = std::env::args().nth(1).expect("用法: zip_timing <path>");
    let fsz = File::open(&path).unwrap().metadata().unwrap().len();
    println!("文件 {} 字节 ({:.4} GB)", fsz, fsz as f64 / 1e9);

    let t = Instant::now();
    let info = detect_zip_base(&path).unwrap().unwrap();
    println!(
        "[自研定位]                {:>10.3?}   Δ_cd={}",
        t.elapsed(),
        info.delta_cd
    );

    let t = Instant::now();
    let mut a = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    println!("[Detect + new]            {:>10.3?}", t.elapsed());
    {
        let mut zf = a.by_index(0).unwrap();
        let mut b = vec![0u8; 16];
        let n = zf.read(&mut b).unwrap();
        println!("   （entry0 前 16 字节 {}）", hex(&b[..n]));
    }

    let t = Instant::now();
    let cfg = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(info.delta_cd as u64),
    };
    let a2 = zip::ZipArchive::with_config(cfg, File::open(&path).unwrap());
    println!("[Known(Δ_cd) + with_config] {:>10.3?}  ok={}", t.elapsed(), a2.is_ok());

    let t = Instant::now();
    let w = Window::open_to_end(&path, info.delta_cd as u64).unwrap();
    let a3 = zip::ZipArchive::new(w);
    println!("[Window(Δ_cd) + Detect]    {:>10.3?}  ok={}", t.elapsed(), a3.is_ok());
}
