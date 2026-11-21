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

mod model;

pub mod net_worth;

#[cfg(test)]
mod tests;

pub use model::DashboardOverview;
pub use net_worth::query_dashboard_overview;

use rusqlite::Connection;

use ledger_accounts::balance::list_account_balances_with_visibility;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;
use ledger_transaction::amount;

struct HoldingValue {
    market_value_cents: Option<i64>,
    currency_code: String,
}

impl FromRow for HoldingValue {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(HoldingValue {
            market_value_cents: row.get(0)?,
            currency_code: row.get(1)?,
        })
    }
}

fn compute_dashboard_overview(conn: &Connection) -> Result<DashboardOverview> {
    let mut accounts_sum = 0i64;
    for ab in list_account_balances_with_visibility(conn, false)? {
        accounts_sum +=
            amount::convert_to_native_current(conn, ab.balance_cents, &ab.account.currency_code)?;
    }

    let holdings: Vec<HoldingValue> = query_all(
        conn,
        "SELECT h.market_value_cents, a.currency_code \
         FROM v_holdings h JOIN accounts a ON a.id = h.account_id",
        [],
    )?;
    let mut holdings_sum = 0i64;
    for h in holdings {
        if let Some(market_value_cents) = h.market_value_cents {
            holdings_sum +=
                amount::convert_to_native_current(conn, market_value_cents, &h.currency_code)?;
        }
    }

    let physical_assets_value_cents =
        ledger_physical_asset::list_physical_assets(conn, None)?.holding_total_native_cents;

    Ok(DashboardOverview {
        native_currency: amount::default_currency_code(conn)?,
        net_worth_cents: accounts_sum + holdings_sum + physical_assets_value_cents,
        accounts_balance_cents: accounts_sum,
        holdings_market_value_cents: holdings_sum,
        physical_assets_value_cents,
    })
}
