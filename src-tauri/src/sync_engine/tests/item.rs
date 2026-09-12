//! 物品域的全域 op 产出与重放收敛（issue #860）：溯源创建、修改、处置、软删；
//! 源端折算随行（重放端无汇率也不重折算，ADR-0091 决策 3）。

use super::super::{OpOutcome, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::item::{
    ItemDisposeInput, ItemInput, create_item, delete_item, dispose_item, update_item,
};
use crate::test_support;
use crate::test_support::seed_account;
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::write::protocol;

fn expense_input(account_id: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: 100_000,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        note: Some("买手机".into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

fn item_input(tx_id: &str, note: &str) -> ItemInput {
    ItemInput {
        name: "手机".into(),
        purchase_date: "2026-01-10".into(),
        total_cost_cents: 100_000,
        currency_code: "CNY".into(),
        note: Some(note.into()),
        purchase_transaction_id: Some(tx_id.into()),
    }
}

#[test]
fn item_lifecycle_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // 购买交易（A 端经行为层创建，产交易 op）→ 溯源创建 → 修改 → 处置 → 软删。
    let tx_id = protocol::create(&conn_a, expense_input("acc-1"))
        .unwrap()
        .id;
    let item_id = create_item(&conn_a, item_input(&tx_id, "主力机"), &mut || {}).unwrap();
    update_item(
        &conn_a,
        &item_id,
        item_input(&tx_id, "换成备用机"),
        &mut || {},
    )
    .unwrap();
    dispose_item(
        &conn_a,
        &item_id,
        ItemDisposeInput {
            disposal_date: "2026-02-01".into(),
            residual_value_cents: Some(5_000),
        },
        &mut || {},
    )
    .unwrap();

    let ops = read_ops(&conn_a).unwrap();
    // 交易 op + 物品 create/update/dispose 各一条（软删在后续步骤）。
    assert_eq!(ops.len(), 4, "交易 + create/update/dispose：{ops:?}");
    assert_eq!(ops[1].command.subject().0, "item");

    wire_in(&conn_b, &wire_out(&conn_a));
    let row = |conn: &rusqlite::Connection| -> (String, String, i64, String, Option<i64>) {
        conn.query_row(
            "SELECT name, status, residual_value_cents, disposal_date, cost_native_cents \
             FROM items WHERE id=?1",
            [&item_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, String>(3)?,
                    r.get(4)?,
                ))
            },
        )
        .unwrap()
    };
    let (native_on_a, native_on_b) = (row(&conn_a).4, row(&conn_b).4);
    assert_eq!(native_on_a, Some(100_000), "CNY 折算 = 原额");
    assert_eq!(native_on_a, native_on_b, "本位币成本随命令携带，两端一致");
    assert_eq!(row(&conn_a), row(&conn_b), "重放后物品状态一致");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    // 软删后重投递：全部 Skipped。
    delete_item(&conn_a, &item_id, &mut || {}).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
    let deleted: i64 = conn_b
        .query_row(
            "SELECT is_deleted FROM items WHERE id=?1",
            [&item_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1);
}

#[test]
fn item_create_replay_does_not_reconvert() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // 汇率只在 A 端存在（EUR→CNY）：源端折算 8.0；B 端无汇率——若重放端重折算
    // 会因缺汇率失败（挂起），物品缺失即失败信号。
    test_support::seed_exchange_rate(&conn_a, "EUR", "CNY", 8.0);
    let mut input = expense_input("acc-1");
    input.currency_code = "EUR".into();
    let tx_id = protocol::create(&conn_a, input).unwrap().id;
    let item_id = create_item(
        &conn_a,
        ItemInput {
            currency_code: "EUR".into(),
            ..item_input(&tx_id, "进口好物")
        },
        &mut || {},
    )
    .unwrap();

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Applied),
        "重放不依赖本地汇率表：{reports:?}"
    );
    let native: i64 = conn_b
        .query_row(
            "SELECT cost_native_cents FROM items WHERE id=?1",
            [&item_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(native, 800_000, "源端折算结果随 op 携带（100 EUR × 8.0）");
}
