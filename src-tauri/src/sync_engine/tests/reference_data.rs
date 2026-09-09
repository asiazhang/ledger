//! 参考数据字典（账户 / 分类 / 商户）的全域 op 产出与重放收敛（issue #860）。
//!
//! 双端场景 = 同进程两个引擎实例（[`super::common`] 同纪律）；各写入入口走
//! 公开写入口（域函数），op 产出与重放断言全部对准同步引擎公开接口
//! （`read_ops` / `ingest_ops` / `parked_ops`）。

use super::super::{DomainCommand, OpOutcome, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::accounts::{
    self, AccountCommand, AccountInput, AccountType, AccountUpdateInput, create_account,
    delete_account, update_account,
};
use crate::categories::{
    self, CategoryCommand, CategoryInput, ReorderItem, create_category, delete_category,
    reorder_categories, update_category,
};
use crate::merchants::{
    self, MerchantCommand, create_merchant_by_name, delete_merchant, update_merchant,
};
use crate::test_support;
use rusqlite::Connection;

fn account_input(name: &str) -> AccountInput {
    AccountInput {
        name: name.into(),
        kind: AccountType::Cash,
        currency_code: "CNY".into(),
        initial_balance_cents: Some(1000),
    }
}

/// 账户业务字段快照（簿记戳是各端本地事实，不参与状态等值判定）。
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
        },
    )
    .unwrap();
    delete_account(&conn_a, &id).unwrap();

    // A 端三入口各产出一条 op（create / update / delete）。
    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 3, "三个写入口各产出一条 op：{ops:?}");
    assert!(ops.iter().all(|op| op.command.entity() == "account"));

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));
    assert_eq!(
        read_account(&conn_b, &id),
        read_account(&conn_a, &id),
        "重放后账户状态一致（含软删）"
    );
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    // 重复投递：全部 Skipped，无第二次效果。
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn account_concurrent_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 同一账户在两端等量种子（夹具不产 op，两端时钟同为 0），并发改名时钟相同，
    // 全序 tiebreak 落 DeviceId 字典序（dev-a < dev-b，B 末）。
    test_support::seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    update_account(
        &conn_a,
        "acc-1",
        AccountUpdateInput {
            name: Some("甲".into()),
            currency_code: None,
        },
    )
    .unwrap();
    update_account(
        &conn_b,
        "acc-1",
        AccountUpdateInput {
            name: Some("乙".into()),
            currency_code: None,
        },
    )
    .unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    // 两端收敛序末者（dev-b 的「乙」）；A 的编辑在 B 端为 LWW 输者（可审计）。
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

    // 目标账户经公开写入口创建并先同步（两端都有账户行与余额缓存行）。
    // 种子已预置 CNY 黑洞账户，用 USD 账户触发黑洞即建路径。
    let account_id = create_account(
        &conn_a,
        AccountInput {
            name: "美元现金".into(),
            kind: AccountType::Cash,
            currency_code: "USD".into(),
            initial_balance_cents: Some(100),
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    // 余额调整：黑洞即建（账户写）+ 调整转账（交易写）——两类 op 同事务产出。
    // 折算汇率只在 A 端存在：调整转账折算在源端完成、随 op 携带（ADR-0091 决策 3）。
    test_support::seed_exchange_rate(&conn_a, "USD", "CNY", 7.2);
    let (tx_id, created) = accounts::adjust_account_balance(
        &conn_a,
        &account_id,
        &crate::accounts::AccountBalanceAdjustInput {
            target_balance_cents: 900,
            date: "2026-01-10".into(),
            note: None,
        },
    )
    .unwrap();
    assert!(created, "黑洞账户应即建");

    wire_in(&conn_b, &wire_out(&conn_a));

    // B 端黑洞账户就位（is_hidden=1），调整转账落地（重放不因外键缺失挂起）。
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

    // find-or-create 未命中即建（产 op），命中复用不产第二条创建 op。
    let id = create_merchant_by_name(&conn_a, "京东").unwrap();
    let again = create_merchant_by_name(&conn_a, "京东").unwrap();
    assert_eq!(id, again, "命中复用同一商户");
    update_merchant(
        &conn_a,
        &id,
        merchants::MerchantUpdateInput {
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
        categories::CategoryUpdateInput {
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

    // 重复投递：全部 Skipped。
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

/// 命令 LWW 裁决域钉子：各域命令 subject 指向实体 id，防实体判别键漂移。
#[test]
fn command_subject_is_entity_id_across_domains() {
    assert_eq!(
        Some(("account", "acc-1")),
        DomainCommand::Account(AccountCommand::Delete { id: "acc-1".into() }).subject()
    );
    assert_eq!(
        Some(("category", "cat-1")),
        DomainCommand::Category(CategoryCommand::Delete { id: "cat-1".into() }).subject()
    );
    assert_eq!(
        Some(("merchant", "m-1")),
        DomainCommand::Merchant(MerchantCommand::Delete { id: "m-1".into() }).subject()
    );
}
