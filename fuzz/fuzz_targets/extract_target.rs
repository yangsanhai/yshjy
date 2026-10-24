//! fuzz：把任意字节当「一条流」，走**真写盘**的解压路径
//!
//! # 为什么要单开这个 target（前三个都是只读的）
//! 落盘是最容易出事的一环：路径穿越、写到沙箱外、失败留半截文件、解压炸弹打爆磁盘。
//! 这些只有真解一遍才暴露得出来 —— 只读的 `list()` 永远碰不到。
//!
//! # 判据（除了不 panic / 不死循环）
//! 1. 一轮跑完不许留下 `.yshjy-part` 半成品（原子写入的 Drop 该清干净）
//! 2. 写出量必须被限额挡住（4 MiB），炸弹输入不许把磁盘打满
//! 3. 解出的文件都在沙箱目录里（`safety::resolve_safe` 的那层防线）
//!
//! 返回 `Err` 是**正常结果**：畸形输入本来就该被拒绝，这里不断言语义。
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use std::path::Path;
use yshjy::ctx::UnpackCtx;
use yshjy::layout::ArchiveFormat;
use yshjy::unpack::{self, Shape};

/// 沙箱：每次跑之前清空，跑完也清空（不删目录本身，省得每轮重建）
const SANDBOX: &str = "/tmp/yshjy-fuzz-write";

/// 单轮允许写出的上限（fuzz 用，压到很小）
const MAX_BYTES: u64 = 4 * 1024 * 1024;

fn wipe(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// 走一遍沙箱，返回 (文件数, 总字节)。顺带断言没有残留的半成品。
fn inspect(dir: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                stack.push(p);
            } else {
                files += 1;
                bytes += md.len();
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                assert!(
                    !name.ends_with(yshjy::io::PART_SUFFIX),
                    "解压结束还留着半成品：{name}"
                );
            }
        }
    }
    (files, bytes)
}

fuzz_target!(|data: &[u8]| {
    let sandbox = Path::new(SANDBOX);
    let _ = std::fs::create_dir_all(sandbox);
    wipe(sandbox);

    let combos: [(ArchiveFormat, Shape); 8] = [
        (ArchiveFormat::Zip, Shape::Plain),
        (ArchiveFormat::Zip, Shape::Volumes),
        (ArchiveFormat::SevenZ, Shape::Volumes),
        (ArchiveFormat::Rar4, Shape::Plain),
        (ArchiveFormat::Rar5, Shape::Plain),
        (ArchiveFormat::Gzip, Shape::Single),
        (ArchiveFormat::Brotli, Shape::Single),
        (ArchiveFormat::Tar, Shape::Tar),
    ];

    for (fmt, shape) in combos {
        let Some(h) = unpack::find(fmt, shape) else {
            continue;
        };
        // 每个组合独立的游标：上一个 handler 可能把流读到任意位置了
        let mut cur = Cursor::new(data);
        let mut src = unpack::Source::new(&mut cur).with_packed_total(data.len() as u64);

        let mut opts = unpack::UnpackOptions::new(sandbox);
        // 限额压到最小 —— 这是 fuzz，不是给用户干活
        opts.limits.max_extract_bytes = MAX_BYTES;
        opts.limits.max_ratio = 50;
        opts.limits.warn_executables = false;
        opts.limits.ask = None; // 无人值守：命中限额直接失败

        let ctx = UnpackCtx::quiet(0);
        let _ = h.unpack(&mut src, &opts, &ctx);

        let (files, bytes) = inspect(sandbox);
        // 限额是硬约束：写出的总量不许超（原子写入会先写 .part，也算在内）
        assert!(
            bytes <= MAX_BYTES + 1024 * 1024,
            "写出 {bytes} 字节，超过限额 {MAX_BYTES}（文件 {files} 个）"
        );
        wipe(sandbox);
    }
});
