//! 批量写入（`TransactionBatch::run`）测试：写入与判定标记、幂等键（IdempotencyKey）
//! 语义（重跑跳过 / 内容无关 / 约束回滚 / 索引命中）与批次汇总日志（ADR-0009 决策 #5）。

use rusqlite::{Connection, params};

use crate::commands::batch::TransactionBatch;
use crate::models::TransactionInput;
use crate::test_utils::{CapturedEvent, capture_events};
use crate::transaction::amount::TransactionKind;
use tracing::Level;

use super::common::{insert_account, make_input, setup};

#[test]
fn batch_create_marks_duplicates_and_keeps_rows() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

    let inputs = vec![
        make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01"),
        make_input("acc-dedup", TransactionKind::Expense, 500, "2026-07-02"),
    ];
    let first = TransactionBatch::run(&conn, inputs.clone(), true).unwrap();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r.success && !r.duplicate && r.id.is_some())
    );

    let second = TransactionBatch::run(&conn, inputs, true).unwrap();
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

    let inputs = vec![make_input(
        "acc-dedup",
        TransactionKind::Income,
        1000,
        "2026-07-01",
    )];
    TransactionBatch::run(&conn, inputs.clone(), true).unwrap();
    let second = TransactionBatch::run(&conn, inputs, false).unwrap();
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

// ---------------------------------------------------------------------------
// 幂等键（IdempotencyKey）：重跑跳过、内容无关、一键一活交易约束与索引命中。
// ---------------------------------------------------------------------------

#[test]
fn batch_create_idempotency_key_rerun_skips_and_returns_id() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].success && !first[0].duplicate, "首次导入应新写入");
    let id1 = first[0].id.clone().unwrap();

    let second = TransactionBatch::run(&conn, vec![a], true).unwrap();
    assert_eq!(second.len(), 1);
    assert!(
        second[0].success && second[0].duplicate,
        "同幂等键重跑应去重跳过"
    );
    assert_eq!(
        second[0].id.as_deref(),
        Some(id1.as_str()),
        "同键重跑应返回该笔已有 id"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "重复导入不应新增交易");
}

#[test]
fn batch_create_idempotency_key_content_agnostic() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();

    // 同一幂等键、本轮内容不同：仍应按同一条跳过（内容无关）。
    let mut b = make_input("acc-key", TransactionKind::Expense, 2000, "2026-02-01");
    b.idempotency_key = Some("file:1:1".into());
    let second = TransactionBatch::run(&conn, vec![b], true).unwrap();
    assert!(
        second[0].success && second[0].duplicate,
        "同键不同内容仍应去重跳过"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "内容无关：即便内容变化也不新增");
}

#[test]
fn batch_create_idempotency_key_different_keys_same_content_keeps_both() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = TransactionBatch::run(&conn, vec![a, b], true).unwrap();
    assert_eq!(r.len(), 2);
    assert!(
        r.iter().all(|x| x.success && !x.duplicate),
        "不同键但内容完全相同应都保留"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "内容相同的两笔独立交易都应落库");
}

#[test]
fn batch_create_idempotency_key_same_key_dedup_false_raises_constraint() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("dup-key".into());
    let mut b = make_input("acc-key", TransactionKind::Income, 2000, "2026-01-02");
    b.idempotency_key = Some("dup-key".into());

    // dedup=false 直接落库两笔同键：部分唯一索引应拒绝（一键至多一活交易）。
    let err = TransactionBatch::run(&conn, vec![a, b], false).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("unique"),
        "同键重复应触发唯一索引约束，实际: {err:?}"
    );

    // 提交失败整批回滚（外部可观察结果）：触发约束的行与同批已写行都不落库。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "批次失败应整批回滚，不残留任何行");
}

#[test]
fn idempotency_key_dedup_query_uses_partial_index() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    TransactionBatch::run(&conn, vec![a], true).unwrap();

    // EXPLAIN QUERY PLAN 的 detail 列（第 4 列，索引 3）应命中部分唯一索引，而非全表扫描。
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT id FROM transactions \
             WHERE idempotency_key=?1 AND is_deleted=0 LIMIT 1",
        )
        .unwrap();
    let details: Vec<String> = stmt
        .query_map(rusqlite::params!["file:1:1"], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    let plan = details.join(" | ");
    assert!(
        plan.contains("idx_transactions_idempotency_key"),
        "幂等键去重查询应命中部分唯一索引: {plan}"
    );
}

fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
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
    // buy/sell 本位币折算走 Amount 接缝（issue #70）：补 1:1 汇率，非默认币种账户交易不报缺汇率。
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-fix','USD','CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();
}

#[test]
fn batch_create_idempotency_key_buy_sell_different_instruments_kept() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv-key", "inst-aapl");
    // 第二个不同标的（相同币种 USD）。
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'MSFT','stock','Msft','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params!["inst-msft"],
    )
    .unwrap();

    // 两笔买入：不同标的、相同原始金额字段（amount_cents=0，内容哈希盲区），带键应都保留。
    let mut buy1 = make_buy_input("acc-inv-key", "inst-aapl", 10.0, 10000, 500);
    buy1.idempotency_key = Some("file:1:1".into());
    let mut buy2 = make_buy_input("acc-inv-key", "inst-msft", 5.0, 20000, 300);
    buy2.idempotency_key = Some("file:1:2".into());

    let r = TransactionBatch::run(&conn, vec![buy1, buy2], true).unwrap();
    assert_eq!(r.len(), 2);
    assert!(
        r.iter().all(|x| x.success && !x.duplicate),
        "不同标的(带键)都应保留，不应被内容哈希误去重"
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

// ---------------------------------------------------------------------------
// 导入批次汇总日志（ADR-0009 决策 #5 / issue #45）
// ---------------------------------------------------------------------------

/// 从事件字段里取同名 key 的值（无则 None）。
fn field_value<'a>(event: &'a CapturedEvent, key: &str) -> Option<&'a str> {
    event
        .fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// 从捕获的事件里找出批次汇总事件（唯一带 `total`+`failed` 字段的事件）。
fn find_batch_summary(events: &[CapturedEvent]) -> Option<&CapturedEvent> {
    events.iter().find(|e| {
        e.fields.iter().any(|(k, _)| k == "total") && e.fields.iter().any(|(k, _)| k == "failed")
    })
}

/// 成功批次：汇总行以 info 级别出现，含总耗时与条数（失败数=0）。
#[test]
fn batch_create_logs_summary_on_success() {
    let conn = setup();
    insert_account(&conn, "acc-log-ok", "现金", "cash", "CNY");

    let inputs = vec![
        make_input("acc-log-ok", TransactionKind::Income, 1000, "2026-07-01"),
        make_input("acc-log-ok", TransactionKind::Expense, 500, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = TransactionBatch::run(&conn, inputs, true).unwrap();
        assert_eq!(r.len(), 2);
    });

    let summary = find_batch_summary(&events).expect("应有一条批次汇总日志");
    assert_eq!(summary.level, Level::INFO, "汇总行应为 info 级（默认可见）");
    assert_eq!(field_value(summary, "total"), Some("2"), "交易条数应为 2");
    assert_eq!(
        field_value(summary, "failed"),
        Some("0"),
        "全成功时失败数应为 0"
    );
    assert!(
        field_value(summary, "elapsed_ms").is_some(),
        "汇总行应含总耗时"
    );
}

/// 批次中途回滚：汇总行仍出现，且含失败条数（触发唯一约束回滚的那条）。
#[test]
fn batch_create_logs_summary_on_rollback() {
    let conn = setup();
    insert_account(&conn, "acc-log-rb", "现金", "cash", "CNY");

    let mut a = make_input("acc-log-rb", TransactionKind::Income, 1000, "2026-07-01");
    a.idempotency_key = Some("dup-rb".into());
    let mut b = make_input("acc-log-rb", TransactionKind::Income, 2000, "2026-07-02");
    b.idempotency_key = Some("dup-rb".into());

    let events = capture_events(|| {
        let err = TransactionBatch::run(&conn, vec![a, b], false).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("unique"),
            "同键重复应触发唯一索引约束，实际: {err:?}"
        );
    });

    let summary = find_batch_summary(&events).expect("回滚后仍应有批次汇总日志");
    assert_eq!(summary.level, Level::INFO);
    assert_eq!(field_value(summary, "total"), Some("2"));
    assert_eq!(field_value(summary, "failed"), Some("1"), "应含失败条数");
    assert!(field_value(summary, "elapsed_ms").is_some());
}

/// 部分行无效但批次提交成功：汇总行含非零失败条数（有效行落库、无效行跳过）。
#[test]
fn batch_create_logs_failed_count_with_invalid_row() {
    let conn = setup();
    insert_account(&conn, "acc-log-part", "现金", "cash", "CNY");

    // 失败行：转账未指定目标账户 → Invalid；成功行：普通收入。
    let inputs = vec![
        make_input(
            "acc-log-part",
            TransactionKind::Transfer,
            1000,
            "2026-07-01",
        ),
        make_input("acc-log-part", TransactionKind::Income, 1000, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = TransactionBatch::run(&conn, inputs, false).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].success, "转账未指定目标账户应失败");
        assert!(!r[0].duplicate && r[0].id.is_none(), "失败行应无 id");
        let msg = r[0].error.as_deref().expect("失败行应带 error");
        assert!(
            msg.contains("目标账户"),
            "错误信息应说明转账约束，实际: {msg}"
        );
        assert!(
            r[1].success && !r[1].duplicate && r[1].id.is_some(),
            "同批有效行应正常落库"
        );
    });

    let summary = find_batch_summary(&events).expect("应有一条批次汇总日志");
    assert_eq!(summary.level, Level::INFO);
    assert_eq!(field_value(summary, "total"), Some("2"));
    assert_eq!(field_value(summary, "failed"), Some("1"), "应含失败条数");

    // 单条校验失败不影响同批（外部可观察结果）：仅有效行落库。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "无效行不落库，有效行正常落库");
}

/// 零金额行：单条校验失败（金额必须大于 0），不影响同批其他行落库（issue #66
/// 迁自 `transactions` 模块旧 `batch_rejects_zero_amount`，改经 `run` 断言）。
#[test]
fn batch_create_zero_amount_row_isolated() {
    let conn = setup();
    insert_account(&conn, "acc-log-zero", "现金", "cash", "CNY");

    let inputs = vec![
        TransactionInput {
            amount_cents: 0,
            ..make_input("acc-log-zero", TransactionKind::Income, 100, "2026-07-01")
        },
        make_input("acc-log-zero", TransactionKind::Income, 1000, "2026-07-02"),
    ];
    let r = TransactionBatch::run(&conn, inputs, false).unwrap();
    assert_eq!(r.len(), 2);
    assert!(!r[0].success, "零金额应校验失败");
    assert!(!r[0].duplicate && r[0].id.is_none(), "失败行应无 id");
    let msg = r[0].error.as_deref().expect("失败行应带 error");
    assert!(
        msg.contains("大于 0"),
        "错误信息应说明金额约束，实际: {msg}"
    );
    assert!(
        r[1].success && !r[1].duplicate && r[1].id.is_some(),
        "同批有效行应正常落库"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "仅有效行落库");
}
