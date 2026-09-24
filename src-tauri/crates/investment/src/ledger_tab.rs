//! 投资明细列表读命令（ADR-0135 决策 3 / issue #1778）：投资页「明细」页签的
//! 取数接口——五种投资 kind（buy/sell/convert/split/dividend）交易行的投资投影，
//! 四维过滤（账户 / 标的 / 类型子集 / 日期）+ 服务端 offset 分页（ADR-0008），
//! 排序与主列表同构（date 倒序，created_at / id 同秒 tiebreaker）。
//!
//! 不变量：
//! - 行集闭包 = 五种投资 kind 且有证券扩展行（`security_transactions` 以
//!   `transaction_id` 为主键，JOIN 一对一）——通用 kind（income/expense/transfer/
//!   refund）不入集，类型子集传了也只是空集不报错；
//! - 过滤条件与 total/items 共用同一 WHERE 子句，total 恒为满足条件的总数；
//! - 账户过滤是涉及账户语义（账户端 ∪ 出资端，ADR-0135 决策 3 / ADR-0096）；
//! - 标的过滤认 convert 两腿任一命中（与时点持仓推算口径对齐）；
//! - COUNT 与 items 是多语句读闭包，整体收进同一读事务（#1699 / #1702 纪律，
//!   域单测带读快照探针，删除接线即红）。

use rusqlite::Connection;

use super::model::{
    InvestmentTransactionListFilter, InvestmentTransactionListResult, InvestmentTransactionRow,
};
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount::TransactionKind;

/// 行集闭包（ADR-0135 决策 1）：五种投资 kind 整体构成明细行集——自枚举闭集取
/// 字符串面，不写裸字面量（漏登由编译期穷尽性守住，先例 `prepare` 的分派）。
const LEDGER_TAB_KINDS: [TransactionKind; 5] = [
    TransactionKind::Buy,
    TransactionKind::Sell,
    TransactionKind::Convert,
    TransactionKind::Split,
    TransactionKind::Dividend,
];

/// 列表投影列（列序与 [`ledger_infra::db::query::FromRow`] 一一对应）：公共字段 +
/// 证券扩展行载荷列 + 转入腿展示字段。前两列 `t.id, t.kind` 兼作读快照探针的
/// marker 锚（探针以首段投影列区分 items 语句与 COUNT 语句，改列序先跑探针测试，
/// 见域单测 `page_and_total_share_one_snapshot` 与 `test_support::snapshot_probe`）。
const ROW_COLUMNS: &str = "t.id, t.kind, t.amount_cents, t.account_id, t.funding_account_id, \
     t.date, st.instrument_id, i.symbol, i.name, i.instrument_type, \
     st.quantity, st.price_cents, st.fee_cents, \
     st.to_instrument_id, ti.symbol, st.to_quantity, st.out_amount_cents, st.in_amount_cents";

/// 投资明细列表（ADR-0135 决策 3 / issue #1778）：按四维过滤返回五种投资 kind
/// 交易行的投资投影，date 倒序 + offset 分页，items + total 同快照。
pub fn list_investment_transactions(
    conn: &Connection,
    filter: &InvestmentTransactionListFilter,
) -> Result<InvestmentTransactionListResult> {
    // 读快照一致性（#1699 / #1702 同根因的多语句读闭包）：COUNT 与 items 收进
    // 同一读事务——rollback journal 下语句之间无快照隔离，写提交落在其间即页与
    // 总数错位（嵌套感知：已在事务中则加入外层，回滚权归外层）。
    ensure_transaction(conn, || {
        // 过滤条件与 total/items 共用同一 WHERE 子句：total 恒为「满足过滤条件的
        // 未删除投资交易总数」。
        let mut where_clause = String::from("WHERE t.is_deleted=0");
        let mut params: Vec<String> = Vec::new();
        // 行集闭包：kind 限定五种投资 kind（与其余维度 AND 组合）。
        let placeholders = vec!["?"; LEDGER_TAB_KINDS.len()].join(",");
        where_clause.push_str(&format!(" AND t.kind IN ({placeholders})"));
        LEDGER_TAB_KINDS
            .iter()
            .for_each(|k| params.push(k.as_str().to_string()));
        if let Some(from) = filter.from.as_deref() {
            where_clause.push_str(" AND t.date >= ?");
            params.push(from.to_string());
        }
        if let Some(to) = filter.to.as_deref() {
            where_clause.push_str(" AND t.date <= ?");
            params.push(to.to_string());
        }
        if let Some(account_id) = filter.account_id.as_deref() {
            // 涉及账户语义（ADR-0135 决策 3）：账户端 ∪ 出资端（buy/sell，
            // ADR-0096）；dividend 的到账账户即其账户端，天然命中。
            where_clause.push_str(" AND (t.account_id = ? OR t.funding_account_id = ?)");
            params.push(account_id.to_string());
            params.push(account_id.to_string());
        }
        if let Some(instrument_id) = filter.instrument_id.as_deref() {
            // 两腿命中口径（与时点持仓推算认 convert 两腿对齐）：转出腿
            // （st.instrument_id）或 convert 转入腿（st.to_instrument_id）任一命中。
            where_clause.push_str(" AND (st.instrument_id = ? OR st.to_instrument_id = ?)");
            params.push(instrument_id.to_string());
            params.push(instrument_id.to_string());
        }
        // 类型子集多选（维度内取或、与其余维度 AND 组合）；空集合视为未携带
        // （不过滤，先例同主列表 kinds）；通用 kind 恒空集（与行集闭包 AND）。
        if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
            let placeholders = vec!["?"; kinds.len()].join(",");
            where_clause.push_str(&format!(" AND t.kind IN ({placeholders})"));
            kinds
                .iter()
                .for_each(|k| params.push(k.as_str().to_string()));
        }

        // JOIN 一对一（transaction_id 是 security_transactions 主键）：COUNT 不因
        // JOIN 膨胀；标的过滤维度引用 st，total 与 items 必须同 JOIN。
        let total: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM transactions t \
                 JOIN security_transactions st ON st.transaction_id = t.id \
                 {where_clause}"
            ),
            rusqlite::params_from_iter(params.iter()),
            |r| r.get(0),
        )?;

        // 确定性排序与主列表同构：date DESC, created_at DESC, id DESC——id 是最终
        // tiebreaker（created_at 秒级精度，同秒写入的行不加 id 翻页会漂移）。
        let mut sql = format!(
            "SELECT {ROW_COLUMNS} \
             FROM transactions t \
             JOIN security_transactions st ON st.transaction_id = t.id \
             JOIN instruments i ON i.id = st.instrument_id \
             LEFT JOIN instruments ti ON ti.id = st.to_instrument_id \
             {where_clause} ORDER BY t.date DESC, t.created_at DESC, t.id DESC"
        );
        // offset 分页（ADR-0008）：小于 1 按 1 处理、offset saturating 防溢出
        // （与主列表 `list_transactions_internal` 同一形态）。
        if let Some(page_size) = filter.page_size {
            let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
            let page = i64::try_from(filter.page.unwrap_or(1).max(1)).unwrap_or(i64::MAX);
            let offset = page.saturating_sub(1).saturating_mul(page_size);
            sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
        }
        let items = query_all::<InvestmentTransactionRow, _>(
            conn,
            &sql,
            rusqlite::params_from_iter(params),
        )?;
        Ok(InvestmentTransactionListResult { items, total })
    })
}
