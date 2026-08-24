//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算、持久化与编排行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。

use std::time::Duration;

use rusqlite::Connection;
use rusqlite::params;

use super::http::{
    ClistResponse, Pacer, RetryConfig, StockItem, f2_to_cents, request_json_from_hosts,
    request_json_with_retry,
};
use super::persist::{apply_stock_item, build_existing_instruments};
use crate::db::{init_db, new_uuid, now_iso, open_in_memory};
use crate::error::Result;

fn setup_db() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}

fn fast_cfg(max_retries: u32, max_throttle_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        base_backoff: Duration::from_millis(1),
        max_throttle_retries,
        throttle_cooldown: Duration::from_millis(1),
    }
}

/// 起一个本地 HTTP 服务，按调用次数回调响应 (status, body)，返回基础地址。
fn spawn_http_server(responder: impl Fn(usize) -> (u16, String) + Send + 'static) -> String {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let mut seq = 0usize;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            seq += 1;
            let (status, body) = responder(seq);
            let reason = if status == 200 { "OK" } else { "Limited" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

#[test]
fn request_json_retries_429_then_succeeds() {
    let url = spawn_http_server(|n| {
        if n == 1 {
            (429, "rate limited".into())
        } else {
            (200, r#"{"data":{"total":7}}"#.into())
        }
    });
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test"), ("pn", "1")];
    let json = request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(3, 3))
        .unwrap();
    assert_eq!(json.data.total, Some(7));
}

#[test]
fn request_json_retries_on_json_decode_failure() {
    let url = spawn_http_server(|n| {
        if n == 1 {
            (200, "not json at all".into())
        } else {
            (200, r#"{"data":{"total":9}}"#.into())
        }
    });
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let json = request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(3, 3))
        .unwrap();
    assert_eq!(json.data.total, Some(9));
}

#[test]
fn request_json_returns_error_after_429_exhausted() {
    let url = spawn_http_server(|_| (429, "rate limited".into()));
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(2, 2))
        .unwrap_err();
    assert!(err.to_string().contains("429"));
}

#[test]
fn request_json_returns_error_when_connection_refused() {
    // 显式禁用系统代理：默认 Client 会读取系统代理（如 Clash/Surge 监听 127.0.0.1），
    // 代理转发到无监听的端口时会返回空 body 响应，导致“连接被拒绝”语义失效。
    // 目标用保留端口 1，本机几乎不可能有服务监听，可稳定触发 ECONNREFUSED。
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let url = "http://127.0.0.1:1/x".to_string();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(2, 0))
        .unwrap_err();
    assert!(err.to_string().contains("HTTP 请求失败"));
}

#[test]
fn request_json_falls_back_to_next_host() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let hits = Arc::new(AtomicUsize::new(0));
    let h1 = hits.clone();
    let url1 = spawn_http_server(move |_| {
        h1.fetch_add(1, Ordering::SeqCst);
        (500, "boom".into())
    });
    let h2 = hits.clone();
    let url2 = spawn_http_server(move |_| {
        h2.fetch_add(1, Ordering::SeqCst);
        (200, r#"{"data":{"total":7}}"#.into())
    });

    let hosts = [url1.as_str(), url2.as_str()];
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let resp =
        request_json_from_hosts(&client, &params, &hosts, fast_cfg(0, 0), &mut pacer, "test")
            .unwrap();
    assert_eq!(resp.data.total, Some(7));
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn request_json_returns_error_when_all_hosts_fail() {
    let url = spawn_http_server(|_| (500, "boom".into()));
    let hosts = [url.as_str()];
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_from_hosts(&client, &params, &hosts, fast_cfg(0, 0), &mut pacer, "test")
        .unwrap_err();
    assert!(err.to_string().contains("全部行情主机请求失败"));
}

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
