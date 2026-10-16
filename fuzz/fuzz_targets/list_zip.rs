//! fuzz：把任意字节当「一条流」，走统一接口的**列清单**路径
//!
//! # 为什么挑这条路（性价比最高的入口）
//! `list()` 是**纯读**的（不解压、不写盘），却会一次走遍所有格式的头部解析：
//! ZIP 的中央目录、7z 的头、RAR 的成员头、单流的魔数嗅探、tar 的块结构 ——
//! 一次输入覆盖全部五个 handler。
//!
//! # 判据
//! 只要求 **不 panic / 不越界 / 不无限循环**。返回 `Err` 是**正常结果**
//! （畸形输入本来就该被拒绝），这里不断言语义。
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use yshjy::ctx::UnpackCtx;
use yshjy::layout::ArchiveFormat;
use yshjy::unpack::{self, Shape};

fuzz_target!(|data: &[u8]| {
    let 组合: [(ArchiveFormat, Shape); 8] = [
        (ArchiveFormat::Zip, Shape::Plain),
        (ArchiveFormat::Zip, Shape::Volumes),
        (ArchiveFormat::SevenZ, Shape::Volumes),
        (ArchiveFormat::Rar4, Shape::Plain),
        (ArchiveFormat::Rar5, Shape::Plain),
        (ArchiveFormat::Gzip, Shape::Single),
        (ArchiveFormat::Brotli, Shape::Single),
        (ArchiveFormat::Tar, Shape::Tar),
    ];
    let ctx = UnpackCtx::quiet(0);
    for (fmt, shape) in 组合 {
        // 每个组合用**独立的游标**：上一个 handler 可能已经把流读到了任意位置
        let mut cur = Cursor::new(data);
        let mut src = unpack::Source::new(&mut cur).with_packed_total(data.len() as u64);
        // 注意：这个目录**不会**被写入 —— list 只读
        let opts = unpack::UnpackOptions::new("/tmp/yshjy-fuzz-只读不写");
        if let Some(h) = unpack::find(fmt, shape) {
            let _ = h.list(&mut src, &opts, &ctx);
        }
    }
});
