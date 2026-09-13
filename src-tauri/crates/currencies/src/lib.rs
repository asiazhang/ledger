// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! 币种领域 crate（Currency，spec #1086 / issue #1095 自根包域目录拆出，P3 叶子
//! 域；#404 参考数据域归位，模型 #418 随域归位；参考数据三域各自独立 crate、
//! 不合并）：币种字典与汇率实体、清单查询、本位币基准（账本级设置）与账本级
//! 设置同步命令的单一权威。
//!
//! 币种字典为种子权威参考数据（字典本体无写命令、无失效信号）：清单查询实现
//! 住 `list`，IPC 参数解包与命令注册留在 `commands::currencies` 壳层。
//! 币种与汇率实体集中 `model`（#417 归属原则：实体归属优先于消费方分布，
//! 汇率随币种参考数据域走），消费方经域路径显式 import。
//!
//! 本位币基准（`base_currency`）是字典之上的账本级设置（LedgerLevelSetting
//! 首个成员，issue #858 / ADR-0091 决策 3）：写 `app_settings`（非字典表），
//! 随多端同步分发，op 产出/重放经 `command`。
//!
//! 依赖方向（spec #1086 / ADR-0112 / issue #1095 AC）：本域消费基础设施、同步
//! 协议与核心交易域（`ledger-infra` / `ledger-sync-protocol` /
//! `ledger-transaction`），对根包（壳层）与任何同级业务域零依赖——本位币基准
//! 读取对核心交易域的供给经 [`install_base_currency_hook`]
//! 注册进交易域接缝（`transaction::base_currency_seam`，下层提供实现、壳层启动
//! 接线，ADR-0112 决策 5），反向引用由 cargo 依赖图编译期拒绝（生产依赖面无
//! 根包，dev-dependency 环只覆盖测试目标；机器面负向核对住结构守门的 crate
//! 依赖方向，Cargo.toml 注释留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-currencies` 的依赖图内存在本 crate 的两份实例——被测
//! 本实例与根包图内实例（tauri-app 经生产依赖携带）。本域测试不消费交易域接缝
//! 注册静态（清单查询、本位币读写与校验均为直驱路径；唯一接缝供给面
//! [`install_base_currency_hook`] 不在域单测覆盖内，由壳层接线
//! 点 `transaction_wiring` 与交易域消费路径的既有测试守卫），全部用例直接驱动
//! 本实例，仅测试数据库工厂经 dev-dependency 取自根包
//!（`tauri_app_lib::test_support::open`，ADR-0084）。

mod base_currency;
mod command;
mod list;
mod model;

pub use base_currency::{
    DEFAULT_BASE_CURRENCY, current_base_currency, install_base_currency_hook, set_base_currency,
};
pub use command::LedgerSettingCommand;
pub use list::list_currencies;
pub use model::{Currency, ExchangeRate, ExchangeRateInput};

/// 同步重放接缝（跨 crate 消费：sync_engine::apply_ops 经本接缝执行外来
/// 账本级设置命令，与本地写同协议）。
pub use command::replay_command;

#[cfg(test)]
mod tests;
