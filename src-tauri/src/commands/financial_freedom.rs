//! 财务自由度口径命令（issue #343 / ADR-0048）：可投资资产 × 3% 安全提取率
//! ÷ 年度预算总额的只读聚合。
//!
//! 组织沿用仪表盘先例：命令层为薄壳（锁 `DbState` 后调 conn 级核心函数），
//! 核心函数吃 `&Connection` 可直接单测/供 e2e 复用。
//!
//! 口径（ADR-0048；术语见投资域 InvestableAssets / FinancialFreedom 与
//! 预算域 AnnualBudgetTotal 词汇表）：
//! - 分子 = Σ 持仓市值 + Σ 投资账户余额，均折算全局默认币种；排除隐藏账户
//!   （含黑洞——隐藏投资账户的现金与持仓一并不进分子）；从未录价的持仓按
//!   空值语义跳过，不以零计入；缺汇率错误上抛（中文错误信息），不静默混币种。
//!   与净资产管线的边界差异：净资产排除投资账户余额是防重复计资产，本口径
//!   计入是因其现金不被持仓市值体现（见 ADR-0048 决策 2）。
//! - 分母 = Σ 月度预算 × 12 + Σ 年度预算（全部未删除预算），无窗口、不滚动；
//!   预算金额即默认币种单币种口径，与折算后的分子同币种相除；不回退实际支出。
//! - 3% 安全提取率为常量，单点收口于 [`SAFE_WITHDRAWAL_RATE`]；调整先修订 ADR-0048。
//! - 覆盖年数 = 分子 ÷ 分母；未设预算（零分母）时 ratio 与 coverage_years 均为 0，
//!   占位引导在展示层。
//! - 实时计算不落库；不新增任何写函数（ADR-0013）。
//!
//! 与净资产聚合（dashboard）的同形片段刻意未收拢：本票边界不动既有命令
//! （spec #341 Out of Scope），提取共享持仓市值合计留待后续重构另行立项。

use rusqlite::Connection;
use tauri::State;

use crate::db::DbState;
use crate::db::query::{FromRow, query_all};
use crate::error::{AppError, Result};
use crate::models::{AccountType, FinancialFreedomOverview};
use crate::transaction::amount;

/// 安全提取率常量（单点收口，ADR-0048）：自由度 = 可投资资产 × 3% 对年度预算
/// 总额的覆盖比例。取保守的 3% 而非教科书 4%——达标线更扎实、留足安全边际。
const SAFE_WITHDRAWAL_RATE: f64 = 0.03;

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

/// conn 级聚合：计算财务自由度总览（只读）。
pub fn query_financial_freedom(conn: &Connection) -> Result<FinancialFreedomOverview> {
    // 分子·投资账户现金：余额口径与账户列表一致（account_flow，排除隐藏/黑洞），
    // 仅取投资账户——未投入的现金不被持仓市值体现，漏算会低估可投资资产。
    let mut cash_sum = 0i64;
    for ab in crate::db::balance::list_account_balances_with_visibility(conn, false)? {
        if ab.account.kind == AccountType::Investment {
            cash_sum +=
                amount::convert_to_native(conn, ab.balance_cents, &ab.account.currency_code)?;
        }
    }

    // 分子·持仓市值：v_holdings 市值（账户本位币）→ 全局默认币种；NULL 市值跳过。
    // 排除隐藏账户（v_holdings 本身不过滤可见性，与净资产管线共用的视图口径在此
    // 由本命令的分子口径收紧）。
    let holdings: Vec<HoldingValue> = query_all(
        conn,
        "SELECT h.market_value_cents, a.currency_code \
         FROM v_holdings h JOIN accounts a ON a.id = h.account_id \
         WHERE a.is_deleted=0 AND a.is_hidden=0",
        [],
    )?;
    let mut holdings_sum = 0i64;
    for h in holdings {
        if let Some(market_value_cents) = h.market_value_cents {
            holdings_sum += amount::convert_to_native(conn, market_value_cents, &h.currency_code)?;
        }
    }
    let numerator_cents = cash_sum + holdings_sum;

    // 分母：年度预算总额（全部未删除预算，无窗口不滚动；月度 × 12 为节奏年化）。
    let denominator_cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN period='monthly' THEN amount_cents * 12 \
                                  WHEN period='yearly' THEN amount_cents END), 0) \
         FROM budgets WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )?;

    // 零分母（未设预算）：返回零，不回退实际支出；占位引导在展示层。
    let (ratio, coverage_years) = if denominator_cents == 0 {
        (0.0, 0.0)
    } else {
        (
            round1(
                numerator_cents as f64 * SAFE_WITHDRAWAL_RATE / denominator_cents as f64 * 100.0,
            ),
            round1(numerator_cents as f64 / denominator_cents as f64),
        )
    };

    Ok(FinancialFreedomOverview {
        ratio,
        numerator_cents,
        denominator_cents,
        coverage_years,
        native_currency: amount::default_currency_code().to_string(),
    })
}

/// 一位小数四舍五入（ratio 与 coverage_years 共用刻度）。
fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// 财务自由度总览：可投资资产 × 3% 安全提取率对年度预算总额的覆盖比例（只读）。
#[tauri::command]
pub fn financial_freedom(db: State<'_, DbState>) -> Result<FinancialFreedomOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_financial_freedom(&conn)
}
