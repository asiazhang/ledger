//! 币种领域模块（Currency，#404 参考数据域归位）。
//!
//! 币种为种子权威参考数据（无写命令、无失效信号）：清单查询实现迁入本域微目录，
//! IPC 参数解包与命令注册留在 `commands::currencies` 壳层。域不依赖壳层。

mod list;

pub use list::list_currencies;

#[cfg(test)]
mod tests;
