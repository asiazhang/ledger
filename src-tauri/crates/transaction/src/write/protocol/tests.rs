//! 写入协议模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`behavior`]：写删行为、refund 链、嵌套感知事务与即建商户证据
//! - [`protocol`]：写入协议协议级对称断言（Local / Replay 两形态，ADR-0105）
//! - [`category`]：分类携带收口（issue #582）
//! - [`merchant`]：商户携带收口与即建商户证据
//! - [`oplog`]：op 产出——写成功追加 op、失败不残留（issue #855）

mod behavior;
mod category;
mod merchant;
mod oplog;
mod protocol;
