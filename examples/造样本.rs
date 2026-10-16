//! 造测试样本 —— 生成各种格式的压缩文件（给手工验证 / 冒烟用）
//!
//! 用法：
//! ```text
//! cargo run --example 造样本 -- /tmp/样本目录
//! ```
//!
//! 为什么要专门造：系统上不一定装了 `brotli` / `lz4` 这些命令，
//! 而验证「按扩展名识别 + 真能解开」又必须有一份真实文件。
//!
//! 注意：这里用的数据是**故意不太能压**的（伪随机），因为高度可压缩的内容
//! 会触发压缩炸弹防护（那是安全功能在正常工作，不该为了测试去放宽限值）。

use std::io::Write;

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => std::path::PathBuf::from(d),
        None => {
            eprintln!("用法：cargo run --example 造样本 -- <输出目录>");
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(&dir).expect("建目录失败");

    // 伪随机数据：压缩比接近 1，不会撞上炸弹防护
    let data: Vec<u8> = (0..20000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    std::fs::write(dir.join("样本.txt"), &data).expect("写原始数据失败");

    // ① Brotli（没有魔数，只能靠 `.br` 认）
    {
        let mut out = Vec::new();
        {
            let mut w = brotli::CompressorWriter::new(&mut out, 4096, 6, 22);
            w.write_all(&data).expect("brotli 压缩失败");
        }
        let p = dir.join("样本.txt.br");
        std::fs::write(&p, &out).expect("写 brotli 失败");
        println!("  {} （{} 字节）", p.display(), out.len());
    }

    // ② LZ4 帧格式（有魔数，能靠头部认出来）
    {
        let mut enc = lz4_flex::frame::FrameEncoder::new(Vec::new());
        enc.write_all(&data).expect("lz4 压缩失败");
        let out = enc.finish().expect("lz4 收尾失败");
        let p = dir.join("样本.txt.lz4");
        std::fs::write(&p, &out).expect("写 lz4 失败");
        println!("  {} （{} 字节）", p.display(), out.len());
    }

    println!("\n原始数据（对照用）：{}", dir.join("样本.txt").display());
}
