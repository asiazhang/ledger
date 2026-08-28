//! `commands::item` 单元测试：内部函数校验语义与失效信号回调（BDD 场景外的快速反馈）。

use rusqlite::Connection;

use crate::commands::item::{create_item_internal, delete_item_internal, list_items_internal};
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
