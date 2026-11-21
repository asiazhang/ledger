use std::fmt::Write as _;

use super::kind::TransactionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferSide {
    Out,
    In,
    Funding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    AccountFlow(TransferSide),
    ExpenseNet,
    IncomeNet,
    RefundGross,
    PolicyPremium,
    PolicyInflow,
}

fn coefficient(kind: TransactionKind, measure: Measure) -> i64 {
    match measure {
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

pub fn signed_amount(kind: TransactionKind, amount: i64, measure: Measure) -> i64 {
    coefficient(kind, measure) * amount
}

fn measure_amount_column(measure: Measure) -> &'static str {
    match measure {
        Measure::AccountFlow(_) => "amount_cents",
        _ => "amount_native_cents",
    }
}

fn kind_case_expr(alias: &str, measure: Measure) -> String {
    let amount_col = format!("{alias}.{}", measure_amount_column(measure));
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

pub fn funding_item_flow_expr(funding_alias: &str, tx_alias: &str) -> String {
    let mut pos: Vec<&'static str> = Vec::new();
    let mut neg: Vec<&'static str> = Vec::new();
    for kind in TransactionKind::ALL {
        match coefficient(kind, Measure::AccountFlow(TransferSide::Out)) {
            1 => pos.push(kind.as_str()),
            -1 => neg.push(kind.as_str()),
            _ => {}
        }
    }
    let amount_col = format!("{funding_alias}.amount_cents");
    let kind_col = format!("{tx_alias}.kind");
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

pub fn expense_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::ExpenseNet)
}

pub fn income_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::IncomeNet)
}

pub fn refund_gross_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::RefundGross)
}

pub fn policy_premium_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyPremium)
}

pub fn policy_inflow_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyInflow)
}

pub fn expense_gross_expr(alias: &str) -> String {
    format!(
        "({} + {})",
        expense_net_expr(alias),
        refund_gross_expr(alias)
    )
}

pub fn contributing_kinds(measure: Measure) -> Vec<&'static str> {
    TransactionKind::ALL
        .into_iter()
        .filter(|k| coefficient(*k, measure) != 0)
        .map(|k| k.as_str())
        .collect()
}

pub fn contributing_kinds_sql(measure: Measure) -> String {
    quote_list(&contributing_kinds(measure))
}
