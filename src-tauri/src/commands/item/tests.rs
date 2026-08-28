//! `commands::item` 单元测试：内部函数校验语义与失效信号回调（BDD 场景外的快速反馈）。

use rusqlite::Connection;

use crate::commands::item::{
    create_item_internal, delete_item_internal, list_items_internal, update_item_internal,
};
use crate::db::{init_db, open_in_memory};
use crate::models::ItemInput;

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
    }
}

/// 创建一件物品并返回其 id（默认不监听信号）。
fn seed(conn: &Connection, name: &str, date: &str, cost_cents: i64) -> String {
    create_item_internal(conn, input(name, date, cost_cents), &mut || {}).unwrap()
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
    let mut inp = input("水杯", "2026-01-15", 5_000);
    inp.note = Some("Starbucks 联名".into());
    let id = create_item_internal(&conn, inp, &mut || {}).unwrap();
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
fn create_item_persists_and_returns_id() {
    let conn = conn();
    let mut fired = 0;
    let id = create_item_internal(&conn, input("iPhone", "2026-01-15", 599_900), &mut || {
        fired += 1
    })
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
    create_item_internal(
        &conn,
        input("显示器", &purchase.format("%Y-%m-%d").to_string(), 100_000),
        &mut || {},
    )
    .unwrap();
    let items = list_items_internal(&conn).unwrap();
    assert_eq!(items[0].used_days, 10);
    assert!((items[0].per_day_cents - 10_000.0).abs() < 1e-9);
}

#[test]
fn create_item_rejects_blank_name_and_nonpositive_cost() {
    let conn = conn();
    let mut fired = 0;
    let err = create_item_internal(&conn, input("  ", "2026-01-15", 100), &mut || fired += 1)
        .unwrap_err();
    assert!(err.to_string().contains("物品名称不能为空"));
    let err = create_item_internal(&conn, input("水杯", "2026-01-15", 0), &mut || fired += 1)
        .unwrap_err();
    assert!(err.to_string().contains("物品总成本必须大于 0"));
    assert_eq!(fired, 0, "校验失败不应发出失效信号");
    assert!(list_items_internal(&conn).unwrap().is_empty());
}

#[test]
fn create_item_rejects_bad_date() {
    let conn = conn();
    let err =
        create_item_internal(&conn, input("水杯", "2026/01/15", 100), &mut || {}).unwrap_err();
    assert!(err.to_string().contains("日期格式无效"));
}

#[test]
fn delete_item_soft_deletes_and_filters_from_list() {
    let conn = conn();
    let old =
        create_item_internal(&conn, input("旧手机", "2025-06-01", 300_000), &mut || {}).unwrap();
    create_item_internal(&conn, input("新手机", "2026-01-15", 599_900), &mut || {}).unwrap();

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
    let id = create_item_internal(&conn, input("水杯", "2026-01-15", 100), &mut || {}).unwrap();
    delete_item_internal(&conn, &id, &mut || {}).unwrap();
    let err = delete_item_internal(&conn, &id, &mut || fired += 1).unwrap_err();
    assert!(err.to_string().contains("物品不存在"));
    assert_eq!(fired, 0);
}

#[test]
fn create_item_rejects_currency_without_rate() {
    let conn = conn();
    let mut inp = input("转接头", "2026-02-01", 10_000);
    inp.currency_code = "XYZ".into();
    let err = create_item_internal(&conn, inp, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("汇率"));
}

#[test]
fn list_items_excludes_soft_deleted() {
    let conn = conn();
    create_item_internal(&conn, input("耳机", "2026-03-01", 20_000), &mut || {}).unwrap();
    conn.execute("UPDATE items SET is_deleted=1", rusqlite::params![])
        .unwrap();
    assert!(list_items_internal(&conn).unwrap().is_empty());
}
