use std::borrow::Cow;

use rusqlite::Connection;

use ledger_infra::error::Result;

use ledger_sync_protocol::command::SyncCommand;

use super::command::{DomainCommand, ReplayBinding, ReplayEffect};

fn labeled<B: ReplayBinding>(command: &B::Command) -> (&'static str, Option<Cow<'_, str>>) {
    (B::ENTITY, B::subject(command))
}

impl DomainCommand {
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
            DomainCommand::Goal(cmd) => labeled::<GoalBinding>(cmd),
            DomainCommand::Instrument(cmd) => labeled::<InstrumentBinding>(cmd),
            DomainCommand::ExchangeRate(cmd) => labeled::<ExchangeRateBinding>(cmd),
            DomainCommand::Price(cmd) => labeled::<PriceBinding>(cmd),
        }
    }
}

pub(crate) struct TransactionBinding;

impl ReplayBinding for TransactionBinding {
    const ENTITY: &'static str = <ledger_transaction::TransactionCommand as SyncCommand>::ENTITY;
    type Command = ledger_transaction::TransactionCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_transaction::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct ScheduledBinding;

impl ReplayBinding for ScheduledBinding {
    const ENTITY: &'static str = <ledger_scheduled::ScheduledCommand as SyncCommand>::ENTITY;
    type Command = ledger_scheduled::ScheduledCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_scheduled::replay_command(conn, command)
    }
}

pub(crate) struct LedgerSettingBinding;

impl ReplayBinding for LedgerSettingBinding {
    const ENTITY: &'static str = <ledger_currencies::LedgerSettingCommand as SyncCommand>::ENTITY;
    type Command = ledger_currencies::LedgerSettingCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_currencies::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct AccountBinding;

impl ReplayBinding for AccountBinding {
    const ENTITY: &'static str = <ledger_accounts::AccountCommand as SyncCommand>::ENTITY;
    type Command = ledger_accounts::AccountCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_accounts::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct CategoryBinding;

impl ReplayBinding for CategoryBinding {
    const ENTITY: &'static str = <ledger_categories::CategoryCommand as SyncCommand>::ENTITY;
    type Command = ledger_categories::CategoryCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_categories::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct MerchantBinding;

impl ReplayBinding for MerchantBinding {
    const ENTITY: &'static str = <ledger_merchants::MerchantCommand as SyncCommand>::ENTITY;
    type Command = ledger_merchants::MerchantCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_merchants::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct BudgetBinding;

impl ReplayBinding for BudgetBinding {
    const ENTITY: &'static str = <ledger_budget::BudgetCommand as SyncCommand>::ENTITY;
    type Command = ledger_budget::BudgetCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_budget::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct PolicyBinding;

impl ReplayBinding for PolicyBinding {
    const ENTITY: &'static str = <ledger_policy::PolicyCommand as SyncCommand>::ENTITY;
    type Command = ledger_policy::PolicyCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_policy::replay_policy_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct InsurerBinding;

impl ReplayBinding for InsurerBinding {
    const ENTITY: &'static str = <ledger_policy::InsurerCommand as SyncCommand>::ENTITY;
    type Command = ledger_policy::InsurerCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_policy::replay_insurer_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct ItemBinding;

impl ReplayBinding for ItemBinding {
    const ENTITY: &'static str = <ledger_item::ItemCommand as SyncCommand>::ENTITY;
    type Command = ledger_item::ItemCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_item::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct PhysicalAssetBinding;

impl ReplayBinding for PhysicalAssetBinding {
    const ENTITY: &'static str =
        <ledger_physical_asset::PhysicalAssetCommand as SyncCommand>::ENTITY;
    type Command = ledger_physical_asset::PhysicalAssetCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_physical_asset::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct GoalBinding;

impl ReplayBinding for GoalBinding {
    const ENTITY: &'static str = <ledger_savings_goal::SavingsGoalCommand as SyncCommand>::ENTITY;
    type Command = ledger_savings_goal::SavingsGoalCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_savings_goal::replay_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct InstrumentBinding;
impl ReplayBinding for InstrumentBinding {
    const ENTITY: &'static str = <ledger_investment::InstrumentCommand as SyncCommand>::ENTITY;
    type Command = ledger_investment::InstrumentCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_investment::replay_instrument_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct ExchangeRateBinding;

impl ReplayBinding for ExchangeRateBinding {
    const ENTITY: &'static str = <ledger_investment::ExchangeRateCommand as SyncCommand>::ENTITY;
    type Command = ledger_investment::ExchangeRateCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_investment::replay_exchange_rate_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}

pub(crate) struct PriceBinding;

impl ReplayBinding for PriceBinding {
    const ENTITY: &'static str = <ledger_investment::PriceCommand as SyncCommand>::ENTITY;
    type Command = ledger_investment::PriceCommand;

    fn subject(command: &Self::Command) -> Option<Cow<'_, str>> {
        SyncCommand::subject(command)
    }

    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect> {
        ledger_investment::replay_price_command(conn, command)?;
        Ok(ReplayEffect::Applied)
    }
}
