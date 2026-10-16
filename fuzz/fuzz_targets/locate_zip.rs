//! fuzz：ZIP 定位（`layout::zip::locate`）—— 伪装包最核心的一步
//!
//! # 为什么它最该被猛啃
//! 它会**按字节解引用**：读尾部窗口、找 EOCD、解析 ZIP64 定位器与 EOCD64、
//! 反推基准偏移、再做结构自校验。里面全是偏移算术（`checked_sub`、相对/绝对
//! 位置换算、小端读整数）—— 越界与下溢最容易出在这一块。
//!
//! 这一步是「别人都做不到」的地方（我们靠自己定位，不靠文件头），
//! 所以它也必须是最不容易被畸形输入打穿的地方。
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let mut cur = Cursor::new(data);
    // 输入自己就是"整个文件"，长度取真实长度
    let _ = yshjy::layout::zip::locate(&mut cur, data.len() as u64);
});
