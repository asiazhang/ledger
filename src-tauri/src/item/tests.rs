//! `item` 域单元测试索引（issue #397 域目录化随迁，外挂测试目录先例：
//! `transaction::writer` 测试目录）。
//!
//! - [`cost`] — `item::cost` 接缝：日历天数（含起止日）与分子下限（issue #114）
//! - [`crud`] — 域 API（`item::domain`）校验语义与失效信号回调（BDD 场景外的快速反馈）

mod cost;
mod crud;
