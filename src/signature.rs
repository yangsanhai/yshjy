// SPDX-License-Identifier: MIT
// Copyright (c) 2026 yangsanhai（杨三海解压）

//! 作者签名 —— 编译进二进制的溯源标记。
//!
//! **为什么作者名写死成常量、而不是构建时读 git 配置**：写死才能保证
//! 「任何人拿这份源码重新编译，产物里都带着原作者的名字」。这正是 MIT 许可证
//! 第 1 条（保留版权声明）在二进制里的落点；读构建机器的 git 用户名反而会
//! 把签名变成「谁编的就是谁的」。
//!
//! 改这里的字符串不属于「个性化配置」，是版权声明的一部分（见 LICENSE）。

/// 作者（与 `Cargo.toml` 的 authors、LICENSE 保持一致）
pub const 作者: &str = "yangsanhai";

/// 项目标识
pub const 项目: &str = "yshjy（杨三海解压）";

/// 许可证标识（SPDX）
pub const 许可: &str = "MIT";

/// `--version` 末尾那一行：分发时的可见署名（MIT 要求副本里带版权声明）
pub const 署名行: &str = "yshjy · MIT License · Copyright (c) 2026 yangsanhai";

/// 隐藏溯源标记：程序**不打印**它，但它一定躺在二进制的只读数据段里。
///
/// `#[used]` 阻止链接器把它当死数据丢掉；不加的话优化后可能整个消失。
/// 用途：别人把自己产品里的可疑二进制丢给 `strings | grep yangsanhai`，
/// 就能看出「这是从 yshjy 的核心来的」。
///
/// 注意：删掉它本身不违法（MIT 允许改），但同时抹掉源码里的
/// `SPDX-License-Identifier` / `Copyright` 头就违反 MIT 条款了 —— 这才是可举证的点。
#[used]
pub static 溯源标记: &str =
    "yshjy|MIT|Copyright (c) 2026 yangsanhai|github.com/yangsanhai|original-author-marker";
