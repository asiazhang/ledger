//! Amount 接缝（issue #54 / spec #52）：交易金额口径的单一权威。
//!
//! 三块职责：
//! - [`Kind`] 枚举：8 种交易类型的模块内真源（字符串 ↔ 枚举互转）。
//! - kind→度量矩阵：[`signed_amount`]（行级/展示）与四个 SQL 片段 builder
//!   （服务端聚合）由同一 [`coefficient`] 矩阵驱动，二者口径恒一致。
//! - [`convert_to_native`]：raw → 本位币折算，基准为全局默认币种
//!   （[`default_currency_code`]），不依赖 per-account 币种，避免跨账户漂移。
//!
//! 本模块为行为保持的 expand 步骤：尚无消费方接线，语义由测试锁定。

use std::fmt;
use std::fmt::Write as _;

use rusqlite::Connection;

use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// Kind 枚举
// ---------------------------------------------------------------------------

/// 交易类型真源。与 `transactions.kind` 的 CHECK 约束（V001）一一对应：
///
/// | kind | 含义 |
/// |------|------|
/// | [`Kind::Income`] | 收入 |
/// | [`Kind::Expense`] | 支出 |
/// | [`Kind::Transfer`] | 转账（`account_id` 转出、`to_account_id` 转入） |
/// | [`Kind::Refund`] | 退款（关联原支出交易） |
/// | [`Kind::Buy`] | 买入证券（减少现金，扩展表记持仓） |
/// | [`Kind::Sell`] | 卖出证券（增加现金） |
/// | [`Kind::Dividend`] | 现金分红 |
/// | [`Kind::Split`] | 拆股/送股（现金影响恒为 0） |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Income,
    Expense,
    Transfer,
    Refund,
    Buy,
    Sell,
    Dividend,
    Split,
}

impl Kind {
    /// 全部 kind，矩阵断言与 SQL 片段生成按此遍历。
    pub const ALL: [Kind; 8] = [
        Kind::Income,
        Kind::Expense,
        Kind::Transfer,
        Kind::Refund,
        Kind::Buy,
        Kind::Sell,
        Kind::Dividend,
        Kind::Split,
    ];

    /// 数据库存储的 kind 字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Income => "income",
            Kind::Expense => "expense",
            Kind::Transfer => "transfer",
            Kind::Refund => "refund",
            Kind::Buy => "buy",
            Kind::Sell => "sell",
            Kind::Dividend => "dividend",
            Kind::Split => "split",
        }
    }

    /// 从 kind 字符串解析；未知值报参数错误。
    pub fn parse(s: &str) -> Result<Kind> {
        let kind = match s {
            "income" => Kind::Income,
            "expense" => Kind::Expense,
            "transfer" => Kind::Transfer,
            "refund" => Kind::Refund,
            "buy" => Kind::Buy,
            "sell" => Kind::Sell,
            "dividend" => Kind::Dividend,
            "split" => Kind::Split,
            other => {
                return Err(AppError::Invalid(format!(
                    "未知交易类型: {other}（合法值: income/expense/transfer/refund/buy/sell/dividend/split）"
                )));
            }
        };
        Ok(kind)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// 具名度量
// ---------------------------------------------------------------------------

/// 转账中账户的角色：转出侧（`account_id`）或转入侧（`to_account_id`）。
/// `account_flow` 度量对 transfer 的符号由此决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferSide {
    /// 转出账户侧：现金流出（−）。
    Out,
    /// 转入账户侧：现金流入（+）。
    In,
}

/// 具名金额度量。每种度量对每种 kind 的符号见 [`coefficient`] 矩阵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    /// 账户现金流动：某账户视角下的现金出入（余额口径）。
    /// 转账按 [`TransferSide`] 取号，其余 kind 与侧无关。
    AccountFlow(TransferSide),
    /// 支出净额 = 毛支出 − 退款；投资类（buy/sell）不计入经营收支。
    ExpenseNet,
    /// 收入净额 = 收入 + 分红（dividend 计入收入）。
    IncomeNet,
    /// 退款毛额：独立成列可看，供毛值/净值并存展示。
    RefundGross,
}

/// kind→度量系数矩阵（单一真源）：-1 / 0 / +1。
///
/// Rust 助手 [`signed_amount`] 与 SQL 片段 builder 均由此驱动；
/// 修改任何口径只改这里，两侧行为同步变化。
fn coefficient(kind: Kind, measure: Measure) -> i64 {
    match measure {
        Measure::AccountFlow(side) => match kind {
            Kind::Income | Kind::Refund | Kind::Sell | Kind::Dividend => 1,
            Kind::Expense | Kind::Buy => -1,
            Kind::Transfer => match side {
                TransferSide::Out => -1,
                TransferSide::In => 1,
            },
            Kind::Split => 0,
        },
        Measure::ExpenseNet => match kind {
            Kind::Expense => 1,
            Kind::Refund => -1,
            _ => 0,
        },
        Measure::IncomeNet => match kind {
            Kind::Income | Kind::Dividend => 1,
            _ => 0,
        },
        Measure::RefundGross => match kind {
            Kind::Refund => 1,
            _ => 0,
        },
    }
}

/// 行级/展示用有符号金额：`coefficient(kind, measure) × amount_native_cents`。
///
/// 输入应为本位币金额（`amount_native_cents`）；
/// `split` 对现金度量恒为 0，buy/sell 不进 expense_net/income_net。
pub fn signed_amount(kind: Kind, amount_native_cents: i64, measure: Measure) -> i64 {
    coefficient(kind, measure) * amount_native_cents
}

// ---------------------------------------------------------------------------
// SQL 片段 builder（服务端聚合）
// ---------------------------------------------------------------------------

/// 由 coefficient 矩阵生成 `CASE ... END` 片段：按系数分组 kind，
/// 输出对 `alias.amount_native_cents` 的有符号表达式。
///
/// 只负责 kind→符号，不含 `is_deleted` 等过滤，过滤条件由调用方 WHERE 决定。
fn kind_case_expr(alias: &str, measure: Measure) -> String {
    let amount_col = format!("{alias}.amount_native_cents");
    let kind_col = format!("{alias}.kind");
    let mut pos: Vec<&'static str> = Vec::new();
    let mut neg: Vec<&'static str> = Vec::new();
    for kind in Kind::ALL {
        match coefficient(kind, measure) {
            1 => pos.push(kind.as_str()),
            -1 => neg.push(kind.as_str()),
            _ => {}
        }
    }
    let mut expr = String::from("CASE");
    if !pos.is_empty() {
        let _ = write!(
            expr,
            " WHEN {kind_col} IN ({list}) THEN {amount_col}",
            list = quote_list(&pos)
        );
    }
    if !neg.is_empty() {
        let _ = write!(
            expr,
            " WHEN {kind_col} IN ({list}) THEN -{amount_col}",
            list = quote_list(&neg)
        );
    }
    expr.push_str(" ELSE 0 END");
    expr
}

fn quote_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// `account_flow` 聚合片段。转账符号按 `side` 取：
/// 转出侧 join `t.account_id`、转入侧 join `t.to_account_id` 后分别求和相加。
pub fn account_flow_expr(alias: &str, side: TransferSide) -> String {
    kind_case_expr(alias, Measure::AccountFlow(side))
}

/// `expense_net` 聚合片段（毛支出 − 退款）。
pub fn expense_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::ExpenseNet)
}

/// `income_net` 聚合片段（收入 + 分红）。
pub fn income_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::IncomeNet)
}

/// `refund_gross` 聚合片段（退款毛额）。
pub fn refund_gross_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::RefundGross)
}

/// 毛支出聚合片段 = `expense_net + refund_gross`（spec #52 净值关系恒等式：
/// `expense_net = expense_gross − refund_gross`）。
///
/// 毛值不作为独立度量进入矩阵，而由两个具名度量经恒等式导出，
/// 毛值/净值口径由同一矩阵驱动、永不漂移（月度汇总毛值三列用，见 issue #57）。
pub fn expense_gross_expr(alias: &str) -> String {
    format!(
        "({} + {})",
        expense_net_expr(alias),
        refund_gross_expr(alias)
    )
}

/// 对度量有贡献（系数非 0）的 kind 字符串列表，矩阵驱动。
/// 供聚合 SQL 的 `WHERE kind IN (...)` 行过滤使用，与聚合片段出自同一矩阵，
/// 避免手写 kind 清单漂移（如 income_net 必须含 dividend）。
pub fn contributing_kinds(measure: Measure) -> Vec<&'static str> {
    Kind::ALL
        .into_iter()
        .filter(|k| coefficient(*k, measure) != 0)
        .map(|k| k.as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// 本位币折算
// ---------------------------------------------------------------------------

/// 全局默认（本位）币种。所有 `amount_native_cents` 的折算基准。
///
/// MVP 阶段为常量 `CNY`（与种子数据一致）；未来引入用户设置时，
/// 仅此函数改为读设置，模块内其余口径不变。
pub fn default_currency_code() -> &'static str {
    "CNY"
}

/// 查询货币对当前汇率（正查失败则反查取倒数）。
///
/// 私有依赖（spec #52）：与 `commands::fx::exchange_rate` 语义一致，
/// 后续 Writer 接缝落地时统一收口为单一实现。
fn lookup_exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rate <= 0.0 {
            return Err(AppError::Invalid(format!(
                "汇率 {base_code}->{quote_code} 非正: {rate}"
            )));
        }
        return Ok(rate);
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::Invalid(format!(
                "反向汇率 {quote_code}->{base_code} 非正: {rev}"
            )));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::Invalid(format!(
        "未找到 {base_code} -> {quote_code} 的汇率（正反向均无）"
    )))
}

/// 将原始币种金额折算为**全局默认币种**金额（四舍五入到分）。
///
/// - 币种与默认币种相同 → 1:1 原样返回。
/// - 基准为 [`default_currency_code`]，**与账户币种无关**：
///   各账户的交易统一折算到同一本位币，避免跨账户漂移。
/// - 正反向汇率均无 → 报错，不静默混币种。
pub fn convert_to_native(conn: &Connection, amount_cents: i64, currency_code: &str) -> Result<i64> {
    let target = default_currency_code();
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}
