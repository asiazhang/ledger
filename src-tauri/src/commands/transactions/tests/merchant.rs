//! 商户携带收口（issue #188 / ADR-0028）：按 kind 拒绝/放行、软删商户的历史引用；
//! 以及「即建商户」证据外传（issue #331 / ADR-0044 决策 4）。

use super::super::*;
use super::common::{insert_account, make_input, setup};
use rusqlite::Connection;

use crate::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use crate::transaction::amount::TransactionKind;
use rusqlite::params;

// ---------------------------------------------------------------------------
// 商户携带收口（issue #188 / ADR-0028）：行为层按 kind 拒绝/放行
// ---------------------------------------------------------------------------
fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name],
    )
    .unwrap();
}

/// expense / income / refund 可携带商户：创建成功且读回 merchant_id 正确。
#[test]
fn create_income_expense_refund_with_merchant() {
    let conn = setup();
    insert_account(&conn, "acc-m", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    let income_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-m", TransactionKind::Income, 500, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    // refund 可携带商户（创建时携带的商户被继承覆盖，读回为原支出商户）
    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            kind: TransactionKind::Refund,
            merchant_id: Some("mer-jd".into()),
            refund_of_transaction_id: Some(expense_id.clone()),
            ..make_input("acc-m", TransactionKind::Refund, 100, "2026-01-03")
        },
    )
    .unwrap()
    .id;

    for (id, expect_merchant) in [
        (&expense_id, Some("mer-jd")),
        (&income_id, Some("mer-jd")),
        (&refund_id, Some("mer-jd")),
    ] {
        let merchant_id: Option<String> = conn
            .query_row(
                "SELECT merchant_id FROM transactions WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            merchant_id.as_deref(),
            expect_merchant,
            "交易 {id} 商户不符"
        );
    }
}

/// transfer / buy / sell / dividend / split 携带商户 → 行为层拒绝（schema 不设 kind 限制）。
#[test]
fn create_txn_with_merchant_rejected_for_non_merchant_kinds() {
    let conn = setup();
    insert_account(&conn, "acc-m", "现金", "cash", "CNY");
    insert_account(&conn, "acc-m-to", "银行", "bank", "CNY");
    insert_account(&conn, "acc-m-inv", "证券", "investment", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    // transfer：转出/转入账户齐备，仅因携带商户被拒。
    let err = create_transaction_internal(
        &conn,
        TransactionInput {
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 交易类型 transfer 不能携带商户");

    // buy / sell 携带商户：即使投资字段齐备也在行为层被拒（先于投资域 prepare）。
    for kind in [TransactionKind::Buy, TransactionKind::Sell] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                kind,
                merchant_id: Some("mer-jd".into()),
                instrument_id: Some("inst-x".into()),
                quantity: Some(10.0),
                price_cents: Some(1000),
                fee_cents: Some(0),
                ..make_input("acc-m-inv", kind, 10000, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带商户"),
            "{kind} 应报「不能携带商户」，实际: {err}"
        );
    }

    // dividend / split：携带商户时商户拒绝优先于「暂不支持」（两者均拒绝且不落库）。
    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                kind,
                merchant_id: Some("mer-jd".into()),
                ..make_input("acc-m", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带商户"),
            "{kind} 携带商户应报「不能携带商户」，实际: {err}"
        );
    }

    // 全部被拒，无任何落库。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "拒绝的交易不应落库");
}

/// 修改路径同款收口：把既有交易改成携带商户的 transfer → 拒绝且事务回滚。
#[test]
fn update_txn_with_merchant_rejected_for_transfer() {
    let conn = setup();
    insert_account(&conn, "acc-m", "现金", "cash", "CNY");
    insert_account(&conn, "acc-m-to", "银行", "bank", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        make_input("acc-m", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-02")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 交易类型 transfer 不能携带商户");
    // 拒绝后原交易保持不变。
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.merchant_id, None);
}

/// 读回（list / get / search）携带 merchant_id：软删商户的历史交易读回 merchant_id
/// 照常保留（商户名由前端经参考表解析，改名即时生效——引用指向 id 不回刷历史行）。
#[test]
fn read_back_carries_merchant_id_after_merchant_soft_delete_and_rename() {
    let conn = setup();
    insert_account(&conn, "acc-m", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            note: Some("备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    // 软删商户 + 改名：历史交易 merchant_id 不变（指向 id）。
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-jd'", [])
        .unwrap();
    conn.execute("UPDATE merchants SET name='京东商城' WHERE id='mer-jd'", [])
        .unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"));

    let listed = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(listed.items[0].merchant_id.as_deref(), Some("mer-jd"));

    let searched = crate::commands::search::search_transactions_internal(
        &conn, "备注", 1, 10, None, None, None, None,
    )
    .unwrap();
    assert_eq!(searched.items.len(), 1);
    assert_eq!(searched.items[0].merchant_id.as_deref(), Some("mer-jd"));
}

/// 软删商户后，历史交易仍可修改其他字段（保持原商户=历史引用，跳过在用校验）：
/// 与账户/分类更新语义一致——引用已软删参考数据不阻止编辑既有行。
#[test]
fn update_historical_txn_keeps_soft_deleted_merchant() {
    let conn = setup();
    insert_account(&conn, "acc-m", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");
    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            note: Some("旧备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-jd'", [])
        .unwrap();

    // 只改备注、保持原商户（提交当前 merchant_id）→ 修改成功，商户引用保留。
    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.note.as_deref(), Some("新备注"));
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"), "商户引用应保留");

    // 改选其他（在用）商户 → 成功。
    insert_merchant(&conn, "mer-pdd", "拼多多");
    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_id: Some("mer-pdd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-pdd"));

    // 改选已软删商户 → 拒绝（新选择仍须在用）。
    let err = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
    // 拒绝后原商户不变（事务回滚）。
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-pdd"));
}

// ---------------------------------------------------------------------------
// 即建商户证据外传（issue #331 / ADR-0044 决策 4）：创建/修改编排入口把「是否即建」
// 作为证据返回，壳层据此经信号映射单点判定发射。本组测试锁定证据口径（真/假），
// 并把行为层实际返回的证据喂回 [`signals_for`] 组合断言发射结果——
// 「仅命中复用零信号」由此可执行验证。
// ---------------------------------------------------------------------------

/// 商户表在用行数（含软删不计）。
fn active_merchant_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM merchants WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// 交易行上的商户引用。
fn merchant_id_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT merchant_id FROM transactions WHERE id=?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 创建携带新商户名 → 即建：证据真、商户行落库、交易引用它；
/// 证据喂映射单点 → 参考失效信号（壳层将要发射的就是这个）。
#[test]
fn create_with_new_merchant_name_reports_merchant_created() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("盒马".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(
        write.evidence,
        WriteEvidence::MerchantCreated(true),
        "新名字即建应为真"
    );
    assert_eq!(active_merchant_count(&conn), 1, "商户行应随交易落库");
    let merchant_id = merchant_id_of(&conn, &write.id).expect("交易应引用即建的商户");
    let name: String = conn
        .query_row(
            "SELECT name FROM merchants WHERE id=?1",
            params![merchant_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "盒马");

    // 组合断言：证据真 → 参考失效信号（两壳据此发射）。
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, write.evidence),
        &[Signal::LedgerChanged]
    );
}

/// 创建名字命中复用 → 证据假、零信号（不播无谓重拉）。
#[test]
fn create_with_hit_merchant_name_reports_reuse_and_zero_signal() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("京东".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(write.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(active_merchant_count(&conn), 1, "命中复用不新建");
    // 组合断言：仅命中复用 → 零信号。
    assert_eq!(signals_for(WriteOp::CreateTransaction, write.evidence), &[]);
}

/// 直接带 merchant_id / 不带商户 → 证据恒假、零信号。
#[test]
fn create_without_new_name_reports_false_evidence() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let with_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let without = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 500, "2026-01-02"),
    )
    .unwrap();

    assert_eq!(with_id.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(without.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, with_id.evidence),
        &[]
    );
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, without.evidence),
        &[]
    );
}

/// refund 携带商户名被继承语义忽略（不解析、不即建）→ 证据假、无碎商户。
#[test]
fn create_refund_ignores_merchant_name_and_reports_false() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("京东".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let refund = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("不存在的商户".into()),
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-ev", TransactionKind::Refund, 100, "2026-01-02")
        },
    )
    .unwrap();

    assert_eq!(refund.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(active_merchant_count(&conn), 1, "refund 不即建商户");
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, refund.evidence),
        &[]
    );
}

/// 修改为带新商户名 → 证据真、商户行落库；映射单点 → 参考失效信号。
#[test]
fn update_to_new_merchant_name_reports_merchant_created() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");
    let id = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;

    let evidence = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_name: Some("物美".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(evidence, WriteEvidence::MerchantCreated(true));
    assert_eq!(active_merchant_count(&conn), 1);
    assert_eq!(
        signals_for(WriteOp::UpdateTransaction, evidence),
        &[Signal::LedgerChanged]
    );
}

/// 修改名字命中复用 / 保持 merchant_id → 证据假、零信号。
#[test]
fn update_reusing_merchant_reports_false_and_zero_signal() {
    let conn = setup();
    insert_account(&conn, "acc-ev", "现金", "cash", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");

    // 改成名字命中：复用既有商户，证据假。
    let id = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;
    let hit = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_name: Some("京东".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    assert_eq!(hit, WriteEvidence::MerchantCreated(false));
    assert_eq!(signals_for(WriteOp::UpdateTransaction, hit), &[]);

    // 保持 merchant_id（仅改其他字段）：证据假。
    let keep = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_id: Some("mer-jd".into()),
            note: Some("只改备注".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    assert_eq!(keep, WriteEvidence::MerchantCreated(false));
    assert_eq!(signals_for(WriteOp::UpdateTransaction, keep), &[]);
}
