//! sevenz-rust2 能力验证：列表 / 解压 / 密码（含中文）/ 分卷 / 非零偏移。
//!
//! 用法:
//!   p7z list      <path> [--pw <p>]              直接走文件路径
//!   p7z through   <path> [--pw <p>]              通过 File 走 Archive::read
//!   p7z extract   <path> <dest> [--pw <p>]
//!   p7z prefix    <path> <base> [--pw <p>]       自定义 Window 从 base 开始
//!   p7z autoprefix <path> [--pw <p>]             自扫 7z 签名再 Window
//!   p7z concat    <dest> [--pw <p>] <vol1> <vol2> ...
//!   p7z tailverify <path> [--pw <p>]             直接 open 分卷首卷（应失败）
use sevenz_rust2::{Archive, ArchiveReader, Password};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek};
use yshjy_spike::*;

fn sha256_hex(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().iter().map(|x| format!("{x:02x}")).collect()
}

fn vm_hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM ?")
        .trim()
        .to_string()
}

fn show(a: &Archive) {
    println!("   条目 {} 个, solid={}", a.files.len(), a.is_solid);
    for e in &a.files {
        println!(
            "   - {:?} size={} dir={} has_stream={} crc={:08x}",
            e.name, e.size, e.is_directory, e.has_stream, e.crc
        );
    }
}

fn extract_into<R: Read + Seek>(
    archive: Archive,
    source: R,
    password: Password,
    dest: &str,
) -> Result<(), sevenz_rust2::Error> {
    fs::create_dir_all(dest)?;
    let mut reader = ArchiveReader::from_archive(archive, source, password);
    reader.for_each_entries(|entry, r| {
        let out = std::path::Path::new(dest).join(&entry.name);
        if entry.is_directory {
            fs::create_dir_all(&out)?;
            return Ok(true);
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p)?;
        }
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        fs::write(&out, &buf)?;
        println!("   + {} {} 字节 sha256={}", entry.name, buf.len(), sha256_hex(&buf));
        Ok(true)
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut pw_raw: Option<String> = None;
    if let Some(i) = args.iter().position(|a| a == "--pw") {
        if i + 1 < args.len() {
            pw_raw = Some(args.remove(i + 1));
        }
        args.remove(i);
    }
    if args.is_empty() {
        println!("用法见源码头部注释");
        return;
    }
    let cmd = args.remove(0);
    let pw: Password = match &pw_raw {
        Some(p) => Password::from(p.as_str()),
        None => Password::empty(),
    };
    println!("命令={cmd} 参数={args:?} 密码={:?}", pw_raw);
    println!("Password 内部字节数 = {} (UTF-16LE)", pw.as_slice().len());

    match cmd.as_str() {
        "list" => {
            let p = &args[0];
            match Archive::open_with_password(p, &pw) {
                Ok(a) => {
                    println!("   ✅ Archive::open_with_password 成功");
                    show(&a);
                }
                Err(e) => println!("   ❌ 失败: {e}"),
            }
        }
        "through" => {
            let p = &args[0];
            let mut f = fs::File::open(p).unwrap();
            match Archive::read(&mut f, &pw) {
                Ok(a) => {
                    println!("   ✅ Archive::read(&mut File) 成功");
                    show(&a);
                }
                Err(e) => println!("   ❌ 失败: {e}"),
            }
        }
        "extract" => {
            let p = args[0].clone();
            let dest = args[1].clone();
            match sevenz_rust2::decompress_file_with_password(&p, &dest, pw.clone()) {
                Ok(()) => println!("   ✅ decompress_file_with_password 成功 → {dest}"),
                Err(e) => println!("   ❌ 失败: {e}"),
            }
            println!("   峰值内存 {}", vm_hwm());
        }
        "prefix" => {
            let p = args[0].clone();
            let base: u64 = args[1].parse().unwrap();
            let w = Window::open_to_end(&p, base).unwrap();
            match Archive::read(&mut { w }, &pw) {
                Ok(a) => {
                    println!("   ✅ Window(base={base}) + Archive::read 成功");
                    show(&a);
                }
                Err(e) => println!("   ❌ Window 失败: {e}"),
            }
        }
        "autoprefix" => {
            let p = args[0].clone();
            match find_sig(&p, &SEVENZ_SIG, 64 * 1024 * 1024) {
                Ok(Some(base)) => {
                    println!("   自扫 7z 签名 @ {base}");
                    let w = Window::open_to_end(&p, base).unwrap();
                    match Archive::read(&mut { w }, &pw) {
                        Ok(a) => {
                            println!("   ✅ 自动基准 + Window + Archive::read 成功");
                            show(&a);
                        }
                        Err(e) => println!("   ❌ 失败: {e}"),
                    }
                }
                Ok(None) => println!("   ❌ 64MiB 内没找到 7z 签名"),
                Err(e) => println!("   ❌ 出错: {e}"),
            }
        }
        "concat" => {
            let dest = args[0].clone();
            let vols: Vec<String> = args[1..].to_vec();
            println!("   分卷: {vols:?}");
            let c = Concat::open(&vols).unwrap();
            match Archive::read(&mut { c }, &pw) {
                Ok(a) => {
                    println!("   ✅ Concat + Archive::read 成功");
                    show(&a);
                    let c2 = Concat::open(&vols).unwrap();
                    let _ = extract_into(a, c2, pw.clone(), &dest);
                    println!("   峰值内存 {}", vm_hwm());
                }
                Err(e) => println!("   ❌ 失败: {e}"),
            }
        }
        "tailverify" => {
            let p = &args[0];
            let mut f = fs::File::open(p).unwrap();
            match Archive::read(&mut f, &pw) {
                Ok(a) => {
                    println!("   ✅ 首卷单独打开成功（意外）");
                    show(&a);
                }
                Err(e) => println!("   ❌ 首卷单独打开失败（预期）: {e}"),
            }
        }
        _ => println!("未知命令 {cmd}"),
    }
}
