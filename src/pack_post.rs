// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）
//! 打包后处理：分卷 / 校验和 / 自动说明书（DESIGN §14.2 / §17.8）
//!
//! 放在打包核心之外：核心只管「拼出一个合法的伪装包」，这里管「交付形态」。
//! 三件事互相独立，任何一件失败都不该把已经打好的包判成失败。
//!
//! 注意：**分卷会删掉未切分的大文件**（用户要的就是分卷，留一份完整的等于占双倍空间）。

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorCode, Result};

/// 后处理选项
#[derive(Debug, Clone)]
pub struct PostOptions {
    /// 每卷字节数（`None` = 不分卷）
    pub split: Option<u64>,
    /// 生成 `.sha256`（sha256sum 兼容）
    pub checksum: bool,
    /// 生成旁边的「说明.txt」（告诉对方这是什么、怎么打开）
    pub readme: bool,
    /// 写在说明书里的解压码（`None` = 不写）
    pub password: Option<String>,
}

impl Default for PostOptions {
    fn default() -> Self {
        Self {
            split: None,
            checksum: true,
            readme: true,
            password: None,
        }
    }
}

/// 后处理结果
#[derive(Debug, Clone, Default)]
pub struct PostReport {
    /// 分卷列表（没分卷 = 空）
    pub volumes: Vec<PathBuf>,
    /// 校验和文件
    pub checksum: Option<PathBuf>,
    /// 说明书文件
    pub readme: Option<PathBuf>,
}

/// 解析 `2G` / `500M` / `1048576` 这种大小写法
pub fn parse_size(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_uppercase();
    if t.is_empty() {
        return None;
    }
    let (num, mult) = if let Some(x) = t.strip_suffix("GB").or_else(|| t.strip_suffix("G")) {
        (x, 1024u64 * 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("MB").or_else(|| t.strip_suffix("M")) {
        (x, 1024 * 1024)
    } else if let Some(x) = t.strip_suffix("KB").or_else(|| t.strip_suffix("K")) {
        (x, 1024)
    } else if let Some(x) = t.strip_suffix('B') {
        (x, 1)
    } else {
        (t.as_str(), 1)
    };
    let v: f64 = num.trim().parse().ok()?;
    if v <= 0.0 {
        return None;
    }
    Some((v * mult as f64) as u64)
}

/// 对刚打好的包做交付形态处理
pub fn postprocess(out: &Path, o: &PostOptions) -> Result<PostReport> {
    let mut rep = PostReport::default();

    // ① 校验和要在分卷**之前**算（分卷后原文件就没了；也想给出完整包的哈希）
    let full_hash = if o.checksum { Some(sha256_file(out)?) } else { None };

    // ② 分卷
    if let Some(size) = o.split {
        rep.volumes = split_file(out, size)?;
    }

    // ③ 校验和文件（分卷后写各卷的，否则写整体的）
    if let Some(h) = full_hash {
        let path = PathBuf::from(format!("{}.sha256", out.display()));
        let mut text = String::new();
        if rep.volumes.is_empty() {
            let name = out.file_name().map(|s| s.to_string_lossy()).unwrap_or_default();
            text.push_str(&format!("{h}  {name}\n"));
        } else {
            for v in &rep.volumes {
                let vh = sha256_file(v)?;
                let name = v.file_name().map(|s| s.to_string_lossy()).unwrap_or_default();
                text.push_str(&format!("{vh}  {name}\n"));
            }
        }
        std::fs::write(&path, text)
            .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("写校验和失败: {e}")))?;
        rep.checksum = Some(path);
    }

    // ④ 说明书
    if o.readme {
        let path = PathBuf::from(format!("{}.说明.txt", out.display()));
        let name = out.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let pw = o
            .password
            .as_ref()
            .map(|p| format!("解压码：{p}\n（别公开贴在评论区，私发对方）\n"))
            .unwrap_or_default();
        let text = format!(
            "【这是什么】\n\
             不是视频、不是图片 —— 是一个压缩包，故意改成这个后缀躲网盘审核。\n\
             文件名：{name}\n\n\
             【怎么打开】\n\
             用「杨三海解压」（yshjy）把这个文件拖进去，或者：\n\
             \x20 yshjy \"{name}\"\n\
             {pw}\n\
             【打不开怎么办】\n\
             普通播放器当然打不开。其他解压软件多半也认不出（这正是伪装的目的）。\n\
             缺分卷时把 .001/.002… 全部下下来放同一个目录，再由第一块打开。\n"
        );
        std::fs::write(&path, text)
            .map_err(|e| Error::new(ErrorCode::PermissionDenied).context(format!("写说明书失败: {e}")))?;
        rep.readme = Some(path);
    }

    Ok(rep)
}

/// 按大小切分，命名 `<原名>.001` / `.002` …
fn split_file(out: &Path, size: u64) -> Result<Vec<PathBuf>> {
    let src = File::open(out)
        .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open: {e}")))?;
    let mut r = BufReader::new(src);
    let mut volumes = Vec::new();
    let mut idx = 1u32;

    loop {
        let path = PathBuf::from(format!("{}.{idx:03}", out.display()));
        let f = File::create(&path).map_err(|e| {
            Error::new(ErrorCode::PermissionDenied).context(format!("建分卷失败: {e}"))
        })?;
        let mut w = BufWriter::with_capacity(1 << 20, f);
        let copied = std::io::copy(&mut (&mut r).take(size), &mut w)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("写分卷失败: {e}")))?;
        w.flush()
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("flush: {e}")))?;
        if copied == 0 {
            // 空卷（正好整除时不产生多余的尾卷）
            let _ = std::fs::remove_file(&path);
            break;
        }
        volumes.push(path);
        if copied < size {
            break;
        }
        idx += 1;
    }

    // 分卷取代原文件：留着完整包等于占双倍空间（用户要的就是分卷交付）
    let _ = std::fs::remove_file(out);
    Ok(volumes)
}

fn sha256_file(path: &Path) -> Result<String> {
    let f = File::open(path)
        .map_err(|e| Error::new(ErrorCode::PathNotFound).context(format!("open: {e}")))?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r
            .read(&mut buf)
            .map_err(|e| Error::new(ErrorCode::Internal).context(format!("read: {e}")))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析大小写法() {
        assert_eq!(parse_size("2G"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("500M"), Some(500 * 1024 * 1024));
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("0"), None);
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn 校验和是标准_sha256() {
        let d = std::env::temp_dir().join(format!("yshjy_post_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("x.bin");
        // "abc" 的 SHA-256 是公认值
        std::fs::write(&f, b"abc").unwrap();
        let o = PostOptions {
            split: None,
            checksum: true,
            readme: false,
            password: None,
        };
        let rep = postprocess(&f, &o).unwrap();
        let text = std::fs::read_to_string(rep.checksum.unwrap()).unwrap();
        assert!(text.starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 分卷按大小切开且原包被移除() {
        let d = std::env::temp_dir().join(format!("yshjy_split_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("包.mp4");
        std::fs::write(&f, vec![7u8; 25]).unwrap();
        let o = PostOptions {
            split: Some(10),
            checksum: false,
            readme: false,
            password: None,
        };
        let rep = postprocess(&f, &o).unwrap();
        assert_eq!(rep.volumes.len(), 3, "25 字节按 10 切 = 3 卷");
        assert!(!f.exists(), "分卷后原包应被移除");
        let sizes: Vec<u64> = rep.volumes.iter().map(|p| std::fs::metadata(p).unwrap().len()).collect();
        assert_eq!(sizes, vec![10, 10, 5]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 说明书能写进解压码() {
        let d = std::env::temp_dir().join(format!("yshjy_readme_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.mp4");
        std::fs::write(&f, b"x").unwrap();
        let o = PostOptions {
            split: None,
            checksum: false,
            readme: true,
            password: Some("秘密123".into()),
        };
        let rep = postprocess(&f, &o).unwrap();
        let text = std::fs::read_to_string(rep.readme.unwrap()).unwrap();
        assert!(text.contains("秘密123"));
        assert!(text.contains("不是视频"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
