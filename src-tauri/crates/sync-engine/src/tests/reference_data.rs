use super::super::{DomainCommand, OpOutcome, ingest_ops, parked_ops, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use ledger_accounts::{
    self, AccountCommand, AccountInput, AccountType, AccountUpdateInput, create_account,
    delete_account, update_account,
};
use ledger_categories::{
    self, CategoryCommand, CategoryInput, ReorderItem, create_category, delete_category,
    reorder_categories, update_category,
};
use ledger_merchants::{
    self, MerchantCommand, create_merchant_by_name, delete_merchant, update_merchant,
};
use rusqlite::Connection;
use tauri_app_lib::test_support;

fn account_input(name: &str) -> AccountInput {
    AccountInput {
        name: name.into(),
        kind: AccountType::Cash,
        currency_code: "CNY".into(),
        initial_balance_cents: Some(1000),
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    }
}

fn read_account(conn: &Connection, id: &str) -> Option<(String, String, i64, bool)> {
    conn.query_row(
        "SELECT name, currency_code, initial_balance_cents, is_deleted FROM accounts WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)),
    )
    .ok()
}

#[test]
fn account_create_update_delete_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_account(&conn_a, account_input("现金")).unwrap();
    update_account(
        &conn_a,
        &id,
        AccountUpdateInput {
            name: Some("钱包".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    delete_account(&conn_a, &id).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 3, "三个写入口各产出一条 op：{ops:?}");
    assert!(ops.iter().all(|op| op.command.subject().0 == "account"));

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));
    assert_eq!(
        read_account(&conn_b, &id),
        read_account(&conn_a, &id),
        "重放后账户状态一致（含软删）"
    );
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn account_concurrent_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    test_support::seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    update_account(
        &conn_a,
        "acc-1",
        AccountUpdateInput {
            name: Some("甲".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    update_account(
        &conn_b,
        "acc-1",
        AccountUpdateInput {
            name: Some("乙".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    let name_on_a = read_account(&conn_a, "acc-1").unwrap().0;
    let name_on_b = read_account(&conn_b, "acc-1").unwrap().0;
    assert_eq!(name_on_a, "乙");
    assert_eq!(name_on_b, "乙");
    assert!(
        reports_on_b
            .iter()
            .any(|r| r.outcome == OpOutcome::Superseded),
        "A 的并发编辑应为 LWW 输者：{reports_on_b:?}"
    );
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn black_hole_adjust_produces_account_and_transaction_ops() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let account_id = create_account(
        &conn_a,
        AccountInput {
            name: "美元现金".into(),
            kind: AccountType::Cash,
            currency_code: "USD".into(),
            initial_balance_cents: Some(100),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    test_support::seed_exchange_rate(&conn_a, "USD", "CNY", 7.2);
    test_support::seed_fx_rate_history(&conn_a, "fxh-bh", "USD", "CNY", "2026-01-05", 7.2);
    let (tx_id, created) = ledger_accounts::adjust_account_balance(
        &conn_a,
        &account_id,
        &ledger_accounts::AccountBalanceAdjustInput {
            target_balance_cents: 900,
            date: "2026-01-10".into(),
            note: None,
        },
    )
    .unwrap();
    assert!(created, "黑洞账户应即建");

    wire_in(&conn_b, &wire_out(&conn_a));

    let hidden: i64 = conn_b
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_hidden=1 AND name='无(USD)' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hidden, 1, "黑洞创建 op 重放后 B 端黑洞就位");
    let amount: i64 = conn_b
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1 AND is_deleted=0",
            [&tx_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 800, "调整转账重放落地");
}

#[test]
fn merchant_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_merchant_by_name(&conn_a, "京东").unwrap();
    let again = create_merchant_by_name(&conn_a, "京东").unwrap();
    assert_eq!(id, again, "命中复用同一商户");
    update_merchant(
        &conn_a,
        &id,
        ledger_merchants::MerchantUpdateInput {
            name: Some("京东集团".into()),
        },
    )
    .unwrap();
    delete_merchant(&conn_a, &id).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 3, "即建一次 + 改名 + 删除：{ops:?}");
    assert!(
        ops.iter().any(|op| matches!(
            &op.command,
            DomainCommand::Merchant(MerchantCommand::Create { name, .. }) if name == "京东"
        )),
        "创建 op 随行落定名"
    );

    wire_in(&conn_b, &wire_out(&conn_a));
    let row = |conn: &Connection| -> (String, bool) {
        conn.query_row(
            "SELECT name, is_deleted FROM merchants WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .unwrap()
    };
    assert_eq!(row(&conn_a), row(&conn_b), "重放后商户状态一致");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn category_lifecycle_and_reorder_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let parent = create_category(
        &conn_a,
        CategoryInput {
            name: "餐饮".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let child = create_category(
        &conn_a,
        CategoryInput {
            name: "咖啡".into(),
            kind: "expense".into(),
            parent_id: Some(parent.clone()),
            icon: Some("☕".into()),
        },
    )
    .unwrap();
    update_category(
        &conn_a,
        &child,
        ledger_categories::CategoryUpdateInput {
            name: Some("精品咖啡".into()),
            icon: None,
            parent_id: None,
        },
    )
    .unwrap();
    reorder_categories(
        &conn_a,
        vec![
            ReorderItem {
                id: parent.clone(),
                sort_order: 1,
            },
            ReorderItem {
                id: child.clone(),
                sort_order: 0,
            },
        ],
    )
    .unwrap();
    delete_category(&conn_a, &child).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(
        ops.len(),
        5,
        "create×2 + update + reorder + delete：{ops:?}"
    );
    assert!(
        ops.iter()
            .any(|op| matches!(&op.command, DomainCommand::Category(CategoryCommand::Reorder { items }) if items.len() == 2)),
        "重排 op 整批随行"
    );

    wire_in(&conn_b, &wire_out(&conn_a));
    let row = |conn: &Connection, id: &str| -> (String, Option<String>, i64, bool) {
        conn.query_row(
            "SELECT name, parent_id, sort_order, is_deleted FROM categories WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)),
        )
        .unwrap()
    };
    assert_eq!(row(&conn_a, &parent), row(&conn_b, &parent));
    assert_eq!(row(&conn_a, &child), row(&conn_b, &child));
    assert!(row(&conn_a, &child).3, "子分类已软删");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn command_subject_is_entity_id_across_domains() {
    use std::borrow::Cow;
    assert_eq!(
        ("account", Some(Cow::Borrowed("acc-1"))),
        DomainCommand::Account(AccountCommand::Delete { id: "acc-1".into() }).subject()
    );
    assert_eq!(
        ("category", Some(Cow::Borrowed("cat-1"))),
        DomainCommand::Category(CategoryCommand::Delete { id: "cat-1".into() }).subject()
    );
    assert_eq!(
        ("merchant", Some(Cow::Borrowed("m-1"))),
        DomainCommand::Merchant(MerchantCommand::Delete { id: "m-1".into() }).subject()
    );
    assert_eq!(
        (
            "exchange_rate",
            Some(Cow::<str>::Owned("EUR->CNY".to_string()))
        ),
        DomainCommand::ExchangeRate(ledger_investment::ExchangeRateCommand::Upsert {
            id: "er-1".into(),
            base_code: "EUR".into(),
            quote_code: "CNY".into(),
            rate: 8.0,
            priced_at: "2026-01-10".into(),
            source: None,
        })
        .subject()
    );
    assert_eq!(
        (
            "price",
            Some(Cow::<str>::Owned("inst-1|2026-01-12".to_string()))
        ),
        DomainCommand::Price(ledger_investment::PriceCommand::ManualPrice {
            instrument_id: "inst-1".into(),
            date: "2026-01-15".into(),
            price_cents: 100,
        })
        .subject()
    );
    assert_eq!(
        ("price", Some(Cow::Borrowed("inst-1"))),
        DomainCommand::Price(ledger_investment::PriceCommand::MarketPrice {
            instrument_id: "inst-1".into(),
            price_cents: 100,
            currency_code: "CNY".into(),
            priced_at: "2026-01-15".into(),
            source: None,
        })
        .subject()
    );
}

fn read_credit_terms(conn: &Connection, id: &str) -> (Option<i64>, Option<i64>, Option<i64>) {
    conn.query_row(
        "SELECT credit_limit_cents, statement_day, due_day FROM accounts WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn account_credit_terms_create_and_update_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_account(
        &conn_a,
        AccountInput {
            name: "招行信用卡".into(),
            kind: AccountType::Credit,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(-12_345),
            credit_limit_cents: Some(5_000_000),
            statement_day: Some(5),
            due_day: Some(25),
        },
    )
    .unwrap();
    update_account(
        &conn_a,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(Some(6_000_000)),
            statement_day: Some(Some(10)),
            due_day: Some(Some(28)),
            is_archived: None,
        },
    )
    .unwrap();

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Applied),
        "创建 + 编辑两条 op 应全部应用，实际: {reports:?}"
    );
    assert_eq!(
        read_credit_terms(&conn_b, &id),
        (Some(6_000_000), Some(10), Some(28))
    );
    assert_eq!(
        read_credit_terms(&conn_b, &id),
        read_credit_terms(&conn_a, &id),
        "重放后档案两端一致"
    );

    update_account(
        &conn_a,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(None),
            statement_day: None,
            due_day: Some(None),
            is_archived: None,
        },
    )
    .unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
        "整批重投：已应用段跳过、新段应用，不得挂起或失败，实际: {reports:?}"
    );
    assert_eq!(read_credit_terms(&conn_b, &id), (None, Some(10), None));
    assert_eq!(
        read_credit_terms(&conn_b, &id),
        read_credit_terms(&conn_a, &id)
    );
}

#[test]
fn legacy_account_op_without_credit_fields_replays_as_unset() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_account(
        &conn_a,
        AccountInput {
            kind: AccountType::Credit,
            credit_limit_cents: Some(5_000_000),
            statement_day: Some(5),
            due_day: Some(25),
            ..account_input("招行信用卡")
        },
    )
    .unwrap();

    let mut legacy: serde_json::Value = serde_json::from_str(&wire_out(&conn_a)[0]).unwrap();
    legacy["schema_version"] = serde_json::json!(24);
    let row = legacy["command"]["payload"]["row"].as_object_mut().unwrap();
    for key in ["credit_limit_cents", "statement_day", "due_day"] {
        assert!(row.remove(key).is_some(), "前置：op 行载荷应携带 {key}");
    }
    let raw = serde_json::to_string(&legacy).unwrap();

    let reports = wire_in(&conn_b, &[raw]);
    assert!(
        matches!(reports[0].outcome, OpOutcome::Applied),
        "旧格式 op 应照常应用，实际: {:?}",
        reports[0].outcome
    );
    assert_eq!(read_credit_terms(&conn_b, &id), (None, None, None));
    assert_eq!(read_account(&conn_b, &id), read_account(&conn_a, &id));

    let again = wire_in(&conn_b, &[serde_json::to_string(&legacy).unwrap()]);
    assert_eq!(again[0].outcome, OpOutcome::Skipped);
}

fn read_archived(conn: &Connection, id: &str) -> Option<bool> {
    conn.query_row("SELECT is_archived FROM accounts WHERE id=?1", [id], |r| {
        Ok(r.get::<_, i64>(0)? != 0)
    })
    .ok()
}

#[test]
fn account_set_archived_op_replays_and_converges() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_account(&conn_a, account_input("借出·张三")).unwrap();
    ledger_accounts::set_account_archived(&conn_a, &id, true).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 2, "create + set_archived 各一条 op：{ops:?}");
    assert!(
        matches!(
            &ops[1].command,
            DomainCommand::Account(AccountCommand::SetArchived { archived: true, .. })
        ),
        "归档产出 SetArchived 变体"
    );

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));
    assert_eq!(read_archived(&conn_b, &id), Some(true), "重放后归档态收敛");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    ledger_accounts::set_account_archived(&conn_a, &id, false).unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
        "重投不产生挂起：{reports:?}"
    );
    assert_eq!(
        read_archived(&conn_b, &id),
        Some(false),
        "恢复后收敛回未归档"
    );

    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn set_archived_op_from_newer_schema_parks_on_old_end() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    let id = create_account(&conn_a, account_input("借出·李四")).unwrap();
    let create_op = &wire_out(&conn_a)[0];
    let reports = wire_in(&conn_b, std::slice::from_ref(create_op));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));

    ledger_accounts::set_account_archived(&conn_a, &id, true).unwrap();
    let mut op = read_ops(&conn_a).unwrap().remove(1);
    op.schema_version += 1;
    let ahead = serde_json::to_string(&op).unwrap();

    let reports = ingest_ops(&conn_b, &[ahead]).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.schema-ahead"
        ),
        "旧版本端把归档 op 挂起并携带码化原因"
    );
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, op.op_id, "op 身份保留（升级后可重放）");
    assert_eq!(read_archived(&conn_b, &id), Some(false), "未执行不落地");
}

#[test]
fn unknown_account_action_parks_and_never_drops() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);

    let future_action = r#"{"op_id":"op-account-future","device_id":"dev-x","clock":9,"schema_version":20,"command":{"entity":"account","payload":{"action":"set_archived_forever","id":"acc-1","archived":true}}}"#;

    let reports = ingest_ops(&conn, &[future_action.to_string()]).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.op-undecodable"
        ),
        "未知 action 挂起并携带码化原因"
    );
    let parked = parked_ops(&conn).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].payload, future_action, "载荷原样保存");
    assert_eq!(read_archived(&conn, "acc-1"), Some(false), "未执行不落地");
}
