//! 首页财务全貌命令（issue #142）：净资产跨币种合计，只读聚合。
//!
//! 组织沿用报表模块先例：命令层为薄壳（锁 `DbState` 后调 conn 级核心函数），
//! 核心函数吃 `&Connection` 可直接单测/供 e2e 复用。
//!
//! 口径（ADR-0020，真实财富视角）：
//! 净资产 = Σ 非投资账户折本位币余额 + Σ 折本位币持仓市值。
//! - 账户侧余额沿用 `list_account_balances_with_visibility`（内部即 `account_flow` 口径的
//!   `compute_all_balances_with_visibility`，与账户列表/余额页一致，排除隐藏与黑洞账户），
//!   并剔除投资账户（其价值
//!   经持仓市值计入，避免同一笔资产重复计算）；
//! - 持仓侧读 `v_holdings` 视图市值（账户本位币），再折算到全局默认币种；
//!   `market_value_cents` 为 NULL（从未录价或缺折算汇率）时按空值语义跳过；
//! - 币种折算一律复用 [`amount::convert_to_native`]，缺汇率错误上抛
//!   （中文错误信息），不静默混币种；
//! - 投资域不新增任何写函数（ADR-0013）。

use rusqlite::Connection;
use tauri::State;

use crate::db::DbState;
use crate::db::query::{FromRow, query_all};
use crate::error::{AppError, Result};
use crate::models::{AccountType, DashboardOverview};
use crate::transaction::amount;

/// 持仓市值行：`v_holdings` 市值（账户本位币，可为 NULL）+ 账户币种。
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

/// conn 级聚合：计算本位币净资产总览（只读）。
pub fn query_dashboard_overview(conn: &Connection) -> Result<DashboardOverview> {
    // 非投资账户余额合计：余额口径与账户列表一致（account_flow，排除隐藏/黑洞）。
    let mut accounts_sum = 0i64;
    for ab in crate::accounts::list_account_balances_with_visibility(conn, false)? {
        if ab.account.kind == AccountType::Investment {
            continue;
        }
        accounts_sum +=
            amount::convert_to_native(conn, ab.balance_cents, &ab.account.currency_code)?;
    }

    // 持仓市值合计：v_holdings 市值（账户本位币）→ 全局默认币种；NULL 市值跳过。
    let holdings: Vec<HoldingValue> = query_all(
        conn,
        "SELECT h.market_value_cents, a.currency_code \
         FROM v_holdings h JOIN accounts a ON a.id = h.account_id",
        [],
    )?;
    let mut holdings_sum = 0i64;
    for h in holdings {
        if let Some(market_value_cents) = h.market_value_cents {
            holdings_sum += amount::convert_to_native(conn, market_value_cents, &h.currency_code)?;
        }
    }

    Ok(DashboardOverview {
        native_currency: amount::default_currency_code().to_string(),
        net_worth_cents: accounts_sum + holdings_sum,
        accounts_balance_cents: accounts_sum,
        holdings_market_value_cents: holdings_sum,
    })
}

/// 首页净资产总览：本位币净资产及其两个组成。
#[tauri::command]
pub fn dashboard_overview(db: State<'_, DbState>) -> Result<DashboardOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_dashboard_overview(&conn)
}
