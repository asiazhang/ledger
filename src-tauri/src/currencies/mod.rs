//! 币种领域模块（Currency，#404 参考数据域归位；模型 #418 随域归位）。
//!
//! 币种为种子权威参考数据（无写命令、无失效信号）：清单查询实现迁入本域微目录，
//! IPC 参数解包与命令注册留在 `commands::currencies` 壳层。域不依赖壳层。
//! 币种与汇率实体集中本域 [`model`]（#417 归属原则：实体归属优先于消费方分布，
//! 汇率随币种参考数据域走），消费方经域路径显式 import。

mod list;
mod model;

pub use list::list_currencies;
pub use model::{Currency, ExchangeRate, ExchangeRateInput};

#[cfg(test)]
mod tests;
