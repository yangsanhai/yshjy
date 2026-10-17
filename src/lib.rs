//! yshjy 引擎（杨三海解压）
//!
//! # 设计约束（见 docs/ARCHITECTURE.md）
//! - **不依赖任何 UI 框架** —— CLI / 桌面 GUI / 手机共用同一套引擎
//! - **不产出面向用户的中文文案** —— 只产出「错误码 + 结构化参数」，渲染交给外壳
//! - **所有 I/O 走 trait 抽象** —— 便于测试，也便于手机端（SAF 不支持随机定位）适配
//! - **跨平台** —— 不使用平台特有 API，路径一律 `PathBuf`，不用 unsafe
//!
//! # 模块地图
//! - [`error`]  错误码体系
//! - [`io`]     I/O 抽象（偏移窗口 / 分段虚接 / 可中断读取）
//! - [`safety`] 安全护栏（路径穿越 / 空间 / 内存）

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub mod ask;
pub mod batch;
pub mod config;
pub mod cover_extract;
pub mod ctx;
pub mod drill;
pub mod error;
pub mod event;
pub mod extract;
pub mod extract7z;
pub mod extract_rar;
pub mod extract_stream;
pub mod hardware;
pub mod i18n;
pub mod io;
pub mod job;
pub mod layout;
pub mod pack;
pub mod pack_post;
pub mod password_hint;
pub mod paths;
pub mod preview;
pub mod probe;
pub mod progress;
pub mod repair;
pub mod report;
pub mod rescue;
pub mod restore;
pub mod safety;
pub mod scan;
pub mod select;
pub mod tools;
pub mod trash;
pub mod unpack;

pub use error::{Error, ErrorCode, Result};

/// 引擎版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
