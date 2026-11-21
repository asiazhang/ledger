use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_accounts::AccountCommand;
use ledger_budget::BudgetCommand;
use ledger_categories::CategoryCommand;
use ledger_currencies::LedgerSettingCommand;
use ledger_infra::error::Result;
use ledger_investment::{ExchangeRateCommand, InstrumentCommand, PriceCommand};
use ledger_item::ItemCommand;
use ledger_merchants::MerchantCommand;
use ledger_physical_asset::PhysicalAssetCommand;
use ledger_policy::{InsurerCommand, PolicyCommand};
use ledger_savings_goal::SavingsGoalCommand;
use ledger_scheduled::ScheduledCommand;
use ledger_transaction::TransactionCommand;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entity", content = "payload", rename_all = "snake_case")]
pub enum DomainCommand {
    Transaction(TransactionCommand),
    Scheduled(ScheduledCommand),
    LedgerSetting(LedgerSettingCommand),
    Account(AccountCommand),
    Category(CategoryCommand),
    Merchant(MerchantCommand),
    Budget(BudgetCommand),
    Policy(PolicyCommand),
    Insurer(InsurerCommand),
    Item(ItemCommand),
    PhysicalAsset(PhysicalAssetCommand),
    Instrument(InstrumentCommand),
    ExchangeRate(ExchangeRateCommand),
    Price(PriceCommand),
    Goal(SavingsGoalCommand),
}

pub(crate) use ledger_sync_protocol::command::ReplayEffect;

pub(crate) trait ReplayBinding {
    const ENTITY: &'static str;
    type Command;
    fn subject(command: &Self::Command) -> Option<Cow<'_, str>>;
    fn replay(conn: &Connection, command: &Self::Command) -> Result<ReplayEffect>;
}
