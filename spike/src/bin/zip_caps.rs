//! ZIP 能力矩阵：压缩方法 / 加密（ZipCrypto / AES）/ legacy-zip。
//! 用法: zip_caps
use std::fs::File;
use std::io::Read as _;

fn try_open(path: &str, pw: Option<&[u8]>) -> String {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) => return format!("打不开文件: {e}"),
    };
    let mut a = match zip::ZipArchive::new(f) {
        Ok(a) => a,
        Err(e) => return format!("new() 失败: {e}"),
    };
    let n = a.len();
    let mut read_ok = 0;
    let mut errs = Vec::new();
    let mut info = Vec::new();
    for i in 0..n {
        let r = match pw {
            Some(p) => a.by_index_decrypt(i, p),
            None => a.by_index(i),
        };
        match r {
            Ok(mut zf) => {
                let name = zf.name().map(|s| s.to_string()).unwrap_or_default();
                let comp = format!("{:?}", zf.compression());
                let sz = zf.size();
                let csz = zf.compressed_size();
                let enc = zf.encrypted();
                let mut buf = Vec::new();
                match zf.read_to_end(&mut buf) {
                    Ok(_) => {
                        read_ok += 1;
                        info.push(format!("{name}({comp},{csz}->{sz},enc={enc},ok)"));
                    }
                    Err(e) => {
                        info.push(format!("{name}({comp},enc={enc},读失败:{e})"));
                        errs.push(format!("[{i}] {e}"));
                    }
                }
            }
            Err(e) => {
                info.push(format!("[{i}] 打不开"));
                errs.push(format!("[{i}] {e}"));
            }
        }
    }
    format!(
        "entries={n} 成功读出 {read_ok}/{n}\n      {info:?}{}",
        if errs.is_empty() {
            String::new()
        } else {
            format!("\n      错误: {errs:?}")
        }
    )
}

fn main() {
    println!("═══ zip crate 默认编译进去的压缩方法");
    println!("   {:?}", zip::SUPPORTED_COMPRESSION_METHODS);

    let cases: &[(&str, Option<&[u8]>)] = &[
        ("testdata/plain.zip", None),
        ("testdata/zipcrypto.zip", None),
        ("testdata/zipcrypto.zip", Some(b"Secret123")),
        ("testdata/zipcrypto.zip", Some(b"WRONG")),
        ("testdata/aes256.zip", Some(b"Secret123")),
        ("testdata/aes256.zip", Some(b"WRONG")),
        ("testdata/bzip2.zip", None),
        ("testdata/lzma.zip", None),
        ("testdata/ppmd.zip", None),
        ("testdata/d64.zip", None),
        ("testdata/zip_rs/aes_archive.zip", Some(b"helloworld")),
        ("testdata/zip_rs/chinese.zip", None),
        ("testdata/zip_rs/lin-ub_iwd-v11.zip", None),
        ("testdata/zip_rs/implode.zip", None),
        ("testdata/zip_rs/shrink.zip", None),
        ("testdata/zip_rs/shrink_full_table.zip", None),
        ("testdata/zip_rs/reduce.zip", None),
        ("testdata/zip_rs/invalid_offset.zip", None),
        ("testdata/zip_rs/invalid_offset2.zip", None),
        ("testdata/fake_zip_after_junk.bin", None),
    ];

    for (p, pw) in cases {
        println!(
            "\n── {p}  密码={:?}",
            pw.map(|x| String::from_utf8_lossy(x).to_string())
        );
        println!("   {}", try_open(p, *pw));
    }
}
