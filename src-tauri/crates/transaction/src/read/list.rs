//! 交易列表与单笔读取（读路径，ADR-0113 决策 8）：过滤/排序/分页与单笔 SQL 单点。
//!
//! 职责：`list_transactions_internal`（过滤/排序/分页 + 来源与转换投影填充）与
//! `get_transaction_internal`（按 id 读未删除交易）。不变量：过滤条件与 total/items
//! 共用同一 WHERE 子句；排序 date DESC, created_at DESC, id DESC（id 最终 tiebreaker）。
//! ADR 指针：ADR-0113 决策 8 / ADR-0056。陷阱：来源列与转换扩展经 `read::source`
//! 读时投影填充，接缝注册点在跨域接缝区。

use rusqlite::Connection;

use crate::model::{Transaction, TransactionListFilter, TransactionListResult};
use crate::read::source::{attach_convert_fields, attach_sources};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::error::{AppError, Result};

pub use get_transaction_internal as get_transaction;
pub use list_transactions_internal as list_transactions;

pub fn list_transactions_internal(
    conn: &Connection,
    filter: &TransactionListFilter,
) -> Result<TransactionListResult> {
    // 过滤条件与 total/items 共用同一 WHERE 子句，保证 total 恒为"满足过滤条件的未删除交易总数"。
    let mut where_clause = String::from("WHERE is_deleted=0");
    let mut params: Vec<String> = Vec::new();
    if let Some(from) = filter.from.as_deref() {
        where_clause.push_str(" AND date >= ?");
        params.push(from.to_string());
    }
    if let Some(to) = filter.to.as_deref() {
        where_clause.push_str(" AND date <= ?");
        params.push(to.to_string());
    }
    if let Some(account_id) = filter.account_id.as_deref() {
        where_clause.push_str(" AND account_id = ?");
        params.push(account_id.to_string());
    }
    if let Some(account_id) = filter.involving_account_id.as_deref() {
        // 涉及账户三端（issue #937 / ADR-0096）：转出 ∪ 转入 ∪ 出资——按出资账户
        // 过滤命中它出资的 buy/sell；已发布两端语义不变（只增不改）。
        where_clause
            .push_str(" AND (account_id = ? OR to_account_id = ? OR funding_account_id = ?)");
        params.push(account_id.to_string());
        params.push(account_id.to_string());
        params.push(account_id.to_string());
    }
    if let Some(merchant_id) = filter.merchant_id.as_deref() {
        where_clause.push_str(" AND merchant_id = ?");
        params.push(merchant_id.to_string());
    }
    // 分类下钻两字段（issue #377）：精确匹配不含子分类；仅无分类命中 category_id IS NULL。
    // 与其他维度同规：AND 组合，互不隐含。
    if let Some(category_id) = filter.category_id.as_deref() {
        where_clause.push_str(" AND category_id = ?");
        params.push(category_id.to_string());
    }
    if filter.uncategorized_only == Some(true) {
        where_clause.push_str(" AND category_id IS NULL");
    }
    // 标的过滤（ADR-0107）：核心交易行不持标的信息，经投资域扩展表子查询命中——
    // 任一腿命中（instrument_id = 转出腿，to_instrument_id = convert 转入腿），
    // 与其它维度 AND 组合；total 与 items 共用同一 WHERE 子句，口径自动一致。
    if let Some(instrument_id) = filter.instrument_id.as_deref() {
        where_clause.push_str(
            " AND id IN (SELECT transaction_id FROM security_transactions \
             WHERE instrument_id = ? OR to_instrument_id = ?)",
        );
        params.push(instrument_id.to_string());
        params.push(instrument_id.to_string());
    }
    // 类型集合过滤（spec #1025 起为唯一类型维度，手动多选与下钻载荷共用）：kind IN (...)，
    // 维度内取或、与其余维度 AND 组合；单值亦经本参数（原单值 kind = ? 子句已随
    // 参数移除，BREAKING，见 CHANGELOG）。空集合视为未携带（不过滤），先例同
    // uncategorized_only=false。
    if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
        let placeholders = vec!["?"; kinds.len()].join(",");
        where_clause.push_str(&format!(" AND kind IN ({placeholders})"));
        kinds
            .iter()
            .for_each(|k| params.push(k.as_str().to_string()));
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM transactions {where_clause}"),
        rusqlite::params_from_iter(params.iter()),
        |r| r.get(0),
    )?;

    // 确定性排序：date DESC, created_at DESC, id DESC。
    // id 是最终 tiebreaker——`now_iso()` 为秒级精度，同一秒内写入的行 created_at 相同，
    // 不加 id 翻页会漂移（重复/遗漏）。
    let mut sql = format!(
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted,merchant_id,policy_id \
         FROM transactions {where_clause} ORDER BY date DESC, created_at DESC, id DESC"
    );
    // 分页路径优先：传 page_size 时按 offset 页码取当前页（小于 1 按 1 处理，
    // 与 InstrumentListFilter 先例一致；offset 用 saturating 运算防溢出）；
    // 否则 limit 路径取前 N 条（沿用 SQLite 原生语义：LIMIT 0 返回空、负值无上限）；
    // 两者都缺省时返回全部（total 恒返回）。
    if let Some(page_size) = filter.page_size {
        // 钳制到 SQLite 可接受的 64 位整数范围，防止极端输入（usize::MAX）产生
        // "datatype mismatch" 或 debug 构建 panic。
        let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
        let page = filter.page.unwrap_or(1).max(1);
        let offset = i64::try_from(page.saturating_sub(1).saturating_mul(page_size as usize))
            .unwrap_or(i64::MAX);
        sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
    } else if let Some(n) = filter.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    attach_sources(conn, &mut items)?;
    attach_convert_fields(conn, &mut items)?;
    Ok(TransactionListResult { items, total })
}

/// 按 `id` 读取未删除交易，供修改接口返回更新后的完整交易。不存在返回 `NotFound`。
pub fn get_transaction_internal(conn: &Connection, id: &str) -> Result<Transaction> {
    query_one::<Transaction, _>(
        conn,
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
         version,device_id,is_deleted,merchant_id,policy_id FROM transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
    })
}
