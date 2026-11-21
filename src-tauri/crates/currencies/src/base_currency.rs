use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_infra::settings;

pub const DEFAULT_BASE_CURRENCY: &str = "CNY";

pub fn current_base_currency(conn: &Connection) -> Result<String> {
    settings::get(
        conn,
        settings::SettingKey::LedgerBaseCurrency,
        DEFAULT_BASE_CURRENCY.to_string(),
    )
}

fn write_setting(conn: &Connection, code: &str) -> Result<()> {
    let known: Option<i64> = conn
        .query_row("SELECT 1 FROM currencies WHERE code = ?1", [code], |r| {
            r.get(0)
        })
        .optional()?;
    if known.is_none() {
        return Err(AppError::codedp(
            "currency.base-invalid",
            format!("本位币基准必须是币种字典中的币种：{code}"),
            &[code],
        ));
    }
    settings::set(conn, settings::SettingKey::LedgerBaseCurrency, &code)
}

pub fn set_base_currency(conn: &Connection, code: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_setting(conn, code)?;
        super::command::record_local(
            conn,
            super::command::LedgerSettingCommand::SetBaseCurrency {
                code: code.to_string(),
            },
        )
    })
}

pub(crate) fn replay_set_base_currency(conn: &Connection, code: &str) -> Result<()> {
    write_setting(conn, code)
}

pub fn install_base_currency_hook() {
    ledger_transaction::amount::base_currency::register_base_currency_reader(current_base_currency);
}
