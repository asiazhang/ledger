//! 分类携带收口（issue #582）：行为层按 kind 拒绝/放行——expense / income 携带，
//! refund 忽略调用方填值、继承原支出分类（与账户/币种/商户同款继承语义），
//! transfer / buy / sell 携带拒绝；dividend / split 与「暂不支持」并存的拒绝优先级；
//! 修改路径与批量导入路径同款收口（先例：[`super::merchant`] 商户携带收口）。

use super::super::*;
use super::common::{insert_account, make_input, setup};
use crate::error::AppError;
use crate::transaction::amount::TransactionKind;
use rusqlite::{Connection, params};

/// 断言错误为码化拒绝且码/文案正确（锁定「码化拒绝」契约，先例：[`crate::categories::tests`]）。
fn assert_coded_rejection(err: AppError, code: &str, message_part: &str) {
    match err {
        AppError::Coded {
            code: actual,
            message,
            ..
        } => {
            assert_eq!(actual, code);
            assert!(
                message.contains(message_part),
                "应含「{message_part}」，实际: {message}"
            );
        }
        other => panic!("应为码化错误，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 分类携带收口（issue #582）：行为层按 kind 拒绝/放行
// ---------------------------------------------------------------------------
fn insert_category(conn: &Connection, id: &str, name: &str, kind: &str) {
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,NULL,NULL,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind],
    )
    .unwrap();
}

/// 交易行上的分类引用。
fn category_id_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT category_id FROM transactions WHERE id=?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 未删除交易行数（断言拒绝不落库）。
fn active_txn_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// expense / income 可携带分类（读回 category_id 正确）；transfer / buy / sell
/// 携带分类 → 行为层拒绝（schema 不设 kind 限制）；不携带分类的 transfer 行为不变。
#[test]
fn create_expense_income_carry_category_transfer_buy_sell_rejected() {
    let conn = setup();
    insert_account(&conn, "acc-c", "现金", "cash", "CNY");
    insert_account(&conn, "acc-c-to", "银行", "bank", "CNY");
    insert_account(&conn, "acc-c-inv", "证券", "investment", "CNY");
    insert_category(&conn, "cat-food", "餐饮", "expense");
    insert_category(&conn, "cat-salary", "工资", "income");

    // expense / income：携带分类创建成功且读回 category_id 正确。
    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some("cat-food".into()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    let income_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some("cat-salary".into()),
            ..make_input("acc-c", TransactionKind::Income, 500, "2026-01-02")
        },
    )
    .unwrap()
    .id;
    assert_eq!(
        category_id_of(&conn, &expense_id).as_deref(),
        Some("cat-food")
    );
    assert_eq!(
        category_id_of(&conn, &income_id).as_deref(),
        Some("cat-salary")
    );

    // transfer：转出/转入账户齐备，仅因携带分类被拒（码化拒绝，码随测试锁定）。
    let err = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some("cat-food".into()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-03")
        },
    )
    .unwrap_err();
    assert_coded_rejection(err, "transaction.category-unsupported", "不能携带分类");

    // buy / sell 携带分类：即使投资字段齐备也在行为层被拒（先于投资域 prepare）。
    for kind in [TransactionKind::Buy, TransactionKind::Sell] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                category_id: Some("cat-food".into()),
                instrument_id: Some("inst-x".into()),
                quantity: Some(10.0),
                price_cents: Some(1000),
                fee_cents: Some(0),
                ..make_input("acc-c-inv", kind, 10000, "2026-01-04")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带分类"),
            "{kind} 应报「不能携带分类」，实际: {err}"
        );
    }

    // 不携带分类的 transfer 行为不变（创建成功、分类为空）。
    let plain_transfer = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-05")
        },
    )
    .unwrap()
    .id;
    assert_eq!(category_id_of(&conn, &plain_transfer), None);

    // 被拒的交易全部未落库：仅 expense / income / 无分类 transfer 三行。
    assert_eq!(active_txn_count(&conn), 3, "拒绝的交易不应落库");
}

/// refund 忽略调用方填的分类、继承原支出分类（与账户/币种/商户同款继承语义）。
#[test]
fn create_refund_ignores_caller_category_and_inherits_original() {
    let conn = setup();
    insert_account(&conn, "acc-c", "现金", "cash", "CNY");
    insert_category(&conn, "cat-food", "餐饮", "expense");
    insert_category(&conn, "cat-toy", "玩具", "expense");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some("cat-food".into()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    // 调用方填了其他分类：被继承语义覆盖，读回为原支出分类。
    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Refund,
            category_id: Some("cat-toy".into()),
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-c", TransactionKind::Refund, 100, "2026-01-02")
        },
    )
    .unwrap()
    .id;
    assert_eq!(
        category_id_of(&conn, &refund_id).as_deref(),
        Some("cat-food"),
        "refund 应继承原支出分类、忽略调用方填值"
    );
}

/// dividend / split 维持「暂不支持」；携带分类时分类拒绝优先于「暂不支持」
/// （比照商户收口先例，两者均拒绝且不落库）。
#[test]
fn create_dividend_split_with_category_reports_category_rejection_first() {
    let conn = setup();
    insert_account(&conn, "acc-c", "现金", "cash", "CNY");
    insert_category(&conn, "cat-food", "餐饮", "expense");

    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        // 携带分类：报分类拒绝（先于「暂不支持」）。
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                category_id: Some("cat-food".into()),
                ..make_input("acc-c", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带分类"),
            "{kind} 携带分类应报「不能携带分类」，实际: {err}"
        );

        // 不携带分类：维持既有「暂不支持」拒绝不变。
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                ..make_input("acc-c", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("暂不支持"),
            "{kind} 不携带分类应维持「暂不支持」，实际: {err}"
        );
    }

    assert_eq!(active_txn_count(&conn), 0, "全部被拒，无任何落库");
}

/// 修改路径同款收口：把既有交易改成携带分类的 transfer → 拒绝且事务回滚，
/// 原交易保持不变。
#[test]
fn update_to_transfer_with_category_rejected_and_rolls_back() {
    let conn = setup();
    insert_account(&conn, "acc-c", "现金", "cash", "CNY");
    insert_account(&conn, "acc-c-to", "银行", "bank", "CNY");
    insert_category(&conn, "cat-food", "餐饮", "expense");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some("cat-food".into()),
            ..make_input("acc-c", TransactionKind::Expense, 500, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some("cat-food".into()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-02")
        },
    )
    .unwrap_err();
    assert_coded_rejection(err, "transaction.category-unsupported", "不能携带分类");
    // 拒绝后原交易保持不变。
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.category_id.as_deref(), Some("cat-food"));
}

/// 批量导入路径同一收口：批次中携带分类的转账按既有批次失败语义处理
/// （该行 success:false + 错误信息，不影响同批其他行），不静默剥掉分类落库。
#[test]
fn batch_transfer_with_category_fails_row_without_silent_strip() {
    let conn = setup();
    insert_account(&conn, "acc-c", "现金", "cash", "CNY");
    insert_account(&conn, "acc-c-to", "银行", "bank", "CNY");
    insert_category(&conn, "cat-food", "餐饮", "expense");

    let inputs = vec![
        // 同批合法行：带分类的 expense 正常落库。
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some("cat-food".into()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
        // 携带分类的 transfer：单行失败（码化 Invalid 归「单行失败」编排语义）。
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some("cat-food".into()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-02")
        },
        // 同批合法行：不带分类的 transfer 照常落库。
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 2000, "2026-01-03")
        },
    ];
    let outcome = TransactionBatch::run(&conn, inputs, false).unwrap();
    assert_eq!(outcome.results.len(), 3);
    assert!(outcome.results[0].success && outcome.results[0].id.is_some());
    assert!(
        !outcome.results[1].success && outcome.results[1].id.is_none(),
        "携带分类的转账应单行失败"
    );
    assert!(
        outcome.results[1]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("不能携带分类"),
        "失败行应携带分类拒绝错误，实际: {:?}",
        outcome.results[1].error
    );
    assert!(outcome.results[2].success && outcome.results[2].id.is_some());

    // 批次照常提交（单行失败不回滚整批）：仅两行落库，失败行整体不落库
    // （不静默剥掉分类写入）。
    assert_eq!(active_txn_count(&conn), 2);
    let categorized_transfer: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE kind='transfer' AND category_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(categorized_transfer, 0, "不应存在带分类的转账行");
}
