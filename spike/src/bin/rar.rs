//! rars 0.11 能力验证：RAR1.3/2.x/3.x/4.x/5/7 读取、加密、分卷、SFX 前缀。
//! 用法:
//!   rar detect  <path>
//!   rar list    <path> [--pw <p>]
//!   rar test    <path> [--pw <p>]
//!   rar read    <path> <member> [--pw <p>]
//!   rar extract <path> <dest> [--pw <p>]
//!   rar volumes <dest> <vol1> <vol2> ... [--pw <p>]
use rars::{Archive, ArchiveReadOptions, ArchiveReader, ExtractedEntryMeta};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;

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
    let ms: Vec<_> = a.members().collect();
    println!(
        "   family={:?} sfx_offset={} solid={} 成员 {} 个",
        a.family(),
        a.sfx_offset(),
        a.is_solid(),
        ms.len()
    );
    for m in &ms {
        println!(
            "   - {:?} size={} packed={} dir={} enc={} split_before={} split_after={}",
            m.meta.name_lossy(),
            m.meta.unpacked_size,
            m.meta.packed_size,
            m.meta.is_directory,
            m.meta.is_encrypted,
            m.meta.is_split_before,
            m.meta.is_split_after
        );
    }
}

fn opener<'a>(dest: &'a str) -> impl FnMut(&ExtractedEntryMeta) -> rars::Result<Box<dyn Write>> + 'a {
    move |meta: &ExtractedEntryMeta| {
        let rel = String::from_utf8_lossy(&meta.name).replace('\\', "/");
        let out = Path::new(dest).join(rel);
        if meta.is_directory {
            fs::create_dir_all(&out)?;
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        }
        if let Some(par) = out.parent() {
            fs::create_dir_all(par)?;
        }
        Ok(Box::new(fs::File::create(&out)?) as Box<dyn Write>)
    }
}

fn dump_dir(dest: &str) {
    if let Ok(rd) = fs::read_dir(dest) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Ok(b) = fs::read(&p) {
                    println!(
                        "   ↳ {:?} {} 字节 sha256={}",
                        p.file_name().unwrap_or_default(),
                        b.len(),
                        sha256_hex(&b)
                    );
                }
            }
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut pw: Option<String> = None;
    if let Some(i) = args.iter().position(|a| a == "--pw") {
        if i + 1 < args.len() {
            pw = Some(args.remove(i + 1));
        }
        args.remove(i);
    }
    if args.is_empty() {
        println!("用法见源码头部注释");
        return;
    }
    let cmd = args.remove(0);
    let pwb: Option<Vec<u8>> = pw.clone().map(|s| s.into_bytes());
    println!("命令={cmd} 参数={args:?} 密码={pw:?}");

    match cmd.as_str() {
        "detect" => {
            let b = fs::read(&args[0]).unwrap();
            println!("   字节数 {}", b.len());
            match rars::detect_archive_family(&b) {
                Some(s) => println!(
                    "   ✅ detect_archive_family: family={:?} offset={} len={}",
                    s.family, s.offset, s.length
                ),
                None => println!("   ❌ 头部没有 RAR 签名"),
            }
            match rars::find_archive_start(&b, rars::SFX_SCAN_LIMIT) {
                Some(s) => println!(
                    "   find_archive_start: family={:?} offset={}",
                    s.family, s.offset
                ),
                None => println!("   find_archive_start: None"),
            }
        }
        "list" | "test" => {
            let p = args[0].clone();
            let opts = ArchiveReadOptions::with_optional_password(pwb.as_deref());
            match ArchiveReader::read_path_with_options(&p, opts) {
                Ok(a) => {
                    println!("   ✅ ArchiveReader::read_path_with_options 成功");
                    show(&a);
                    if cmd == "test" {
                        match a.test(pwb.as_deref()) {
                            Ok(()) => println!("   ✅ test() 全部校验通过"),
                            Err(e) => println!("   ❌ test() 失败: {e}"),
                        }
                    }
                    println!("   峰值内存 {}", vm_hwm());
                }
                Err(e) => println!("   ❌ 失败: {e}"),
            }
        }
        "read" => {
            let p = args[0].clone();
            let member = args[1].clone();
            let opts = ArchiveReadOptions::with_optional_password(pwb.as_deref());
            match ArchiveReader::read_path_with_options(&p, opts) {
                Ok(a) => match a.read_member(member.as_bytes(), pwb.as_deref()) {
                    Ok(Some(b)) => println!(
                        "   ✅ 读出 {:?} {} 字节 sha256={} 内容头={:?}",
                        member,
                        b.len(),
                        sha256_hex(&b),
                        String::from_utf8_lossy(&b[..b.len().min(40)])
                    ),
                    Ok(None) => println!("   ⚠️ 成员不存在（或目录）"),
                    Err(e) => println!("   ❌ 读成员失败: {e}"),
                },
                Err(e) => println!("   ❌ 打开失败: {e}"),
            }
        }
        "extract" => {
            let p = args[0].clone();
            let dest = args[1].clone();
            let opts = ArchiveReadOptions::with_optional_password(pwb.as_deref());
            match ArchiveReader::read_path_with_options(&p, opts) {
                Ok(a) => {
                    match a.extract_to(pwb.as_deref(), opener(&dest)) {
                        Ok(()) => println!("   ✅ extract_to 成功 → {dest}"),
                        Err(e) => println!("   ❌ extract_to 失败: {e}"),
                    }
                    dump_dir(&dest);
                    println!("   峰值内存 {}", vm_hwm());
                }
                Err(e) => println!("   ❌ 打开失败: {e}"),
            }
        }
        "volumes" => {
            let dest = args[0].clone();
            let vols = args[1..].to_vec();
            let mut archives = Vec::new();
            for v in &vols {
                let dopts = ArchiveReadOptions::with_optional_password(pwb.as_deref());
                match ArchiveReader::read_path_with_options(v, dopts) {
                    Ok(a) => archives.push(a),
                    Err(e) => {
                        println!("   ❌ 读卷 {v} 失败: {e}");
                        return;
                    }
                }
            }
            println!("   已解析 {} 个卷", archives.len());
            for (i, a) in archives.iter().enumerate() {
                println!(
                    "   卷[{i}] family={:?} 成员 {} 个",
                    a.family(),
                    a.members().count()
                );
            }
            let sel: Vec<usize> = (0..archives.len()).collect();
            match rars::read_volume_members_at(&archives, &sel, pwb.as_deref()) {
                Ok(res) => {
                    for (i, r) in res.iter().enumerate() {
                        match r {
                            Some(b) => println!(
                                "   卷[{i}] 拼出 {} 字节 sha256={} 头={:?}",
                                b.len(),
                                sha256_hex(b),
                                String::from_utf8_lossy(&b[..b.len().min(40)])
                            ),
                            None => println!("   卷[{i}] 无数据"),
                        }
                    }
                }
                Err(e) => println!("   ❌ read_volume_members_at 失败: {e}"),
            }
            match rars::extract_volumes_to(&archives, pwb.as_deref(), opener(&dest)) {
                Ok(()) => {
                    println!("   ✅ extract_volumes_to 成功 → {dest}");
                    dump_dir(&dest);
                }
                Err(e) => println!("   ❌ extract_volumes_to 失败: {e}"),
            }
            println!("   峰值内存 {}", vm_hwm());
        }
        "farprefix" => {
            // 前缀超过 rars 自带 SFX_SCAN_LIMIT(8MiB) 时：自己扫签名，再走 family 级 API
            let p = args[0].clone();
            let cap: u64 = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(64 * 1024 * 1024);
            let mut f = fs::File::open(&p).unwrap();
            let flen = f.metadata().unwrap().len();
            let n = flen.min(cap) as usize;
            let mut buf = vec![0u8; n];
            std::io::Read::read_exact(&mut f, &mut buf).unwrap();
            println!("   读入头部 {} 字节用于自扫（上限 {}）", n, cap);
            match rars::find_archive_start(&buf, n) {
                Some(sig) => {
                    println!(
                        "   自扫命中: family={:?} offset={} len={}",
                        sig.family, sig.offset, sig.length
                    );
                    let opts = ArchiveReadOptions::with_optional_password(pwb.as_deref());
                    let r = match sig.family {
                        rars::ArchiveFamily::Rar13 => {
                            rars::rar13::Archive::parse_path_with_signature_and_options(&p, sig, opts)
                                .map(Archive::Rar13)
                        }
                        rars::ArchiveFamily::Rar15To40 => {
                            rars::rar15_40::Archive::parse_path_with_signature(&p, sig, opts)
                                .map(Archive::Rar15To40)
                        }
                        rars::ArchiveFamily::Rar50Plus => {
                            rars::rar50::Archive::parse_path_with_signature(&p, sig, opts)
                                .map(Archive::Rar50Plus)
                        }
                        _ => {
                            println!("   ❌ 未知 family");
                            return;
                        }
                    };
                    match r {
                        Ok(a) => {
                            println!("   ✅ 自己扫 offset + family API 成功（已绕过 8MiB 上限）");
                            show(&a);
                            match a.test(pwb.as_deref()) {
                                Ok(()) => println!("   ✅ test() 全部校验通过"),
                                Err(e) => println!("   ❌ test() 失败: {e}"),
                            }
                        }
                        Err(e) => println!("   ❌ 打开失败: {e}"),
                    }
                }
                None => println!("   ❌ {} 字节内没找到 RAR 签名", n),
            }
        }
        _ => println!("未知命令 {cmd}"),
    }
}
