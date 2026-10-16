//! fuzz：外部数据进门的第一道关 —— 单流格式识别 + 文件名兜底判断
//!
//! `extract_stream::detect()` 吃的是文件头几十字节，决定"这到底是什么格式"；
//! 它错了后面全错。旁边还顺手喂一遍 `.tar.gz` / `.br` 的**按名字判断**分支
//! （Brotli 没有魔数，只能按名字认 —— 这条分支同样吃不受信输入）。
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    let _ = yshjy::extract_stream::detect(data);

    // 把输入同时当"文件名"用一次：覆盖按扩展名判断的那几条分支
    // （文件名来自用户/下载目录，同样是外部输入）
    if let Ok(名字) = std::str::from_utf8(data) {
        if 名字.len() < 256 {
            let p = Path::new(名字);
            let _ = yshjy::extract_stream::looks_like_tar_by_name(p);
            let _ = yshjy::extract_stream::looks_like_brotli_by_name(p);
        }
    }
});
