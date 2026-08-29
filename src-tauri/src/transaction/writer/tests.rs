//! `transaction::writer` 接缝的单元测试（issue #55 / spec #52）。
//!
//! 断言模块外部行为：normalize 的校验/退款继承/本位币折算、insert_row 的全列映射
//! 与审计字段生成、update_row 的字段覆盖与幂等身份保留。全部基于内存库。

use rusqlite::Connection;
use rusqlite::params;

use super::*;

fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?1,'cash',?2,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, currency],
    )
    .unwrap();
}

fn insert_category(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO categories (id,name,kind,created_at,updated_at,version,device_id) \
         VALUES (?1,?1,'expense','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id],
    )
    .unwrap();
}

fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1',?1,?2,?3,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

/// 通用入参构造器。
fn input(kind: TransactionKind, amount_cents: i64, account_id: &str) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-01".into(),
    }
}

/// 读回一行交易的全部业务字段（与 insert_row 的列映射逐列比对）。
fn read_row(conn: &Connection, id: &str) -> NormalizedRow {
    // 命名字段而非长元组：读回列多，逐列命名可读性更好（也避免 clippy type_complexity）。
    let row: RowFields = conn
        .query_row(
            "SELECT kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,merchant_id,refund_of_transaction_id,note,date \
             FROM transactions WHERE id=?1",
            params![id],
            |r| {
                Ok(RowFields {
                    kind: r.get(0)?,
                    amount_cents: r.get(1)?,
                    currency_code: r.get(2)?,
                    amount_native_cents: r.get(3)?,
                    account_id: r.get(4)?,
                    to_account_id: r.get(5)?,
                    category_id: r.get(6)?,
                    merchant_id: r.get(7)?,
                    refund_of_transaction_id: r.get(8)?,
                    note: r.get(9)?,
                    date: r.get(10)?,
                })
            },
        )
        .unwrap();
    NormalizedRow {
        kind: row.kind,
        amount_cents: row.amount_cents,
        currency_code: row.currency_code,
        amount_native_cents: row.amount_native_cents,
        account_id: row.account_id,
        to_account_id: row.to_account_id,
        category_id: row.category_id,
        merchant_id: row.merchant_id,
        refund_of_transaction_id: row.refund_of_transaction_id,
        note: row.note,
        date: row.date,
    }
}

/// `read_row` 的中间读回结构（命名字段避免长元组）。
struct RowFields {
    kind: TransactionKind,
    amount_cents: i64,
    currency_code: String,
    amount_native_cents: i64,
    account_id: String,
    to_account_id: Option<String>,
    category_id: Option<String>,
    merchant_id: Option<String>,
    refund_of_transaction_id: Option<String>,
    note: Option<String>,
    date: String,
}

/// 通过 writer 自身落一笔 expense（normalize + insert_row），作为退款来源。
fn insert_source_expense(conn: &Connection, account_id: &str, category_id: Option<&str>) -> String {
    let norm = normalize(
        conn,
        &Input {
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            category_id: category_id.map(String::from),
            ..input(TransactionKind::Expense, 1000, account_id)
        },
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}

// ---------------------------------------------------------------------------
// normalize：通用 kind 直通
// ---------------------------------------------------------------------------

/// income 直通：字段原样保留，本位币与原始币种 1:1（CNY）。
#[test]
fn normalize_income_passthrough() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(
        &conn,
        &Input {
            note: Some("工资".into()),
            ..input(TransactionKind::Income, 5000, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.kind, TransactionKind::Income);
    assert_eq!(norm.amount_cents, 5000);
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.amount_native_cents, 5000, "本位币与原始币种应 1:1");
    assert_eq!(norm.account_id, "acc");
    assert_eq!(norm.to_account_id, None);
    assert_eq!(norm.refund_of_transaction_id, None);
    assert_eq!(norm.note.as_deref(), Some("工资"));
    assert_eq!(norm.date, "2026-01-01");
}

/// expense 可选字段（分类/备注）透传。
#[test]
fn normalize_expense_passes_optional_fields() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_category(&conn, "cat-food");
    let norm = normalize(
        &conn,
        &Input {
            category_id: Some("cat-food".into()),
            note: Some("午餐".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.category_id.as_deref(), Some("cat-food"));
    assert_eq!(norm.note.as_deref(), Some("午餐"));
}

// ---------------------------------------------------------------------------
// normalize：金额 > 0 校验
// ---------------------------------------------------------------------------

/// 金额为 0 或负数均应报错。
#[test]
fn normalize_rejects_non_positive_amount() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    for bad in [0, -1, -500] {
        let err = normalize(&conn, &input(TransactionKind::Expense, bad, "acc")).unwrap_err();
        assert_eq!(err.to_string(), "参数错误: 金额必须大于 0", "金额 {bad}");
    }
}

// ---------------------------------------------------------------------------
// normalize：transfer 必填目标账户
// ---------------------------------------------------------------------------

/// transfer 缺 `to_account_id` 报错（文案与命令层既有断言一致）。
#[test]
fn normalize_transfer_requires_to_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    let err = normalize(&conn, &input(TransactionKind::Transfer, 3000, "acc-a")).unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 转账必须指定目标账户");
}

/// transfer 带 `to_account_id` 时归一化成功，目标账户透传。
#[test]
fn normalize_transfer_passes_to_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
    let norm = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-b".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-a")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id, "acc-a");
    assert_eq!(norm.to_account_id.as_deref(), Some("acc-b"));
    assert_eq!(norm.refund_of_transaction_id, None);
}

// ---------------------------------------------------------------------------
// normalize：仅接受通用 kind
// ---------------------------------------------------------------------------

/// buy/sell/dividend/split 不属于 writer::normalize 职责，应报错防误用。
#[test]
fn normalize_rejects_non_generic_kinds() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    for kind in [
        TransactionKind::Buy,
        TransactionKind::Sell,
        TransactionKind::Dividend,
        TransactionKind::Split,
    ] {
        let err = normalize(&conn, &input(kind, 1000, "acc")).unwrap_err();
        assert!(
            err.to_string().contains("仅处理通用交易类型"),
            "kind={kind:?} 应被拒绝，实际: {err}"
        );
    }
}

// ---------------------------------------------------------------------------
// normalize：退款继承原支出
// ---------------------------------------------------------------------------

/// refund 未关联原支出交易 → 报错。
#[test]
fn normalize_refund_requires_source_id() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(&conn, &input(TransactionKind::Refund, 200, "acc")).unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 退款必须关联原支出交易");
}

/// 退款继承原支出的账户/币种/分类，忽略调用方填写的 account_id/currency_code/category_id。
#[test]
fn normalize_refund_inherits_source_fields() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    insert_account(&conn, "acc-other", "USD");
    insert_category(&conn, "cat-src");
    let source_id = insert_source_expense(&conn, "acc-src", Some("cat-src"));

    let norm = normalize(
        &conn,
        &Input {
            kind: TransactionKind::Refund,
            amount_cents: 200,
            currency_code: "USD".into(),
            account_id: "acc-other".into(),
            category_id: Some("cat-other".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-other")
        },
    )
    .unwrap();
    // 继承原支出：账户/币种/分类均为来源值，而非调用方填的字段
    assert_eq!(norm.account_id, "acc-src");
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.category_id.as_deref(), Some("cat-src"));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
    // 金额与日期仍是调用方值
    assert_eq!(norm.amount_cents, 200);
    assert_eq!(norm.date, "2026-01-01");
}

/// 关联的交易不是支出（income）→ 报错。
#[test]
fn normalize_refund_rejects_non_expense_source() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let income_norm = normalize(&conn, &input(TransactionKind::Income, 1000, "acc")).unwrap();
    let income_id = insert_row(&conn, &income_norm).unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(income_id),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 退款只能关联支出交易");
}

/// 关联的原支出不存在 → NotFound。
#[test]
fn normalize_refund_source_not_found() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some("no-such-id".into()),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::NotFound(_)),
        "应返回 NotFound，实际: {err:?}"
    );
}

/// 关联的原支出已软删除 → 视为不存在（NotFound）。
#[test]
fn normalize_refund_source_soft_deleted_is_not_found() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let source_id = insert_source_expense(&conn, "acc", None);
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![source_id],
    )
    .unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

// ---------------------------------------------------------------------------
// normalize：商户（merchant_id）
// ---------------------------------------------------------------------------

fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name],
    )
    .unwrap();
}

/// income/expense 携带存在的商户 → 归一化行透传 merchant_id。
#[test]
fn normalize_merchant_passthrough() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
}

/// 携带不存在的商户 → 明确错误（商户不存在）。
#[test]
fn normalize_merchant_not_found_is_rejected() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("no-such-merchant".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "参数错误: 商户不存在或已删除: no-such-merchant"
    );
}

/// 携带已软删除的商户 → 明确错误（软删商户不可再被新交易选择）。
#[test]
fn normalize_soft_deleted_merchant_is_rejected_for_new_txn() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Income, 1000, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}

/// 退款继承原支出的商户（与账户/币种/分类同款继承语义）：忽略调用方填的 merchant_id，
/// 取原支出商户。
#[test]
fn normalize_refund_inherits_source_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");
    insert_merchant(&conn, "mer-pdd", "拼多多");
    // 落一笔带商户的原支出
    let source_norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1000, "acc-src")
        },
    )
    .unwrap();
    let source_id = insert_row(&conn, &source_norm).unwrap();

    // 退款调用方填了另一个商户 → 仍继承原支出的京东
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-pdd".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
}

/// 原支出无商户 → 退款商户为空。
#[test]
fn normalize_refund_without_source_merchant_has_none() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    let source_id = insert_source_expense(&conn, "acc-src", None);
    let norm = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id, None);
}

/// 修改路径保持历史引用：提交商户与该行当前商户相同（`existing_merchant_id`）时
/// 跳过在用校验——软删商户的历史交易仍可修改其他字段（与账户/分类更新语义一致）。
#[test]
fn normalize_keeps_unchanged_merchant_even_if_soft_deleted() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    // 提交值与既有值相同：跳过在用校验，归一化成功。
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-dead"));
}

/// 修改路径改选其他商户仍按新选择校验在用：目标为软删商户 → 拒绝。
#[test]
fn normalize_rejects_changing_to_soft_deleted_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-old", "旧商户");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-old".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}

// ---------------------------------------------------------------------------
// normalize：本位币折算（Amount 接缝）
// ---------------------------------------------------------------------------

/// 非默认币种按 Amount 接缝折算到全局默认币种（CNY），与账户币种无关。
#[test]
fn normalize_converts_via_amount_seam_to_default_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.amount_cents, 10000);
    assert_eq!(norm.currency_code, "USD");
    // 基准为全局默认币种（CNY），即使账户是 USD 也不按账户币种 1:1
    assert_eq!(norm.amount_native_cents, 72000);
}

/// 非默认币种且无汇率 → 报错，不静默 1:1 混币种。
#[test]
fn normalize_errors_without_rate_for_non_default_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-jpy", "JPY");
    let err = normalize(
        &conn,
        &Input {
            currency_code: "JPY".into(),
            ..input(TransactionKind::Expense, 10000, "acc-jpy")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("汇率"), "实际: {err}");
}

// ---------------------------------------------------------------------------
// insert_row：全列映射 + 审计字段
// ---------------------------------------------------------------------------

/// 落库后逐列读回比对：业务字段与归一化行一致，审计字段由模块生成
/// （version=1 / is_deleted=0 / created_at==updated_at / device_id 一致）。
#[test]
fn insert_row_writes_full_row_and_generates_audit_fields() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(
        &conn,
        &Input {
            note: Some("备注".into()),
            ..input(TransactionKind::Expense, 1234, "acc")
        },
    )
    .unwrap();

    let id = insert_row(&conn, &norm).unwrap();
    assert!(!id.is_empty());

    // 业务字段全列映射正确
    assert_eq!(read_row(&conn, &id), norm);

    // 审计字段
    let (created_at, updated_at, version, device_id, is_deleted): (String, String, i64, String, i64) =
        conn.query_row(
            "SELECT created_at,updated_at,version,device_id,is_deleted FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        created_at, updated_at,
        "新建行 created_at 与 updated_at 一致"
    );
    assert_eq!(version, 1);
    assert_eq!(device_id, crate::db::device_id());
    assert_eq!(is_deleted, 0);
}

/// 两次 insert 生成互异的 id。
#[test]
fn insert_row_generates_distinct_ids() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(&conn, &input(TransactionKind::Income, 100, "acc")).unwrap();
    let id1 = insert_row(&conn, &norm).unwrap();
    let id2 = insert_row(&conn, &norm).unwrap();
    assert_ne!(id1, id2);
}

// ---------------------------------------------------------------------------
// update_row：字段覆盖 + 幂等身份保留 + 版本递增
// ---------------------------------------------------------------------------

/// update 覆盖全部可编辑字段，保留 id / created_at，version 递增。
#[test]
fn update_row_overwrites_fields_and_bumps_version() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
    let norm = normalize(
        &conn,
        &Input {
            note: Some("旧备注".into()),
            ..input(TransactionKind::Expense, 500, "acc-a")
        },
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let updated = NormalizedRow {
        kind: TransactionKind::Transfer,
        amount_cents: 3000,
        currency_code: "CNY".into(),
        amount_native_cents: 3000,
        account_id: "acc-a".into(),
        to_account_id: Some("acc-b".into()),
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some("改后".into()),
        date: "2026-02-10".into(),
    };
    update_row(&conn, &id, &updated).unwrap();

    assert_eq!(read_row(&conn, &id), updated);
    let (created_at_after, updated_at, version): (String, String, i64) = conn
        .query_row(
            "SELECT created_at,updated_at,version FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(created_at_after, created_at, "created_at 应保留");
    assert_eq!(version, 2, "version 应递增");
    assert!(!updated_at.is_empty(), "updated_at 应刷新");
}

/// update 保留幂等身份（idempotency_key / dedup_hash，由命令层回写、本模块不触碰）。
#[test]
fn update_row_preserves_idempotent_identity() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(&conn, &input(TransactionKind::Expense, 500, "acc")).unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    // 模拟批量导入回写幂等身份（与 batch 模块落库后 UPDATE 同构）
    conn.execute(
        "UPDATE transactions SET dedup_hash=?2, idempotency_key=?3 WHERE id=?1",
        params![id, "hash-abc", "row-1"],
    )
    .unwrap();

    update_row(
        &conn,
        &id,
        &NormalizedRow {
            amount_cents: 900,
            note: Some("改后".into()),
            ..norm
        },
    )
    .unwrap();

    let (key, hash): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT idempotency_key,dedup_hash FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(key.as_deref(), Some("row-1"), "幂等键应保留");
    assert_eq!(hash.as_deref(), Some("hash-abc"), "dedup_hash 应保留");
}

// ---------------------------------------------------------------------------
// 端到端：normalize → insert_row → update_row
// ---------------------------------------------------------------------------

/// 创建再修改一笔交易：归一化 → 落库 → 读回 → 更新 → 读回，全链路一致。
#[test]
fn normalize_insert_update_roundtrip() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_category(&conn, "cat-food");

    let created = normalize(
        &conn,
        &Input {
            category_id: Some("cat-food".into()),
            note: Some("午餐".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    let id = insert_row(&conn, &created).unwrap();
    assert_eq!(read_row(&conn, &id), created);
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let modified = NormalizedRow {
        amount_cents: 1800,
        amount_native_cents: 1800,
        date: "2026-02-02".into(),
        ..created
    };
    update_row(&conn, &id, &modified).unwrap();
    assert_eq!(read_row(&conn, &id), modified);
    let (version, created_at_after): (i64, String) = conn
        .query_row(
            "SELECT version,created_at FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(created_at_after, created_at, "created_at 应保留");
}

// ---------------------------------------------------------------------------
// 置脏触发（ADR-0032：已收口连接层统一写入口）
// ---------------------------------------------------------------------------

/// Writer 落库本身对备份域零感知（ADR-0032）：insert_row / update_row 不再自带
/// 置脏；同样的落库经连接层写入口 `db.write` 执行（命令层真实形态）时，
/// 由提交点单点置脏。
#[test]
fn writer_rows_do_not_mark_dirty_entry_does() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");

    let row = normalize(&conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
    let id = insert_row(&conn, &row).unwrap();
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "Writer 落库本身不置脏（触发已上移写入口）"
    );
    update_row(&conn, &id, &row).unwrap();
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "更新同样不置脏"
    );

    // 经写入口执行同样的落库（与 IPC 命令同形态）→ 提交点置脏，且置脏是幂等
    // 标记、不做「已脏跳过」优化。
    let mut owned = setup_db();
    crate::db::init_db(&mut owned).unwrap();
    let state = crate::db::DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(owned)),
    };
    state
        .write(|conn| {
            insert_account(conn, "acc", "CNY");
            let row = normalize(conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
            let id = insert_row(conn, &row).unwrap();
            assert!(
                !crate::auto_backup::get_state(conn).unwrap().dirty,
                "提交点之前（闭包内）不置脏"
            );
            update_row(conn, &id, &row)
        })
        .unwrap();
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        crate::auto_backup::get_state(&conn).unwrap().dirty,
        "写入口提交点应置脏"
    );
}
