//! 交易类型→金额度量矩阵（共享语义区，ADR-0113 决策 8）：行级展示与 SQL 聚合同源。
//!
//! 职责：[`TransferSide`] 归因端点 + [`Measure`] 六度量；[`signed_amount`]（行级/
//! 展示）与五个 SQL 片段 builder 由同一 [`coefficient`] 矩阵驱动。不变量：两条路径
//! 口径恒一致（矩阵是唯一真源，改矩阵同时影响展示与聚合）。ADR 指针：ADR-0096 /
//! ADR-0113 决策 8。陷阱：矩阵数值须以测试锁定。

use std::fmt::Write as _;

use super::kind::TransactionKind;

// ---------------------------------------------------------------------------
// 具名度量
// ---------------------------------------------------------------------------

/// 账户现金流量的归因端点：转出侧（`account_id`）、转入侧（`to_account_id`）
/// 或出资侧（`funding_account_id`，ADR-0096）。`account_flow` 度量对 transfer 的
/// 符号由转出/转入侧决定；出资侧只承载 buy/sell 的现金腿。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferSide {
    /// 转出账户侧：现金流出（−）。
    Out,
    /// 转入账户侧：现金流入（+）。
    In,
    /// 出资账户侧（ADR-0096）：buy/sell 的结算现金端点——结算账户 =
    /// 出资账户 ?? 投资账户；buy 记 −、sell 记 +，kind 矩阵符号不变、
    /// 归因端点可变。非 buy/sell 行不经出资端（准入收口，防御性记 0）。
    Funding,
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
    /// 保单已缴保费（挂单 `expense` 之和）：保单视角统计口径（issue #363 /
    /// ADR-0051 决策 6）。挂单准入（行为层，issue #361）保证只有 expense/income
    /// 可携带 policy_id，故本度量恒不含退款冲减——保单现金流入记 income 挂单
    /// 而非 refund（ADR-0051 决策 4），保费口径不被冲小。
    PolicyPremium,
    /// 保单现金流入（挂单 `income` 之和）：理赔/退保/满期返还统一记 income 挂单
    /// （ADR-0051 决策 4），三者记账语义无差别。
    PolicyInflow,
}

/// kind→度量系数矩阵（单一真源）：-1 / 0 / +1。
///
/// Rust 助手 [`signed_amount`] 与 SQL 片段 builder 均由此驱动；
/// 修改任何口径只改这里，两侧行为同步变化。
fn coefficient(kind: TransactionKind, measure: Measure) -> i64 {
    match measure {
        // 账户现金流动：buy/sell 的符号与端点无关（矩阵符号不变，归因端点可变，
        // ADR-0096）——转出侧带出资守卫后只对未出资行生效，出资侧只对 buy/sell
        // 生效（见 [`account_flow_expr`] 的归因规则）。非 buy/sell 行不经出资端
        // （准入收口在行为层，防御性记 0）。
        Measure::AccountFlow(side) => match kind {
            TransactionKind::Income | TransactionKind::Refund | TransactionKind::Dividend => {
                match side {
                    TransferSide::Funding => 0,
                    _ => 1,
                }
            }
            TransactionKind::Sell => 1,
            TransactionKind::Expense => match side {
                TransferSide::Funding => 0,
                _ => -1,
            },
            TransactionKind::Buy => -1,
            TransactionKind::Transfer => match side {
                TransferSide::Out => -1,
                TransferSide::In => 1,
                TransferSide::Funding => 0,
            },
            // convert 无现金腿（ADR-0099）：两侧标的互换不涉任何账户现金，
            // 三个侧别恒记 0——落账前后全部账户余额（含黑洞）不变。
            TransactionKind::Split | TransactionKind::Convert => 0,
        },
        Measure::ExpenseNet => match kind {
            TransactionKind::Expense => 1,
            TransactionKind::Refund => -1,
            TransactionKind::Income
            | TransactionKind::Transfer
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::IncomeNet => match kind {
            TransactionKind::Income | TransactionKind::Dividend => 1,
            TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::RefundGross => match kind {
            TransactionKind::Refund => 1,
            TransactionKind::Income
            | TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::PolicyPremium => match kind {
            TransactionKind::Expense => 1,
            TransactionKind::Income
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::PolicyInflow => match kind {
            TransactionKind::Income => 1,
            TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
    }
}

/// 行级/展示用有符号金额：`coefficient(kind, measure) × amount_native_cents`。
///
/// 输入应为本位币金额（`amount_native_cents`）；
/// `split` 对现金度量恒为 0，buy/sell 不进 expense_net/income_net。
pub fn signed_amount(kind: TransactionKind, amount_native_cents: i64, measure: Measure) -> i64 {
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
    for kind in TransactionKind::ALL {
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
///
/// 出资账户归因规则单条在此落库为 SQL（ADR-0096 决策 2，全库唯一直 incarnation）：
/// 结算账户 = 出资账户 ?? 投资账户（`account_id`）——带出资账户的 buy/sell，其现金
/// 腿归出资侧（[`TransferSide::Funding`] 端点，join `funding_account_id`），转出侧
/// （投资账户端）对带出资账户的 buy/sell 记 0（钱不经手）；未填出资账户时矩阵符号
/// 原样记在投资账户上。转入侧不需要守卫：buy/sell 行恒无 `to_account_id`。
pub fn account_flow_expr(alias: &str, side: TransferSide) -> String {
    let matrix = kind_case_expr(alias, Measure::AccountFlow(side));
    match side {
        TransferSide::Out => format!(
            "CASE WHEN {alias}.kind IN ('buy','sell') \
             AND {alias}.funding_account_id IS NOT NULL THEN 0 ELSE ({matrix}) END"
        ),
        TransferSide::In | TransferSide::Funding => matrix,
    }
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

/// `policy_premium` 聚合片段（保单已缴保费，issue #363）：挂单 `expense` 之和。
pub fn policy_premium_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyPremium)
}

/// `policy_inflow` 聚合片段（保单现金流入，issue #363）：挂单 `income` 之和。
pub fn policy_inflow_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyInflow)
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
    TransactionKind::ALL
        .into_iter()
        .filter(|k| coefficient(*k, measure) != 0)
        .map(|k| k.as_str())
        .collect()
}

/// 带引号的贡献 kind 清单（如 `'expense','refund'`），可直接内插进
/// `WHERE kind IN (...)`。供聚合 SQL 行过滤用，与 [`contributing_kinds`]
/// 同源，消费方不再各自手拼 SQL 字面量（budget / reports 共用）。
pub fn contributing_kinds_sql(measure: Measure) -> String {
    quote_list(&contributing_kinds(measure))
}
