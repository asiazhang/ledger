//! 重放注册表（ADR-0101）：14 个语义命令类型的适配绑定 + `DomainCommand::subject`
//! 的组装臂。与 [`super::ops`] / [`super::parked`] 平级；依赖方向保持
//! registry → command 单向（绑定 wrap 域侧既有接缝，域侧不认识本文件）。
//!
//! - **适配绑定**（[`ReplayBinding`]）：每个语义命令类型一个零尺寸绑定，`ENTITY` +
//!   `subject()` + `replay()` 单点承载可重放契约；域侧 6 个函数名与 2 种返回形状
//!   （`Result<()>` × 13、`Result<ReplayEffect>` × 1）由绑定吸收，域侧除裁决键派生
//!   外零改动（ADR-0101 决策 1/3）。
//! - **subject 组装**：实体标签自 #1089 起单源取域命令类型的
//!   `SyncCommand::ENTITY`（协议 crate 契约；绑定常量派生，不再另立字面量——
//!   标签宇宙 = {serde derive, `SyncCommand::ENTITY`} 各 14 处，门 a 维持二源
//!   断言），实体键由命令类型派生（域自身知识，`SyncCommand::subject`）；
//!   `impl DomainCommand` 的第二个 impl 块系刻意安排（标签单源化，ADR-0101 勘误 3），
//!   非散落。
//! - **注册完备（门 c）**：`engine::dispatch` 与本文件的 `DomainCommand::subject`
//!   两处穷尽 match 各引用全部绑定，删任一绑定即编译红——无需额外机制。

use std::borrow::Cow;

use rusqlite::Connection;

use crate::error::Result;

use ledger_sync_protocol::command::SyncCommand;

use super::command::{DomainCommand, ReplayBinding, ReplayEffect};

/// 标签组装单点：绑定 `ENTITY`（派生自域命令类型契约）+ 命令类型派生键。
fn labeled<B: ReplayBinding>(command: &B::Command) -> (&'static str, Option<Cow<'_, str>>) {
    (B::ENTITY, B::subject(command))
}

impl DomainCommand {
    /// 命令的裁决域 `(实体标签, 可空实体键)`：标签恒有（op 落库与挂起行 entity 列
    /// 取值），实体键可空（`None` = 无实体指向，冲突域在 OccurrenceKey、不参与同实体
    /// LWW，ADR-0091 决策 4/5）。标签单源取绑定 `ENTITY`，键由命令类型派生
    /// （ADR-0101 决策 2 / 勘误 3）。
    pub(crate) fn subject(&self) -> (&'static str, Option<Cow<'_, str>>) {
        match self {
            DomainCommand::Transaction(cmd) => labeled::<TransactionBinding>(cmd),
            DomainCommand::Scheduled(cmd) => labeled::<ScheduledBinding>(cmd),
            DomainCommand::LedgerSetting(cmd) => labeled::<LedgerSettingBinding>(cmd),
            DomainCommand::Account(cmd) => labeled::<AccountBinding>(cmd),
            DomainCommand::Category(cmd) => labeled::<CategoryBinding>(cmd),
            DomainCommand::Merchant(cmd) => labeled::<MerchantBinding>(cmd),
            DomainCommand::Budget(cmd) => labeled::<BudgetBinding>(cmd),
            DomainCommand::Policy(cmd) => labeled::<PolicyBinding>(cmd),
            DomainCommand::Insurer(cmd) => labeled::<InsurerBinding>(cmd),
            DomainCommand::Item(cmd) => labeled::<ItemBinding>(cmd),
            DomainCommand::PhysicalAsset(cmd) => labeled::<PhysicalAssetBinding>(cmd),
            DomainCommand::Instrument(cmd) => labeled::<InstrumentBinding>(cmd),
            DomainCommand::ExchangeRate(cmd) => labeled::<ExchangeRateBinding>(cmd),
            DomainCommand::Price(cmd) => labeled::<PriceBinding>(cmd),
        }
    }
}

/// 核心交易域适配绑定。
pub(crate) struct TransactionBinding;

impl ReplayBinding for TransactionBinding {
    const ENTITY: &'static str = <crate::transaction::TransactionCommand as SyncCommand>::ENTITY;
    type Command = crate::transaction::TransactionCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::transaction::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 定时计划域适配绑定。
pub(crate) struct ScheduledBinding;

impl ReplayBinding for ScheduledBinding {
    const ENTITY: &'static str =
        <crate::scheduled_transactions::ScheduledCommand as SyncCommand>::ENTITY;
    type Command = crate::scheduled_transactions::ScheduledCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        // 期次触发的幂等命中是 OccurrenceKey 独有语义：域侧 `ReplayEffect` 原样
        // 透传（不映射为 Applied，ADR-0101 决策 3）。
        crate::scheduled_transactions::replay_command(conn, command)
    }
}

/// 账本级设置适配绑定。
pub(crate) struct LedgerSettingBinding;

impl ReplayBinding for LedgerSettingBinding {
    const ENTITY: &'static str = <crate::currencies::LedgerSettingCommand as SyncCommand>::ENTITY;
    type Command = crate::currencies::LedgerSettingCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::currencies::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 账户字典适配绑定。
pub(crate) struct AccountBinding;

impl ReplayBinding for AccountBinding {
    const ENTITY: &'static str = <crate::accounts::AccountCommand as SyncCommand>::ENTITY;
    type Command = crate::accounts::AccountCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::accounts::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 分类字典适配绑定。
pub(crate) struct CategoryBinding;

impl ReplayBinding for CategoryBinding {
    const ENTITY: &'static str = <crate::categories::CategoryCommand as SyncCommand>::ENTITY;
    type Command = crate::categories::CategoryCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::categories::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 商户字典适配绑定。
pub(crate) struct MerchantBinding;

impl ReplayBinding for MerchantBinding {
    const ENTITY: &'static str = <crate::merchants::MerchantCommand as SyncCommand>::ENTITY;
    type Command = crate::merchants::MerchantCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::merchants::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 预算域适配绑定。
pub(crate) struct BudgetBinding;

impl ReplayBinding for BudgetBinding {
    const ENTITY: &'static str = <crate::budget::BudgetCommand as SyncCommand>::ENTITY;
    type Command = crate::budget::BudgetCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::budget::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 保单域适配绑定。
pub(crate) struct PolicyBinding;

impl ReplayBinding for PolicyBinding {
    const ENTITY: &'static str = <crate::policy::PolicyCommand as SyncCommand>::ENTITY;
    type Command = crate::policy::PolicyCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::policy::replay_policy_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 保司字典适配绑定。
pub(crate) struct InsurerBinding;

impl ReplayBinding for InsurerBinding {
    const ENTITY: &'static str = <crate::policy::InsurerCommand as SyncCommand>::ENTITY;
    type Command = crate::policy::InsurerCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::policy::replay_insurer_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 物品域适配绑定。
pub(crate) struct ItemBinding;

impl ReplayBinding for ItemBinding {
    const ENTITY: &'static str = <crate::item::ItemCommand as SyncCommand>::ENTITY;
    type Command = crate::item::ItemCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::item::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 实物资产域适配绑定。
pub(crate) struct PhysicalAssetBinding;

impl ReplayBinding for PhysicalAssetBinding {
    const ENTITY: &'static str =
        <crate::physical_asset::PhysicalAssetCommand as SyncCommand>::ENTITY;
    type Command = crate::physical_asset::PhysicalAssetCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::physical_asset::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 标的字典适配绑定。
pub(crate) struct InstrumentBinding;

impl ReplayBinding for InstrumentBinding {
    const ENTITY: &'static str = <crate::investment::InstrumentCommand as SyncCommand>::ENTITY;
    type Command = crate::investment::InstrumentCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::investment::replay_instrument_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 汇率适配绑定。
pub(crate) struct ExchangeRateBinding;

impl ReplayBinding for ExchangeRateBinding {
    const ENTITY: &'static str = <crate::investment::ExchangeRateCommand as SyncCommand>::ENTITY;
    type Command = crate::investment::ExchangeRateCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::investment::replay_exchange_rate_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

/// 用户侧价格适配绑定（现价录入 / 手动报价；东财行情不进 op）。
pub(crate) struct PriceBinding;

impl ReplayBinding for PriceBinding {
    const ENTITY: &'static str = <crate::investment::PriceCommand as SyncCommand>::ENTITY;
    type Command = crate::investment::PriceCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        crate::investment::replay_price_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}
