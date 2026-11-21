use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::types::Value;

use crate::model::{ROW_COLUMNS, Transaction, TransactionSearchResult};
use ledger_infra::db::query::FromRow;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use crate::shared::search_text::split_terms;

pub use search_transactions_internal as search_transactions;

const MAX_PAGE_SIZE: usize = 200;

#[doc(hidden)]
pub struct TermLowered {
    pub lower: String,
}

pub(super) struct DictEntry {
    pub(super) name_lower: String,
}

#[doc(hidden)]
pub struct SearchDicts {
    accounts: HashMap<String, (DictEntry, bool)>,
    categories: HashMap<String, bool>,
    merchants: HashMap<String, DictEntry>,
}

fn dict_rows<T>(
    conn: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let mapped = stmt.query_map([], map)?;
    Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[doc(hidden)]
pub fn load_search_dicts(conn: &Connection) -> Result<SearchDicts> {
    let entry = |name: String| DictEntry {
        name_lower: name.to_lowercase(),
    };
    let mut accounts = HashMap::new();
    for (id, name, deleted) in dict_rows(conn, "SELECT id, name, is_deleted FROM accounts", |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        accounts.insert(id, (entry(name), deleted != 0));
    }
    let mut categories = HashMap::new();
    for (id, deleted) in dict_rows(conn, "SELECT id, is_deleted FROM categories", |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        categories.insert(id, deleted != 0);
    }
    let mut merchants = HashMap::new();
    for (id, name) in dict_rows(conn, "SELECT id, name FROM merchants", |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        merchants.insert(id, entry(name));
    }
    Ok(SearchDicts {
        accounts,
        categories,
        merchants,
    })
}

const MAX_PUSHED_PURCHASE_IDS: usize = 20_000;

#[doc(hidden)]
pub enum PurchasePush {
    Ids(Vec<String>),
    Exists,
}

fn load_purchase_hit_ids(conn: &Connection, term_lower: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT transaction_id FROM transaction_purchases \
         WHERE name LIKE ?1 ESCAPE '\\' ORDER BY transaction_id",
    )?;
    let rows = stmt.query_map([like_substring_pattern(term_lower)], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn resolve_purchase_pushes(
    conn: &Connection,
    term_lowers: &[TermLowered],
) -> Result<Vec<PurchasePush>> {
    let mut remaining = MAX_PUSHED_PURCHASE_IDS;
    term_lowers
        .iter()
        .map(|term| -> Result<PurchasePush> {
            let ids = load_purchase_hit_ids(conn, &term.lower)?;
            if ids.len() <= remaining {
                remaining -= ids.len();
                Ok(PurchasePush::Ids(ids))
            } else {
                Ok(PurchasePush::Exists)
            }
        })
        .collect()
}

fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 8);
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn like_substring_pattern(term_lower: &str) -> String {
    format!("%{}%", escape_like(term_lower))
}

fn term_matches_dict(term_lower: &str, entry: &DictEntry) -> bool {
    entry.name_lower.contains(term_lower)
}

#[doc(hidden)]
pub struct Stage1Query {
    pub sql: String,
    pub params: Vec<Value>,
}

#[doc(hidden)]
pub struct Stage1Filter {
    pub column: &'static str,
    pub op: &'static str,
    pub value: Value,
}

enum InClauseKind {
    In,
    NullableNotIn,
}

fn push_in_clause(
    col: &str,
    ids: Vec<Value>,
    params: &mut Vec<Value>,
    kind: InClauseKind,
) -> String {
    if ids.is_empty() {
        return match kind {
            InClauseKind::In => "0".into(),
            InClauseKind::NullableNotIn => "1".into(),
        };
    }
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", params.len() + i + 1))
        .collect::<Vec<_>>()
        .join(",");
    params.extend(ids);
    match kind {
        InClauseKind::In => format!("{col} IN ({placeholders})"),
        InClauseKind::NullableNotIn => {
            format!("({col} IS NULL OR {col} NOT IN ({placeholders}))")
        }
    }
}

fn term_clause(
    term: &TermLowered,
    dicts: &SearchDicts,
    purchase: &PurchasePush,
    params: &mut Vec<Value>,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    params.push(like_substring_pattern(&term.lower).into());
    let note_pattern_index = params.len();
    parts.push(format!("t.note LIKE ?{note_pattern_index} ESCAPE '\\'"));
    let account_ids: Vec<Value> = dicts
        .accounts
        .iter()
        .filter(|(_, (entry, deleted))| !*deleted && term_matches_dict(&term.lower, entry))
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    parts.push(push_in_clause(
        "t.account_id",
        account_ids,
        params,
        InClauseKind::In,
    ));
    let merchant_ids: Vec<Value> = dicts
        .merchants
        .iter()
        .filter(|(_, entry)| term_matches_dict(&term.lower, entry))
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    parts.push(push_in_clause(
        "t.merchant_id",
        merchant_ids,
        params,
        InClauseKind::In,
    ));
    match purchase {
        PurchasePush::Ids(ids) if !ids.is_empty() => {
            let ids: Vec<Value> = ids.iter().map(|id| Value::Text(id.clone())).collect();
            parts.push(push_in_clause("t.id", ids, params, InClauseKind::In));
        }
        PurchasePush::Exists => {
            params.push(like_substring_pattern(&term.lower).into());
            let pattern_index = params.len();
            parts.push(format!(
                "EXISTS (SELECT 1 FROM transaction_purchases p WHERE p.transaction_id = t.id AND p.name LIKE ?{pattern_index} ESCAPE '\\')"
            ));
        }
        PurchasePush::Ids(_) => {}
    }
    format!("({})", parts.join(" OR "))
}

#[doc(hidden)]
pub fn build_stage1_query(
    term_lowers: &[TermLowered],
    dicts: &SearchDicts,
    purchases: &[PurchasePush],
    filters: &[Stage1Filter],
) -> Stage1Query {
    assert!(
        term_lowers.len() == purchases.len(),
        "purchases 与词条按下标一一对应（release 下 zip 静默截断会少拼子句、漏命中面）"
    );
    let mut params: Vec<Value> = Vec::new();
    let mut clauses: Vec<String> = Vec::with_capacity(term_lowers.len() + 4);
    clauses.push("t.is_deleted = 0".to_string());

    let deleted_account_ids: Vec<Value> = dicts
        .accounts
        .iter()
        .filter(|(_, (_, deleted))| *deleted)
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    if !deleted_account_ids.is_empty() {
        clauses.push(push_in_clause(
            "t.account_id",
            deleted_account_ids,
            &mut params,
            InClauseKind::NullableNotIn,
        ));
    }

    let deleted_category_ids: Vec<Value> = dicts
        .categories
        .iter()
        .filter(|(_, deleted)| **deleted)
        .map(|(id, _)| Value::Text(id.clone()))
        .collect();
    clauses.push(push_in_clause(
        "t.category_id",
        deleted_category_ids,
        &mut params,
        InClauseKind::NullableNotIn,
    ));

    for (term, purchase) in term_lowers.iter().zip(purchases) {
        clauses.push(term_clause(term, dicts, purchase, &mut params));
    }

    for f in filters {
        params.push(f.value.clone());
        clauses.push(format!("{} {} ?{}", f.column, f.op, params.len()));
    }

    Stage1Query {
        sql: format!(
            "SELECT t.id FROM transactions t INDEXED BY idx_transactions_note_search \
             WHERE {} \
             ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
            clauses.join(" AND ")
        ),
        params,
    }
}

pub(super) fn stage1_sql(where_clauses: &[&str]) -> String {
    format!(
        "SELECT t.id,t.note,t.account_id,t.merchant_id,t.category_id \
         FROM transactions t \
         WHERE {} \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        where_clauses.join(" AND ")
    )
}

fn fetch_display_rows(conn: &Connection, page_ids: &[String]) -> Result<Vec<Transaction>> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = page_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let columns = ROW_COLUMNS
        .iter()
        .map(|c| format!("t.{c}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT {columns} FROM transactions t WHERE t.id IN ({placeholders})");
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Value> = page_ids.iter().map(|id| id.clone().into()).collect();
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        Transaction::from_row,
    )?;
    let mut by_id: HashMap<String, Transaction> = HashMap::with_capacity(page_ids.len());
    for row in rows {
        let txn = row?;
        by_id.insert(txn.id.clone(), txn);
    }
    Ok(page_ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub fn search_transactions_internal(
    conn: &Connection,
    query: &str,
    page: usize,
    page_size: usize,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<TransactionSearchResult> {
    let terms = split_terms(query);
    let has_filter = amount_min_cents.is_some()
        || amount_max_cents.is_some()
        || date_from.is_some()
        || date_to.is_some();
    if terms.is_empty() && !has_filter {
        return Ok(TransactionSearchResult {
            items: Vec::new(),
            total: 0,
        });
    }
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    ensure_transaction(conn, || {
        let dicts = load_search_dicts(conn)?;

        let mut filters: Vec<Stage1Filter> = Vec::new();
        if let Some(min) = amount_min_cents {
            filters.push(Stage1Filter {
                column: "t.amount_native_cents",
                op: ">=",
                value: min.into(),
            });
        }
        if let Some(max) = amount_max_cents {
            filters.push(Stage1Filter {
                column: "t.amount_native_cents",
                op: "<=",
                value: max.into(),
            });
        }
        if let Some(from) = date_from {
            filters.push(Stage1Filter {
                column: "t.date",
                op: ">=",
                value: from.to_string().into(),
            });
        }
        if let Some(to) = date_to {
            filters.push(Stage1Filter {
                column: "t.date",
                op: "<=",
                value: to.to_string().into(),
            });
        }

        let offset = page.saturating_sub(1).saturating_mul(page_size);
        let mut total: i64 = 0;
        let mut page_ids: Vec<String> = Vec::with_capacity(page_size);
        if terms.is_empty() {
            let mut where_clauses: Vec<String> = vec!["t.is_deleted = 0".to_string()];
            where_clauses.extend(filters.iter().map(|f| format!("{} {} ?", f.column, f.op)));
            let where_refs: Vec<&str> = where_clauses.iter().map(String::as_str).collect();
            let filter_params: Vec<Value> = filters.iter().map(|f| f.value.clone()).collect();
            let mut stmt = conn.prepare(&stage1_sql(&where_refs))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(filter_params.iter()), |row| {
                let id = row.get_ref(0)?.as_str()?;
                let account_id = row.get_ref(2)?.as_str().ok();
                let category_id = row.get_ref(4)?.as_str().ok();

                if let Some(account_id) = account_id {
                    let Some((_, account_deleted)) = dicts.accounts.get(account_id) else {
                        return Ok(());
                    };
                    if *account_deleted {
                        return Ok(());
                    }
                }
                if let Some(cid) = category_id
                    && dicts.categories.get(cid).copied().unwrap_or(false)
                {
                    return Ok(());
                }
                total += 1;
                if total as usize > offset && page_ids.len() < page_size {
                    page_ids.push(id.to_string());
                }
                Ok(())
            })?;
            for row in rows {
                row?;
            }
        } else {
            let term_lowers: Vec<TermLowered> = terms
                .iter()
                .map(|t| TermLowered {
                    lower: t.to_lowercase(),
                })
                .collect();
            let purchases = resolve_purchase_pushes(conn, &term_lowers)?;
            let query = build_stage1_query(&term_lowers, &dicts, &purchases, &filters);
            let mut stmt = conn.prepare(&query.sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(query.params.iter()), |row| {
                let id = row.get_ref(0)?.as_str()?;
                total += 1;
                if total as usize > offset && page_ids.len() < page_size {
                    page_ids.push(id.to_string());
                }
                Ok(())
            })?;
            for row in rows {
                row?;
            }
        }

        let mut items = fetch_display_rows(conn, &page_ids)?;
        crate::read::source::attach_sources(conn, &mut items)?;
        crate::read::source::attach_convert_fields(conn, &mut items)?;
        crate::read::funding::attach_fundings(conn, &mut items)?;
        crate::read::purchase::attach_purchases(conn, &mut items)?;

        Ok(TransactionSearchResult { items, total })
    })
}
