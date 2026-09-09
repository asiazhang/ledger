//! 币种领域模块（Currency，#404 参考数据域归位；模型 #418 随域归位）。
//!
//! 币种字典为种子权威参考数据（字典本体无写命令、无失效信号）：清单查询实现
//! 迁入本域微目录，IPC 参数解包与命令注册留在 `commands::currencies` 壳层。
//! 域不依赖壳层。币种与汇率实体集中本域 [`model`]（#417 归属原则：实体归属
//! 优先于消费方分布，汇率随币种参考数据域走），消费方经域路径显式 import。
//!
//! 本位币基准（[`base_currency`]）是字典之上的账本级设置（LedgerLevelSetting
//! 首个成员，issue #858 / ADR-0091 决策 3）：写 `app_settings`（非字典表），
//! 随多端同步分发，op 产出/重放经 [`command`]。

mod base_currency;
mod command;
mod list;
mod model;

pub use base_currency::{DEFAULT_BASE_CURRENCY, current_base_currency, set_base_currency};
pub use command::LedgerSettingCommand;
pub use list::list_currencies;
pub use model::{Currency, ExchangeRate, ExchangeRateInput};

/// 同步重放接缝（crate 内消费：`sync_engine::apply_ops` 经本接缝执行外来
/// 账本级设置命令，与本地写同协议）。
pub(crate) use command::replay_command;

#[cfg(test)]
mod tests;
