//! `commands::item` 单元测试：内部函数校验语义与失效信号回调（BDD 场景外的快速反馈）。

use rusqlite::Connection;

use crate::commands::item::{
    create_item_internal, delete_item_internal, dispose_item_internal, list_items_internal,
    update_item_internal,
};
use crate::commands::transactions::create_transaction_internal;
use crate::db::{init_db, open_in_memory};
use crate::models::{ItemDisposeInput, ItemInput, TransactionInput};
use crate::transaction::amount::TransactionKind;

fn conn() -> Connection {
    let mut conn = open_in_memory().expect("内存库创建失败");
    init_db(&mut conn).expect("迁移失败");
    conn
}

fn input(name: &str, date: &str, cost_cents: i64) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: date.into(),
        total_cost_cents: cost_cents,
        currency_code: "CNY".into(),
        note: None,
        purchase_transaction_id: None,
    }
}

/// 创建脚手架账户 + expense 购买交易（issue #207 起创建必关联交易），返回交易 id。
/// 账户行幂等插入（同 id 复用），交易入参经 Writer 接缝校验（金额>0、日期可解析、折算有汇率）。
fn seed_purchase_tx(conn: &Connection, date: &str, cost_cents: i64, currency: &str) -> String {
    conn.execute(
        "INSERT OR IGNORE INTO accounts \
         (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('acc-item-scaffold','物品脚手架','cash',?1,0,\
          '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [currency],
    )
    .unwrap();
    create_transaction_internal(
        conn,
        TransactionInput {
            merchant_name: None,
            kind: TransactionKind::Expense,
            amount_cents: cost_cents,
            currency_code: currency.into(),
            account_id: "acc-item-scaffold".into(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: date.into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap()
    .id
}

/// 关联脚手架交易的创建入参（日期/成本/币种与交易一致，后端以交易值覆盖带出）。
fn linked_input(name: &str, date: &str, cost_cents: i64, tx_id: &str) -> ItemInput {
    ItemInput {
        purchase_transaction_id: Some(tx_id.into()),
        ..input(name, date, cost_cents)
    }
}

/// 创建一件物品并返回其 id（默认不监听信号）：先建脚手架购买交易再关联创建。
fn seed(conn: &Connection, name: &str, date: &str, cost_cents: i64) -> String {
    let tx = seed_purchase_tx(conn, date, cost_cents, "CNY");
    create_item_internal(conn, linked_input(name, date, cost_cents, &tx), &mut || {}).unwrap()
}

#[test]
fn update_item_persists_fields_and_increments_version() {
    let conn = conn();
    let id = seed(&conn, "iPhone", "2026-01-15", 599_900);
    let before = &list_items_internal(&conn).unwrap()[0].item;
    let created_at = before.created_at.clone();

    let mut fired = 0;
    update_item_internal(
        &conn,
        &id,
        input("iPhone 15", "2026-01-20", 800_000),
        &mut || fired += 1,
    )
    .unwrap();
    assert_eq!(fired, 1, "修改成功应恰好发出一次失效信号");

    let items = list_items_internal(&conn).unwrap();
    let item = &items[0].item;
    assert_eq!(item.id, id, "id 不变");
    assert_eq!(item.name, "iPhone 15");
    assert_eq!(item.purchase_date, "2026-01-20");
    assert_eq!(item.total_cost_cents, 800_000);
    assert_eq!(item.cost_native_cents, 800_000);
    assert_eq!(item.version, 2, "版本应递增");
    assert_eq!(item.created_at, created_at, "created_at 应保留");
    assert!(item.updated_at >= created_at, "updated_at 应刷新");
    assert_eq!(item.status, crate::models::ItemStatus::InUse, "状态不动");
    assert_eq!(item.disposal_date, None, "处置字段不动");
    assert_eq!(item.residual_value_cents, None);
    assert!(!item.is_deleted);
}

#[test]
fn update_item_replaces_note_whole_field() {
    let conn = conn();
    let id = seed(&conn, "水杯", "2026-01-15", 5_000);
    // update 是整体替换语义：入参不带备注（None）即清除
    update_item_internal(&conn, &id, input("水杯", "2026-01-15", 5_000), &mut || {}).unwrap();
    assert_eq!(list_items_internal(&conn).unwrap()[0].item.note, None);
    let mut inp2 = input("水杯", "2026-01-15", 5_000);
    inp2.note = Some("陶瓷".into());
    update_item_internal(&conn, &id, inp2, &mut || {}).unwrap();
    assert_eq!(
        list_items_internal(&conn).unwrap()[0].item.note.as_deref(),
        Some("陶瓷")
    );
}

#[test]
fn update_item_recalculates_daily_cost() {
    let conn = conn();
    let today = crate::item::cost::today();
    let purchase = (today - chrono::Duration::days(9))
        .format("%Y-%m-%d")
        .to_string();
    let id = seed(&conn, "显示器", &purchase, 100_000);
    update_item_internal(
        &conn,
        &id,
        input("显示器 4K", &purchase, 200_000),
        &mut || {},
    )
    .unwrap();
    let entry = &list_items_internal(&conn).unwrap()[0];
    assert_eq!(entry.used_days, 10);
    assert_eq!(entry.numerator_cents, 200_000);
    assert!((entry.per_day_cents - 20_000.0).abs() < 1e-9);
}

#[test]
fn update_item_rejects_missing_id() {
    let conn = conn();
    let mut fired = 0;
    let err = update_item_internal(
        &conn,
        "no-such-id",
        input("幽灵", "2026-01-15", 100),
        &mut || fired += 1,
    )
    .unwrap_err();
    assert!(err.to_string().contains("物品不存在"));
    assert_eq!(fired, 0, "失败不应发出失效信号");
}

#[test]
fn update_item_rejects_soft_deleted_id() {
    let conn = conn();
    let id = seed(&conn, "耳机", "2026-03-01", 20_000);
    conn.execute("UPDATE items SET is_deleted=1", []).unwrap();
    let err = update_item_internal(&conn, &id, input("耳机", "2026-03-01", 20_000), &mut || {})
        .unwrap_err();
    assert!(err.to_string().contains("物品不存在"));
}

#[test]
fn update_item_validates_like_create() {
    let conn = conn();
    let id = seed(&conn, "耳机", "2026-03-01", 20_000);
    for bad in [
        input("  ", "2026-03-01", 20_000),
        input("耳机", "2026-03-01", 0),
        input("耳机", "2026/03/01", 20_000),
    ] {
        let err = update_item_internal(&conn, &id, bad, &mut || {}).unwrap_err();
        assert!(
            err.to_string().contains("不能为空")
                || err.to_string().contains("必须大于 0")
                || err.to_string().contains("日期格式无效"),
            "应报校验错误: {err}"
        );
    }
    // 币种无汇率 → Amount 接缝报错
    let mut inp = input("耳机", "2026-03-01", 20_000);
    inp.currency_code = "XYZ".into();
    let err = update_item_internal(&conn, &id, inp, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("汇率"));
}

#[test]
fn update_item_rejects_purchase_date_after_disposal_date() {
    let conn = conn();
    let id = seed(&conn, "旧手机", "2025-01-01", 300_000);
    conn.execute(
        "UPDATE items SET status='disposed', disposal_date='2025-06-01' WHERE id=?1",
        [&id],
    )
    .unwrap();
    let err = update_item_internal(
        &conn,
        &id,
        input("旧手机", "2025-06-02", 300_000),
        &mut || {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("晚于处置日期"));
    // 未落库：version 不变
    assert_eq!(list_items_internal(&conn).unwrap()[0].item.version, 1);
}

#[test]
fn create_item_requires_purchase_transaction() {
    let conn = conn();
    let mut fired = 0;
    let err = create_item_internal(&conn, input("水杯", "2026-01-15", 100), &mut || {
        fired += 1
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("物品必须关联一笔购买交易创建"),
        "应报溯源守卫错误: {err}"
    );
    assert_eq!(fired, 0, "守卫拒绝不应发出失效信号");
    assert!(
        list_items_internal(&conn).unwrap().is_empty(),
        "守卫拒绝不落库"
    );
}

#[test]
fn create_item_persists_and_returns_id() {
    let conn = conn();
    let tx = seed_purchase_tx(&conn, "2026-01-15", 599_900, "CNY");
    let mut fired = 0;
    let id = create_item_internal(
        &conn,
        linked_input("iPhone", "2026-01-15", 599_900, &tx),
        &mut || fired += 1,
    )
    .unwrap();
    assert!(!id.is_empty());
    assert_eq!(fired, 1, "创建成功应恰好发出一次失效信号");
    let items = list_items_internal(&conn).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item.id, id);
    assert_eq!(items[0].item.status, crate::models::ItemStatus::InUse);
    assert_eq!(items[0].item.cost_native_cents, 599_900);
}

#[test]
fn create_item_daily_cost_uses_cost_seam() {
    let conn = conn();
    // 相对今天构造购买日期，断言含起止两端的日历天数（10 天前购买 → 10 天）。
    let today = crate::item::cost::today();
    let purchase = today - chrono::Duration::days(9);
    let date = purchase.format("%Y-%m-%d").to_string();
    let tx = seed_purchase_tx(&conn, &date, 100_000, "CNY");
    create_item_internal(
        &conn,
        linked_input("显示器", &date, 100_000, &tx),
        &mut || {},
    )
    .unwrap();
    let items = list_items_internal(&conn).unwrap();
    assert_eq!(items[0].used_days, 10);
    assert!((items[0].per_day_cents - 10_000.0).abs() < 1e-9);
}

#[test]
fn create_item_rejects_blank_name() {
    let conn = conn();
    // 创建路径的成本/日期由购买交易带出（Writer 接缝已校验），名称仍是物品侧校验；
    // 总成本>0 与日期格式校验在创建路径被带出遮蔽，由修改路径覆盖（update_item_validates_like_create）。
    let tx = seed_purchase_tx(&conn, "2026-01-15", 100, "CNY");
    let mut fired = 0;
    let err = create_item_internal(
        &conn,
        linked_input("  ", "2026-01-15", 100, &tx),
        &mut || fired += 1,
    )
    .unwrap_err();
    assert!(err.to_string().contains("物品名称不能为空"));
    assert_eq!(fired, 0, "校验失败不应发出失效信号");
    assert!(list_items_internal(&conn).unwrap().is_empty());
}

#[test]
fn delete_item_soft_deletes_and_filters_from_list() {
    let conn = conn();
    let old = seed(&conn, "旧手机", "2025-06-01", 300_000);
    seed(&conn, "新手机", "2026-01-15", 599_900);

    let mut fired = 0;
    delete_item_internal(&conn, &old, &mut || fired += 1).unwrap();
    assert_eq!(fired, 1, "删除成功应恰好发出一次失效信号");

    // 标准列表过滤已删除物品
    let items = list_items_internal(&conn).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item.name, "新手机");

    // 软删除语义：行保留在库，仅打 is_deleted=1 标记（version 自增）
    let (is_deleted, version): (i64, i64) = conn
        .query_row(
            "SELECT is_deleted, version FROM items WHERE id=?1",
            rusqlite::params![old],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(is_deleted, 1, "应为软删除标记而非物理移除");
    assert_eq!(version, 2, "删除应自增 version");
}

#[test]
fn delete_item_missing_id_errors_without_signal() {
    let conn = conn();
    let mut fired = 0;
    let err = delete_item_internal(&conn, "no-such-id", &mut || fired += 1).unwrap_err();
    assert!(err.to_string().contains("物品不存在"));
    assert_eq!(fired, 0, "删除失败不应发出失效信号");

    // 已删除的物品再删一次同样报错（入口只对未删除行生效，不重复打标）
    let id = seed(&conn, "水杯", "2026-01-15", 100);
    delete_item_internal(&conn, &id, &mut || {}).unwrap();
    let err = delete_item_internal(&conn, &id, &mut || fired += 1).unwrap_err();
    assert!(err.to_string().contains("物品不存在"));
    assert_eq!(fired, 0);
}

#[test]
fn list_items_excludes_soft_deleted() {
    let conn = conn();
    seed(&conn, "耳机", "2026-03-01", 20_000);
    conn.execute("UPDATE items SET is_deleted=1", rusqlite::params![])
        .unwrap();
    assert!(list_items_internal(&conn).unwrap().is_empty());
}

// —— 处置（issue #120）：处置日期必填、残值可选，写状态字段，版本递增 ——

fn dispose_input(date: &str, residual: Option<i64>) -> ItemDisposeInput {
    ItemDisposeInput {
        disposal_date: date.into(),
        residual_value_cents: residual,
    }
}

#[test]
fn dispose_item_persists_disposal_fields_and_increments_version() {
    let conn = conn();
    let id = seed(&conn, "手机", "2026-01-01", 100_000);
    let mut fired = 0;
    dispose_item_internal(
        &conn,
        &id,
        dispose_input("2026-01-10", Some(20_000)),
        &mut || fired += 1,
    )
    .unwrap();
    assert_eq!(fired, 1, "处置成功应恰好发出一次失效信号");

    let entry = &list_items_internal(&conn).unwrap()[0];
    assert_eq!(entry.item.status, crate::models::ItemStatus::Disposed);
    assert_eq!(entry.item.disposal_date.as_deref(), Some("2026-01-10"));
    assert_eq!(entry.item.residual_value_cents, Some(20_000));
    assert_eq!(entry.item.version, 2, "版本应递增");
    // 每天成本摊到处置日：分子 = 总成本 − 残值，天数含起止两端
    assert_eq!(entry.numerator_cents, 80_000);
    assert_eq!(entry.used_days, 10);
}

#[test]
fn dispose_item_updates_disposal_info_on_redispose() {
    let conn = conn();
    let id = seed(&conn, "相机", "2026-02-01", 50_000);
    dispose_item_internal(
        &conn,
        &id,
        dispose_input("2026-02-10", Some(10_000)),
        &mut || {},
    )
    .unwrap();
    dispose_item_internal(&conn, &id, dispose_input("2026-02-20", Some(0)), &mut || {}).unwrap();
    let item = &list_items_internal(&conn).unwrap()[0].item;
    assert_eq!(item.disposal_date.as_deref(), Some("2026-02-20"));
    assert_eq!(item.residual_value_cents, Some(0));
    assert_eq!(item.version, 3);
}

#[test]
fn dispose_item_rejects_missing_and_soft_deleted() {
    let conn = conn();
    let id = seed(&conn, "耳机", "2026-03-01", 20_000);
    conn.execute("UPDATE items SET is_deleted=1", []).unwrap();
    for id in ["no-such-id", id.as_str()] {
        let mut fired = 0;
        let err = dispose_item_internal(&conn, id, dispose_input("2026-03-05", None), &mut || {
            fired += 1
        })
        .unwrap_err();
        assert!(err.to_string().contains("物品不存在"), "应报不存在: {err}");
        assert_eq!(fired, 0, "失败不应发出失效信号");
    }
}

#[test]
fn dispose_item_validates_date_and_residual() {
    let conn = conn();
    let id = seed(&conn, "手机", "2026-01-01", 100_000);
    for (bad, fragment) in [
        (dispose_input("2026/01/10", None), "日期格式无效"),
        (dispose_input("2025-12-31", None), "早于购买日期"),
        (dispose_input("2099-12-31", None), "不能晚于今天"),
        (dispose_input("2026-01-10", Some(-1)), "残值不能为负"),
    ] {
        let err = dispose_item_internal(&conn, &id, bad, &mut || {}).unwrap_err();
        assert!(
            err.to_string().contains(fragment),
            "应含「{fragment}」: {err}"
        );
    }
    // 校验失败不落库：状态/版本均不动
    let item = &list_items_internal(&conn).unwrap()[0].item;
    assert_eq!(item.status, crate::models::ItemStatus::InUse);
    assert_eq!(item.version, 1);
}

#[test]
fn dispose_item_numerator_floors_at_zero_when_residual_ge_cost() {
    let conn = conn();
    let id = seed(&conn, "水杯", "2026-03-01", 10_000);
    dispose_item_internal(
        &conn,
        &id,
        dispose_input("2026-03-10", Some(99_999)),
        &mut || {},
    )
    .unwrap();
    let entry = &list_items_internal(&conn).unwrap()[0];
    assert_eq!(entry.numerator_cents, 0, "残值 ≥ 成本时分子下限 0");
    assert_eq!(entry.used_days, 10);
    assert_eq!(entry.per_day_cents, 0.0);
}

#[test]
fn dispose_item_rejects_future_disposal_date() {
    let conn = conn();
    let id = seed(&conn, "手机", "2026-01-01", 100_000);
    let err = dispose_item_internal(&conn, &id, dispose_input("2099-12-31", None), &mut || {})
        .unwrap_err();
    assert!(
        err.to_string().contains("不能晚于今天"),
        "应报未来日期: {err}"
    );
    assert_eq!(
        list_items_internal(&conn).unwrap()[0].item.version,
        1,
        "未落库"
    );
}
