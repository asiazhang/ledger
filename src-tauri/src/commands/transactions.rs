use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::commands::fx::convert_to_native;
use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
};

/// 计算导入去重哈希：`sha256("date|kind|amount_cents|currency_code|account_id|to_account_id")`。
/// `to_account_id` 缺省拼空串；刻意排除 note/category（AI 生成文本非确定性，会让哈希漂移）。
pub fn compute_dedup_hash(input: &TransactionInput) -> String {
    let to_account_id = input.to_account_id.as_deref().unwrap_or("");
    let payload = format!(
        "{}|{}|{}|{}|{}|{}",
        input.date,
        input.kind,
        input.amount_cents,
        input.currency_code,
        input.account_id,
        to_account_id
    );
    let digest = Sha256::digest(payload.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn insert_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }

    if input.kind == "buy" {
        return crate::commands::investment::create_buy_transaction(conn, input);
    }

    if input.kind == "sell" {
        return crate::commands::investment::create_sell_transaction(conn, input);
    }

    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }

    let (category_id, account_id, currency_code, refund_of_id) = if input.kind == "refund" {
        let ref_id = input
            .refund_of_transaction_id
            .ok_or_else(|| AppError::Invalid("退款必须关联原支出交易".into()))?;
        let (cat, acc, cur, okind): (Option<String>, String, String, String) = conn.query_row(
            "SELECT category_id, account_id, currency_code, kind \
             FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![ref_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if okind != "expense" {
            return Err(AppError::Invalid("退款只能关联支出交易".into()));
        }
        (cat, acc, cur, Some(ref_id))
    } else {
        (
            input.category_id,
            input.account_id,
            input.currency_code,
            None,
        )
    };

    let native = convert_to_native(conn, input.amount_cents, &currency_code, &account_id)?;
    let to_account_id = if input.kind == "transfer" {
        input.to_account_id
    } else {
        None
    };
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0)",
        rusqlite::params![
            id,
            input.kind,
            input.amount_cents,
            currency_code,
            native,
            account_id,
            to_account_id,
            category_id,
            refund_of_id,
            input.note,
            input.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

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
    if let Some(kind) = filter.kind.as_deref() {
        where_clause.push_str(" AND kind = ?");
        params.push(kind.to_string());
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
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted \
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
    let items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    Ok(TransactionListResult { items, total })
}

#[tauri::command]
pub fn list_transactions(
    db: State<'_, DbState>,
    filter: Option<TransactionListFilter>,
) -> Result<TransactionListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    list_transactions_internal(&conn, &filter)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    insert_transaction(&conn, input)
}

pub fn create_transactions_internal(
    conn: &Connection,
    inputs: Vec<TransactionInput>,
    dedup: bool,
) -> Result<Vec<CreateTransactionResult>> {
    conn.execute("BEGIN", [])?;
    let mut results = Vec::with_capacity(inputs.len());
    for input in inputs {
        let dedup_hash = compute_dedup_hash(&input);
        if dedup {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM transactions \
                     WHERE dedup_hash=?1 AND is_deleted=0 ORDER BY created_at LIMIT 1",
                    rusqlite::params![dedup_hash],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.is_some() {
                results.push(CreateTransactionResult {
                    success: true,
                    duplicate: true,
                    id: None,
                    error: None,
                });
                continue;
            }
        }
        match insert_transaction(conn, input) {
            Ok(id) => {
                if let Err(e) = conn.execute(
                    "UPDATE transactions SET dedup_hash=?1 WHERE id=?2",
                    rusqlite::params![dedup_hash, id],
                ) {
                    conn.execute("ROLLBACK", [])?;
                    return Err(e.into());
                }
                results.push(CreateTransactionResult {
                    success: true,
                    duplicate: false,
                    id: Some(id),
                    error: None,
                });
            }
            Err(AppError::Invalid(msg)) => results.push(CreateTransactionResult {
                success: false,
                duplicate: false,
                id: None,
                error: Some(msg),
            }),
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                return Err(e);
            }
        }
    }
    conn.execute("COMMIT", [])?;
    Ok(results)
}

#[tauri::command]
pub fn create_transactions(
    db: State<'_, DbState>,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    create_transactions_internal(&conn, inputs, false)
}

/// 删除交易（软删除 `is_deleted=1`）。
///
/// buy 交易同步清理关联持仓（`security_lots` / `security_transactions`）：
/// 若该买入已有部分卖出（`remaining_quantity < initial_quantity`）则拒绝删除。
/// 不存在的 id 返回 `AppError::NotFound`（HTTP 侧映射 404）。IPC 与 HTTP 端点共用本函数。
pub fn delete_transaction_internal(conn: &Connection, id: &str) -> Result<()> {
    let is_buy: bool = conn
        .query_row(
            "SELECT kind='buy' FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;

    if is_buy {
        let sold: i64 = conn.query_row(
            "SELECT COUNT(*) FROM security_lots \
             WHERE buy_transaction_id=?1 \
             AND remaining_quantity < initial_quantity",
            rusqlite::params![id],
            |r| r.get(0),
        )?;
        if sold > 0 {
            return Err(AppError::Invalid("该买入交易已有部分卖出，无法删除".into()));
        }
        conn.execute(
            "DELETE FROM security_lots WHERE buy_transaction_id=?1",
            rusqlite::params![id],
        )?;
        conn.execute(
            "DELETE FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![id],
        )?;
    }

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    delete_transaction_internal(&conn, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_db, open_in_memory};
    use rusqlite::params;

    fn setup() -> Connection {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        conn
    }

    fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![id, name, kind, currency],
        ).unwrap();
    }

    fn make_input(account_id: &str, kind: &str, amount: i64, date: &str) -> TransactionInput {
        TransactionInput {
            kind: kind.into(),
            amount_cents: amount,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: date.into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
        }
    }

    #[test]
    fn dedup_hash_is_stable_for_same_fields() {
        let a = make_input("acc-dedup", "income", 1000, "2026-07-01");
        let b = make_input("acc-dedup", "income", 1000, "2026-07-01");
        assert_eq!(compute_dedup_hash(&a), compute_dedup_hash(&b));
    }

    #[test]
    fn dedup_hash_excludes_note_and_category() {
        let base = make_input("acc-dedup", "expense", 500, "2026-07-02");
        let with_note = TransactionInput {
            note: Some("备注".into()),
            ..base.clone()
        };
        let with_category = TransactionInput {
            category_id: Some("cat-1".into()),
            ..base.clone()
        };
        let h = compute_dedup_hash(&base);
        assert_eq!(compute_dedup_hash(&with_note), h);
        assert_eq!(compute_dedup_hash(&with_category), h);
    }

    #[test]
    fn dedup_hash_changes_when_content_fields_change() {
        let base = make_input("acc-dedup", "income", 1000, "2026-07-01");
        let h = compute_dedup_hash(&base);
        assert_ne!(
            compute_dedup_hash(&make_input("acc-dedup", "income", 2000, "2026-07-01")),
            h
        );
        assert_ne!(
            compute_dedup_hash(&make_input("acc-dedup", "expense", 1000, "2026-07-01")),
            h
        );
        assert_ne!(
            compute_dedup_hash(&make_input("acc-other", "income", 1000, "2026-07-01")),
            h
        );
        assert_ne!(
            compute_dedup_hash(&make_input("acc-dedup", "income", 1000, "2026-07-02")),
            h
        );
    }

    #[test]
    fn dedup_hash_pins_empty_to_account_id_as_empty_string() {
        let no_to = make_input("acc-dedup", "transfer", 3000, "2026-07-03");
        let empty_to = TransactionInput {
            to_account_id: Some("".into()),
            ..no_to.clone()
        };
        assert_eq!(
            compute_dedup_hash(&no_to),
            compute_dedup_hash(&empty_to),
            "缺省 to_account_id 应等同空串"
        );
        let with_to = TransactionInput {
            to_account_id: Some("acc-to".into()),
            ..no_to.clone()
        };
        assert_ne!(
            compute_dedup_hash(&no_to),
            compute_dedup_hash(&with_to),
            "指定 to_account_id 应改变哈希"
        );
    }

    #[test]
    fn dedup_hash_matches_known_sha256_vector() {
        let input = make_input("acc-1", "income", 1000, "2026-07-01");
        // sha256("2026-07-01|income|1000|CNY|acc-1|")
        assert_eq!(
            compute_dedup_hash(&input),
            "d5a4ee5fa04913672a319a06c454283d74d312f13506a27fc81c72b09602a558"
        );
    }

    #[test]
    fn batch_create_marks_duplicates_and_keeps_rows() {
        let conn = setup();
        insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

        let inputs = vec![
            make_input("acc-dedup", "income", 1000, "2026-07-01"),
            make_input("acc-dedup", "expense", 500, "2026-07-02"),
        ];
        let first = create_transactions_internal(&conn, inputs.clone(), true).unwrap();
        assert_eq!(first.len(), 2);
        assert!(
            first
                .iter()
                .all(|r| r.success && !r.duplicate && r.id.is_some())
        );

        let second = create_transactions_internal(&conn, inputs, true).unwrap();
        assert_eq!(second.len(), 2);
        assert!(
            second
                .iter()
                .all(|r| r.success && r.duplicate && r.id.is_none())
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn batch_create_with_dedup_false_writes_duplicates() {
        let conn = setup();
        insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

        let inputs = vec![make_input("acc-dedup", "income", 1000, "2026-07-01")];
        create_transactions_internal(&conn, inputs.clone(), true).unwrap();
        let second = create_transactions_internal(&conn, inputs, false).unwrap();
        assert_eq!(second.len(), 1);
        assert!(second[0].success && !second[0].duplicate && second[0].id.is_some());

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn dedup_ignores_soft_deleted_transactions() {
        let conn = setup();
        insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

        let input = make_input("acc-dedup", "income", 1000, "2026-07-01");
        let first = create_transactions_internal(&conn, vec![input.clone()], true).unwrap();
        let id = first[0].id.clone().unwrap();

        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            params![id, now_iso(), device_id()],
        ).unwrap();

        let second = create_transactions_internal(&conn, vec![input], true).unwrap();
        assert!(second[0].success && !second[0].duplicate && second[0].id.is_some());

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn batch_creates_all_valid_transactions() {
        let conn = setup();
        insert_account(&conn, "acc-batch", "现金", "cash", "CNY");

        let inputs = vec![
            make_input("acc-batch", "income", 1000, "2026-01-01"),
            make_input("acc-batch", "expense", 500, "2026-01-02"),
            make_input("acc-batch", "income", 2000, "2026-01-03"),
        ];

        let results = inputs
            .into_iter()
            .map(|i| insert_transaction(&conn, i))
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(results.len(), 3);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn batch_rejects_transfer_without_to_account() {
        let conn = setup();
        insert_account(&conn, "acc-batch2", "现金", "cash", "CNY");

        let result = insert_transaction(
            &conn,
            make_input("acc-batch2", "transfer", 1000, "2026-01-01"),
        );
        match result {
            Err(AppError::Invalid(msg)) => assert!(msg.contains("目标账户")),
            _ => panic!("expected Invalid error"),
        }
    }

    #[test]
    fn batch_rejects_zero_amount() {
        let conn = setup();
        insert_account(&conn, "acc-batch2", "现金", "cash", "CNY");

        let bad = TransactionInput {
            amount_cents: 0,
            ..make_input("acc-batch2", "income", 100, "2026-01-01")
        };
        let result = insert_transaction(&conn, bad);
        match result {
            Err(AppError::Invalid(msg)) => assert!(msg.contains("大于 0")),
            _ => panic!("expected Invalid error"),
        }
    }

    #[test]
    fn create_income_and_expense_transactions() {
        let conn = setup();
        insert_account(&conn, "acc-crud", "现金", "cash", "CNY");

        let id1 = insert_transaction(&conn, make_input("acc-crud", "income", 5000, "2026-02-01"))
            .unwrap();
        let id2 = insert_transaction(
            &conn,
            TransactionInput {
                amount_cents: 1500,
                note: Some("午餐".into()),
                category_id: None,
                ..make_input("acc-crud", "expense", 100, "2026-02-02")
            },
        )
        .unwrap();
        assert_ne!(id1, id2);
        let row1: (String, String, i64, Option<String>) = conn
            .query_row(
                "SELECT kind, account_id, amount_cents, note FROM transactions WHERE id=?1",
                params![id1],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row1.0, "income");
        assert_eq!(row1.2, 5000);
    }

    #[test]
    fn create_transfer_with_to_account() {
        let conn = setup();
        insert_account(&conn, "acc-from", "A账户", "cash", "CNY");
        insert_account(&conn, "acc-to", "B账户", "cash", "CNY");

        let id = insert_transaction(
            &conn,
            TransactionInput {
                kind: "transfer".into(),
                amount_cents: 3000,
                currency_code: "CNY".into(),
                account_id: "acc-from".into(),
                to_account_id: Some("acc-to".into()),
                date: "2026-03-01".into(),
                category_id: None,
                refund_of_transaction_id: None,
                note: None,
                instrument_id: None,
                quantity: None,
                price_cents: None,
                fee_cents: None,
            },
        )
        .unwrap();
        let (kind, from, to): (String, String, Option<String>) = conn
            .query_row(
                "SELECT kind, account_id, to_account_id FROM transactions WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(kind, "transfer");
        assert_eq!(from, "acc-from");
        assert_eq!(to.as_deref(), Some("acc-to"));
    }

    #[test]
    fn list_transactions_ordered_by_date_desc() {
        let conn = setup();
        insert_account(&conn, "acc-list", "现金", "cash", "CNY");

        insert_transaction(&conn, make_input("acc-list", "income", 100, "2026-01-03")).unwrap();
        insert_transaction(&conn, make_input("acc-list", "income", 200, "2026-01-01")).unwrap();
        insert_transaction(&conn, make_input("acc-list", "income", 300, "2026-01-02")).unwrap();

        let rows: Vec<(String, i64)> = conn
            .prepare(
                "SELECT kind, amount_cents FROM transactions WHERE is_deleted=0 \
                 ORDER BY date DESC, created_at DESC",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, 100); // 01-03 first
        assert_eq!(rows[1].1, 300); // 01-02
        assert_eq!(rows[2].1, 200); // 01-01 last
    }

    #[test]
    fn list_transactions_limit() {
        let conn = setup();
        insert_account(&conn, "acc-limit", "现金", "cash", "CNY");

        insert_transaction(&conn, make_input("acc-limit", "income", 100, "2026-01-01")).unwrap();
        insert_transaction(&conn, make_input("acc-limit", "income", 200, "2026-01-02")).unwrap();
        insert_transaction(&conn, make_input("acc-limit", "income", 300, "2026-01-03")).unwrap();

        let rows: Vec<i64> = conn
            .prepare(
                "SELECT amount_cents FROM transactions WHERE is_deleted=0 \
                 ORDER BY date DESC, created_at DESC LIMIT 2",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, i64>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(rows.len(), 2);
    }

    /// 把所有交易的时间戳改为同一值，模拟"同一批导入每批一个时间戳"。
    fn set_created_at(conn: &Connection, created_at: &str) {
        conn.execute(
            "UPDATE transactions SET created_at=?1, updated_at=?1",
            params![created_at],
        )
        .unwrap();
    }

    #[test]
    fn list_transactions_pagination_returns_page_and_total() {
        let conn = setup();
        insert_account(&conn, "acc-page", "现金", "cash", "CNY");

        for i in 1..=25 {
            insert_transaction(
                &conn,
                make_input("acc-page", "expense", i * 100, &format!("2026-01-{:02}", i)),
            )
            .unwrap();
        }

        let p1 = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(1),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p1.items.len(), 10, "第 1 页应返回 10 条");
        assert_eq!(p1.total, 25, "total 应为过滤后总数");

        let p3 = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(3),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p3.items.len(), 5, "最后一页应返回剩余条数");
        assert_eq!(p3.total, 25);
    }

    #[test]
    fn list_transactions_pagination_total_respects_filters() {
        let conn = setup();
        insert_account(&conn, "acc-f1", "现金", "cash", "CNY");
        insert_account(&conn, "acc-f2", "银行", "bank", "CNY");

        for i in 1..=8 {
            insert_transaction(
                &conn,
                make_input("acc-f1", "expense", i * 100, &format!("2026-02-{:02}", i)),
            )
            .unwrap();
        }
        insert_transaction(&conn, make_input("acc-f2", "income", 9000, "2026-02-09")).unwrap();
        insert_transaction(&conn, make_input("acc-f1", "income", 1000, "2026-02-10")).unwrap();

        let by_account = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                account_id: Some("acc-f1".into()),
                page: Some(1),
                page_size: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_account.items.len(), 5);
        assert_eq!(by_account.total, 9, "total 应按过滤后计数");

        let by_kind = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                kind: Some("income".into()),
                page: Some(1),
                page_size: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_kind.items.len(), 1);
        assert_eq!(by_kind.total, 2);

        let by_date = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                from: Some("2026-02-03".into()),
                to: Some("2026-02-06".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_date.items.len(), 4);
        assert_eq!(by_date.total, 4);
    }

    #[test]
    fn list_transactions_deterministic_order_by_id_when_same_timestamp() {
        let conn = setup();
        insert_account(&conn, "acc-same", "现金", "cash", "CNY");

        let mut ids = Vec::new();
        for i in 1..=5 {
            let id = insert_transaction(
                &conn,
                make_input("acc-same", "expense", i * 100, "2026-03-01"),
            )
            .unwrap();
            ids.push(id);
        }
        // 同一批导入：所有行 created_at 相同（每批一个时间戳）
        set_created_at(&conn, "2026-01-01T00:00:00Z");

        // 期望顺序 = SQLite TEXT 列的 id DESC（字典序降序，确定性 tiebreaker）
        let mut expected = ids.clone();
        expected.sort_by(|a, b| b.cmp(a));

        let mut got = Vec::new();
        for page in 1..=3 {
            let result = list_transactions_internal(
                &conn,
                &TransactionListFilter {
                    page: Some(page),
                    page_size: Some(2),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(result.total, 5);
            for t in result.items {
                got.push(t.id);
            }
        }
        assert_eq!(
            got, expected,
            "同日期同时间戳应按 id DESC 稳定排序，翻页无重复无遗漏"
        );
    }

    #[test]
    fn list_transactions_default_returns_all_with_total() {
        let conn = setup();
        insert_account(&conn, "acc-all", "现金", "cash", "CNY");
        for i in 1..=5 {
            insert_transaction(
                &conn,
                make_input("acc-all", "expense", i * 100, &format!("2026-04-{:02}", i)),
            )
            .unwrap();
        }
        let result = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
        assert_eq!(result.items.len(), 5, "缺省应返回全部");
        assert_eq!(result.total, 5);
    }

    #[test]
    fn list_transactions_limit_path_unchanged() {
        let conn = setup();
        insert_account(&conn, "acc-lim", "现金", "cash", "CNY");
        for i in 1..=5 {
            insert_transaction(
                &conn,
                make_input("acc-lim", "expense", i * 100, &format!("2026-05-{:02}", i)),
            )
            .unwrap();
        }

        let r3 = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                limit: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r3.items.len(), 3, "limit 路径取前 N 条");
        assert_eq!(r3.total, 5);

        let r10 = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                limit: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r10.items.len(), 5, "limit 大于总数时返回全部");
        assert_eq!(r10.total, 5);

        let both = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                limit: Some(1),
                page: Some(1),
                page_size: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            both.items.len(),
            2,
            "传 page_size 时分页路径生效，limit 被忽略"
        );
    }

    #[test]
    fn list_transactions_out_of_range_page_and_empty_result() {
        let conn = setup();
        insert_account(&conn, "acc-bnd", "现金", "cash", "CNY");
        for i in 1..=3 {
            insert_transaction(
                &conn,
                make_input("acc-bnd", "expense", i * 100, &format!("2026-06-{:02}", i)),
            )
            .unwrap();
        }

        // 超范围页码：空 items，total 不变
        let far = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(99),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(far.items.len(), 0, "超范围页码应返回空列表");
        assert_eq!(far.total, 3);

        // page=0 视为第 1 页（page 从 1 起）
        let p0 = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(0),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p0.items.len(), 3);
        assert_eq!(p0.total, 3);

        // 无匹配过滤：空结果 total 0
        let none = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                kind: Some("income".into()),
                page: Some(1),
                page_size: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(none.items.len(), 0);
        assert_eq!(none.total, 0);
    }

    #[test]
    fn list_transactions_degenerate_inputs_do_not_panic() {
        let conn = setup();
        insert_account(&conn, "acc-deg", "现金", "cash", "CNY");
        for i in 1..=5 {
            insert_transaction(
                &conn,
                make_input("acc-deg", "expense", i * 100, &format!("2026-07-{:02}", i)),
            )
            .unwrap();
        }

        // page_size=0：进入分页路径且钳制为 1 条/页（与 InstrumentListFilter 先例一致）
        let zero_ps = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(1),
                page_size: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(zero_ps.items.len(), 1, "page_size=0 应按 1 条/页处理");
        assert_eq!(zero_ps.total, 5);

        // limit=0：沿用 SQLite 原生语义返回空（与旧实现一致）
        let zero_limit = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                limit: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(zero_limit.items.len(), 0, "limit=0 应返回空");
        assert_eq!(zero_limit.total, 5);

        // 极端 page 不应溢出 panic，返回空页且 total 正确
        let huge_page = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(usize::MAX),
                page_size: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(huge_page.items.len(), 0, "极端页码应返回空");
        assert_eq!(huge_page.total, 5);
    }

    #[test]
    fn delete_transaction_soft_deletes() {
        let conn = setup();
        insert_account(&conn, "acc-del", "现金", "cash", "CNY");

        let id =
            insert_transaction(&conn, make_input("acc-del", "income", 1000, "2026-01-01")).unwrap();
        let count_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_before, 1);

        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            params![id, now_iso(), device_id()],
        ).unwrap();

        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0);
    }

    #[test]
    fn delete_transaction_internal_returns_not_found_for_missing_id() {
        let conn = setup();
        insert_account(&conn, "acc-missing", "现金", "cash", "CNY");

        let err = delete_transaction_internal(&conn, "不存在的id").unwrap_err();
        match err {
            AppError::NotFound(msg) => assert!(msg.contains("交易不存在")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn delete_transaction_internal_returns_not_found_for_already_deleted() {
        let conn = setup();
        insert_account(&conn, "acc-gone", "现金", "cash", "CNY");
        let id = insert_transaction(&conn, make_input("acc-gone", "income", 1000, "2026-01-01"))
            .unwrap();
        conn.execute(
            "UPDATE transactions SET is_deleted=1 WHERE id=?1",
            params![id],
        )
        .unwrap();

        let err = delete_transaction_internal(&conn, &id).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn delete_transaction_internal_frees_dedup_slot_for_reimport() {
        let conn = setup();
        insert_account(&conn, "acc-reimport", "现金", "cash", "CNY");

        let input = make_input("acc-reimport", "income", 1000, "2026-07-01");
        let first = create_transactions_internal(&conn, vec![input.clone()], true).unwrap();
        let id = first[0].id.clone().unwrap();

        delete_transaction_internal(&conn, &id).unwrap();

        let second = create_transactions_internal(&conn, vec![input], true).unwrap();
        assert!(
            second[0].success && !second[0].duplicate && second[0].id.is_some(),
            "删除后重跑应重新写入（duplicate=false）"
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    fn make_buy_input(
        account_id: &str,
        instrument_id: &str,
        qty: f64,
        price: i64,
        fee: i64,
    ) -> TransactionInput {
        TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(qty),
            price_cents: Some(price),
            fee_cents: Some(fee),
        }
    }

    fn setup_investment_account(conn: &Connection, account_id: &str, instrument_id: &str) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,'SYM','stock','Symbol','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();
    }

    #[test]
    fn delete_transaction_internal_cleans_up_buy_lots() {
        use crate::commands::investment::create_buy_transaction;
        let conn = setup();
        setup_investment_account(&conn, "acc-inv", "inst-aapl");

        let buy_id = create_buy_transaction(
            &conn,
            make_buy_input("acc-inv", "inst-aapl", 10.0, 10000, 500),
        )
        .unwrap();

        let lots: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
                params![buy_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lots, 1, "买入应建仓一个 lot");
        let stx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
                params![buy_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stx, 1);

        delete_transaction_internal(&conn, &buy_id).unwrap();

        let lots_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
                params![buy_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lots_after, 0, "删除买入应清理 security_lots");
        let stx_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
                params![buy_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stx_after, 0, "删除买入应清理 security_transactions");
        let deleted: i64 = conn
            .query_row(
                "SELECT is_deleted FROM transactions WHERE id=?1",
                params![buy_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1, "交易应被软删除");
    }

    #[test]
    fn delete_transaction_internal_rejects_partially_sold_buy() {
        use crate::commands::investment::{create_buy_transaction, create_sell_transaction};
        let conn = setup();
        setup_investment_account(&conn, "acc-inv2", "inst-msft");

        let buy_id = create_buy_transaction(
            &conn,
            make_buy_input("acc-inv2", "inst-msft", 10.0, 10000, 0),
        )
        .unwrap();

        let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 11000, 0);
        sell.kind = "sell".into();
        sell.date = "2026-01-20".into();
        create_sell_transaction(&conn, sell).unwrap();

        let err = delete_transaction_internal(&conn, &buy_id).unwrap_err();
        match err {
            AppError::Invalid(msg) => assert!(msg.contains("部分卖出")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn create_refund_linked_to_expense() {
        let conn = setup();
        insert_account(&conn, "acc-ref", "现金", "cash", "CNY");

        let expense_id = insert_transaction(
            &conn,
            TransactionInput {
                kind: "expense".into(),
                amount_cents: 1000,
                currency_code: "CNY".into(),
                account_id: "acc-ref".into(),
                date: "2026-04-01".into(),
                category_id: None,
                to_account_id: None,
                refund_of_transaction_id: None,
                note: None,
                instrument_id: None,
                quantity: None,
                price_cents: None,
                fee_cents: None,
            },
        )
        .unwrap();

        let refund_id = insert_transaction(
            &conn,
            TransactionInput {
                kind: "refund".into(),
                amount_cents: 200,
                currency_code: "CNY".into(),
                account_id: "acc-ref".into(),
                date: "2026-04-05".into(),
                refund_of_transaction_id: Some(expense_id.clone()),
                category_id: None,
                to_account_id: None,
                note: None,
                instrument_id: None,
                quantity: None,
                price_cents: None,
                fee_cents: None,
            },
        )
        .unwrap();

        let (kind, refund_of): (String, Option<String>) = conn
            .query_row(
                "SELECT kind, refund_of_transaction_id FROM transactions WHERE id=?1",
                params![refund_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "refund");
        assert_eq!(refund_of, Some(expense_id));
    }
}
