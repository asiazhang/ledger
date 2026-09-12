//! 跨域接缝区（ADR-0113 决策 2）：本域与外域之间的注册点与契约类型。
//!
//! 只持契约与注册点，不含消费逻辑；依赖方向唯一——接缝只可依赖共享语义
//! （`amount` / `model` / `command` / `search_text`），不得依赖写路径与读路径。
//! 实现由提供域装入、壳层启动时接线（ADR-0112 决策 5）。未注册即码化错误。

pub mod balance;
pub mod funding;
pub mod investment;
pub mod merchant;
pub mod source;
