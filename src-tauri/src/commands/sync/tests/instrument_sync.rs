//! 全量同步（InstrumentSync，issue #89）：clist 报文解析、f2 报价换算、
//! 标的字典构建与市场行情落库。

use rusqlite::{Connection, params};

use crate::commands::sync::http::{ClistResponse, StockItem, f2_to_cents};
use crate::commands::sync::persist::{apply_stock_item, build_existing_instruments};
use crate::db::{new_uuid, now_iso};
use crate::error::Result;

use super::common::setup_db;

#[test]
fn clist_response_deserializes_object_diff() {
    let json: ClistResponse = serde_json::from_str(
        r#"{"data":{"total":3,"diff":{"0":{"f12":"600000","f14":"浦发银行","f2":951},"1":{"f12":"600001","f14":"邯郸钢铁","f2":0},"2":{"f12":"600002","f14":"齐鲁石化","f2":"-"}}}}"#,
    )
    .unwrap();
    assert_eq!(json.data.total, Some(3));
    let items = json.data.diff.unwrap().into_items();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].code, "600000");
    assert_eq!(items[0].name, "浦发银行");
    assert_eq!(items[0].price, Some(951.0));
    assert_eq!(items[1].price, None);
    assert_eq!(items[2].price, None);
}

#[test]
fn clist_response_deserializes_array_diff() {
    let json: ClistResponse =
        serde_json::from_str(r#"{"data":{"diff":[{"f12":"600000","f14":"浦发银行","f2":951}]}}"#)
            .unwrap();
    let items = json.data.diff.unwrap().into_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].code, "600000");
    assert_eq!(items[0].price, Some(951.0));
}

#[test]
fn f2_to_cents_scales_by_market() {
    assert_eq!(f2_to_cents(951.0, "sh"), 951);
    assert_eq!(f2_to_cents(1700.0, "sz"), 1700);
    assert_eq!(f2_to_cents(475200.0, "hk"), 47520);
    assert_eq!(f2_to_cents(73600.0, "hk"), 7360);
}

#[test]
fn clist_response_deserializes_f12_only_get_total() {
    let json: ClistResponse =
        serde_json::from_str(r#"{"data":{"total":2461,"diff":{"0":{"f12":"600000"}}}}"#).unwrap();
    assert_eq!(json.data.total, Some(2461));
    let items = json.data.diff.unwrap().into_items();
    assert!(items.is_empty());
}

#[test]
fn clist_response_missing_diff_yields_none() {
    let json: ClistResponse = serde_json::from_str(r#"{"data":{"total":5}}"#).unwrap();
    assert_eq!(json.data.total, Some(5));
    assert!(json.data.diff.is_none());
}

#[test]
fn clist_response_ignores_suspension_prices() {
    let json: ClistResponse = serde_json::from_str(
        r#"{"data":{"diff":{"0":{"f12":"600000","f14":"浦发银行","f2":-1},"1":{"f12":"600001","f14":"邯郸钢铁","f2":0}}}}"#,
    )
    .unwrap();
    let items = json.data.diff.unwrap().into_items();
    assert!(items.iter().all(|s| s.price.is_none()));
}

#[test]
fn build_existing_instruments_returns_empty_when_no_stocks() {
    let conn = setup_db();
    let map = build_existing_instruments(&conn).unwrap();
    assert!(map.is_empty());
}

#[test]
fn build_existing_instruments_returns_stock_symbols() {
    let conn = setup_db();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'600000','stock','浦发银行','CNY','sh',?2,?2,1,'test')",
        params![id, now],
    )
    .unwrap();
    let map = build_existing_instruments(&conn).unwrap();
    assert_eq!(map.len(), 1);
    let (eid, ename, emarket) = map.get("600000").unwrap();
    assert_eq!(eid, &id);
    assert_eq!(ename.as_deref(), Some("浦发银行"));
    assert_eq!(emarket, "sh");
}

#[test]
fn do_sync_inserts_new_instruments_and_prices() {
    let conn = setup_db();

    let items = vec![
        StockItem {
            code: "000001".into(),
            name: "平安银行".into(),
            price: Some(1234.0),
        },
        StockItem {
            code: "000002".into(),
            name: "万科A".into(),
            price: Some(1500.0),
        },
    ];

    let (inserted, updated) = do_sync_with_items(&conn, "sh", "CNY", &items).unwrap();
    assert_eq!(inserted, 2);
    assert_eq!(updated, 0);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);

    let (symbol, name, market): (String, Option<String>, String) = conn
        .query_row(
            "SELECT symbol, name, market FROM instruments WHERE symbol='000001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(symbol, "000001");
    assert_eq!(name.as_deref(), Some("平安银行"));
    assert_eq!(market, "sh");

    // 同步新建行来源标记为同步（issue #293 / ADR-0036 决策 2）。
    let source: String = conn
        .query_row(
            "SELECT source FROM instruments WHERE symbol='000001'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, "eastmoney");

    let price_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(price_count, 2);
}

#[test]
fn do_sync_updates_existing_instrument_name_and_market() {
    let conn = setup_db();

    let existing = vec![StockItem {
        code: "000001".into(),
        name: "旧名称".into(),
        price: Some(500.0),
    }];
    do_sync_with_items(&conn, "sh", "CNY", &existing).unwrap();

    let updated = vec![StockItem {
        code: "000001".into(),
        name: "平安银行".into(),
        price: Some(1234.0),
    }];
    let (inserted, u) = do_sync_with_items(&conn, "sz", "CNY", &updated).unwrap();
    assert_eq!(inserted, 0);
    assert_eq!(u, 1);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let (name, market): (Option<String>, String) = conn
        .query_row(
            "SELECT name, market FROM instruments WHERE symbol='000001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("平安银行"));
    assert_eq!(market, "sz");
}

/// 同步更新分支不覆盖来源（issue #293「来源随行终身不变」）：既有行若是
/// 手动/AI 通道所建的同码 stock 行（ADR-0037 决策 4，来源 'manual'），
/// 同步按代码命中后只更新名称/市场，来源保持不变。
#[test]
fn do_sync_update_keeps_existing_source() {
    let conn = setup_db();

    // 手动/AI 通道所建的同码 stock 行（经同步 upsert 复用的合法并存态）。
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES ('inst-manual','000001','stock','某某科技','CNY','unknown',?1,?1,1,'test','manual')",
        params![now],
    )
    .unwrap();

    let updated = vec![StockItem {
        code: "000001".into(),
        name: "平安银行".into(),
        price: Some(1234.0),
    }];
    let (inserted, updated_count) = do_sync_with_items(&conn, "sz", "CNY", &updated).unwrap();
    assert_eq!((inserted, updated_count), (0, 1));

    let (source, name, market): (String, Option<String>, String) = conn
        .query_row(
            "SELECT source, name, market FROM instruments WHERE symbol='000001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(source, "manual", "同步更新不应覆盖来源");
    assert_eq!(name.as_deref(), Some("平安银行"));
    assert_eq!(market, "sz");
}

#[test]
fn do_sync_skips_zero_price() {
    let conn = setup_db();

    let items = vec![StockItem {
        code: "000001".into(),
        name: "平安银行".into(),
        price: None,
    }];
    let (inserted, _) = do_sync_with_items(&conn, "sh", "CNY", &items).unwrap();
    assert_eq!(inserted, 1);

    let price_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(price_count, 0);
}

#[test]
fn do_sync_updates_market_price_on_existing_instrument() {
    let conn = setup_db();

    let first = vec![StockItem {
        code: "000001".into(),
        name: "平安银行".into(),
        price: Some(1000.0),
    }];
    do_sync_with_items(&conn, "sh", "CNY", &first).unwrap();

    let second = vec![StockItem {
        code: "000001".into(),
        name: "平安银行".into(),
        price: Some(2000.0),
    }];
    do_sync_with_items(&conn, "sh", "CNY", &second).unwrap();

    let (price_cents,): (i64,) = conn
        .query_row("SELECT price_cents FROM market_prices", [], |r| {
            Ok((r.get(0)?,))
        })
        .unwrap();
    assert_eq!(price_cents, 2000);
}

/// 模拟单个市场的同步落库流程（复用持久化逻辑），返回 (新增数, 更新数)。
fn do_sync_with_items(
    conn: &Connection,
    market_code: &str,
    currency: &str,
    items: &[StockItem],
) -> Result<(usize, usize)> {
    let mut total_inserted = 0usize;
    let mut total_updated = 0usize;
    let mut existing_map = build_existing_instruments(conn)?;

    for item in items {
        let (inserted, updated) =
            apply_stock_item(conn, item, market_code, currency, &mut existing_map)?;
        total_inserted += inserted;
        total_updated += updated;
    }

    Ok((total_inserted, total_updated))
}
