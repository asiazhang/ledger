//! 核心交易域测试共享脚手架（crate 根测试目录唯一一份，ADR-0113 决策 6）。
//!
//! 各生产模块的测试外挂在 `<模块>/tests/`（如 `amount/tests/`、`write/protocol/tests/`），
//! 经各自生产模块的 `#[cfg(test)] mod tests;` 声明；本目录只保留跨模块复用的域语义
//! 输入构造器（[`common`]），路径段含 `tests` 自动落在守门豁免内。

pub(crate) mod common;
