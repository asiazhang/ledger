//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算、持久化与编排行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。

use std::time::Duration;

use rusqlite::Connection;
use rusqlite::params;

use super::http::{
    ClistResponse, Pacer, RetryConfig, StockItem, ULIST_BATCH_SIZE, UlistResponse, f2_to_cents,
    request_json_from_hosts, request_json_with_retry, secid_prefix,
};
use super::incremental::do_incremental_sync_with;
use super::persist::{apply_stock_item, build_existing_instruments, upsert_market_price};
use crate::db::{init_db, new_uuid, now_iso, open_in_memory};
use crate::error::{AppError, Result};

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
    let json = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
    )
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
    let json = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(3, 3),
    )
    .unwrap();
    assert_eq!(json.data.total, Some(9));
}

#[test]
fn request_json_returns_error_after_429_exhausted() {
    let url = spawn_http_server(|_| (429, "rate limited".into()));
    let client = reqwest::blocking::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let params = [("fs", "test")];
    let err = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 2),
    )
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
    let err = request_json_with_retry::<ClistResponse>(
        &client,
        &url,
        &params,
        &mut pacer,
        "test",
        fast_cfg(2, 0),
    )
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
    let resp = request_json_from_hosts::<ClistResponse>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
    )
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
    let err = request_json_from_hosts::<ClistResponse>(
        &client,
        &params,
        "/x",
        &hosts,
        fast_cfg(0, 0),
        &mut pacer,
        "test",
    )
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

// ---------------------------------------------------------------------------
// 持仓价格增量同步（issue #103）：secid 构造、ulist 响应解析、编排、跳过规则、
// 结果统计与幂等。编排经注入 mock 查询函数驱动，不依赖真实网络。
// ---------------------------------------------------------------------------

/// 直插一条持仓（账户 + 标的 + 交易 + 批次），绕过交易行为层以聚焦增量同步自身逻辑。
fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'investment',?3,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, format!("账户-{id}"), currency],
    )
    .unwrap();
}

fn insert_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, kind, format!("名称-{symbol}"), currency, market],
    )
    .unwrap();
}

fn insert_lot(conn: &Connection, account_id: &str, instrument_id: &str, currency: &str) {
    let txn_id = format!("txn-{account_id}-{instrument_id}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,?2,1000,?3,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, currency, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,100,?5,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{account_id}-{instrument_id}"),
            account_id,
            instrument_id,
            txn_id,
            currency
        ],
    )
    .unwrap();
}

/// 组合帮手：账户 + 标的 + 持仓批次一步建好。
fn insert_holding(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    insert_account(conn, account_id, currency);
    insert_instrument(conn, instrument_id, symbol, kind, currency, market);
    insert_lot(conn, account_id, instrument_id, currency);
}

fn market_price_of(conn: &Connection, instrument_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT price_cents FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .ok()
}

/// 模拟批量报价：对每个查询的 secid 生成条目。`prices` 为 code → 原始 f2
/// （None 表示停牌/无效价），不在映射中的代码不返回（模拟查询无果）。
fn mock_fetch<'a>(
    prices: &'a [(&'a str, Option<f64>)],
) -> impl FnMut(&str) -> Result<Vec<StockItem>> + 'a {
    move |secids: &str| {
        let mut items = Vec::new();
        for secid in secids.split(',') {
            let code = secid.split('.').nth(1).unwrap_or(secid).to_string();
            if let Some((_, price)) = prices.iter().find(|(c, _)| *c == code) {
                items.push(StockItem {
                    name: format!("名称-{code}"),
                    code,
                    price: *price,
                });
            }
        }
        Ok(items)
    }
}

#[test]
fn secid_prefix_maps_known_markets() {
    assert_eq!(secid_prefix("sh"), Some("1"));
    assert_eq!(secid_prefix("sz"), Some("0"));
    assert_eq!(secid_prefix("hk"), Some("116"));
    assert_eq!(secid_prefix("unknown"), None);
}

#[test]
fn ulist_response_deserializes_cross_market_codes() {
    // 真实 ulist.np/get 响应样本（一次携带跨市场：沪 1.600519 / 深 0.000001 / 港 116.00700）
    let json = r#"{"rc":0,"rt":11,"svr":177542529,"lt":1,"full":1,"dlmkts":"8,10,128","dsc":"0","data":{"total":3,"diff":[{"f2":130280,"f12":"600519","f14":"贵州茅台"},{"f2":1173,"f12":"000001","f14":"平安银行"},{"f2":445400,"f12":"00700","f14":"腾讯控股"}]}}"#;
    let resp: UlistResponse = serde_json::from_str(json).unwrap();
    let items = resp.data.unwrap().diff.unwrap().into_items();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].code, "600519");
    // 价格换算：A 股 f2 直接得分、港股 ÷10（与全量同步一致）
    assert_eq!(f2_to_cents(items[0].price.unwrap(), "sh"), 130280);
    assert_eq!(f2_to_cents(items[1].price.unwrap(), "sz"), 1173);
    assert_eq!(f2_to_cents(items[2].price.unwrap(), "hk"), 44540);
}

#[test]
fn ulist_response_null_data_yields_no_items() {
    // 全部代码无效时东财返回 rc=102 且 data:null：应解析为空而非报错（不中断同步）。
    let json = r#"{"rc":102,"rt":1,"svr":177622402,"lt":1,"full":1,"dlmkts":"8,10,128","dsc":"0","data":null}"#;
    let resp: UlistResponse = serde_json::from_str(json).unwrap();
    assert!(resp.data.is_none());
}

#[test]
fn incremental_sync_normalizes_symbol_suffix() {
    let conn = setup_db();
    // schema 注释示例格式：symbol 带市场后缀（"600519.SH"），secid 应取裸代码 "1.600519"。
    insert_holding(&conn, "acc-1", "inst-sh", "600519.SH", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-hk", "00700.HK", "stock", "HKD", "hk");

    // mock 按响应侧裸代码（f12）返回：归一化后应能匹配并写入价格。
    let prices = [("600519", Some(130280.0)), ("00700", Some(445400.0))];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 2);
    assert_eq!(result.skipped, 0);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(130280));
    assert_eq!(market_price_of(&conn, "inst-hk"), Some(44540));
}

#[test]
fn incremental_sync_all_missing_response_counts_all_skipped() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    // 查询全部无果（如整批代码无效、响应 data:null）：不报错、全部计入跳过。
    let prices: [(&str, Option<f64>); 0] = [];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(market_price_of(&conn, "inst-a"), None);
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_no_holdings_returns_message() {
    let conn = setup_db();
    let mut fetch = mock_fetch(&[]);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "无持仓标的可同步");
}

#[test]
fn incremental_sync_updates_holding_prices_only() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    insert_holding(&conn, "acc-3", "inst-hk", "00700", "stock", "HKD", "hk");
    // 预先存在的旧价（应被覆盖更新，不产生新行）
    upsert_market_price(&conn, "inst-sh", 999, "CNY").unwrap();

    let prices = [
        ("600519", Some(130280.0)),
        ("000001", Some(1173.0)),
        ("00700", Some(445400.0)),
    ];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 3);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "已同步 3 只，跳过 0 只");

    // 价格覆盖更新：A 股直接得分、港股 ÷10
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(130280));
    assert_eq!(market_price_of(&conn, "inst-sz"), Some(1173));
    assert_eq!(market_price_of(&conn, "inst-hk"), Some(44540));
    // 每标的一条价格（无重复）
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    // 标的字典（名称/市场）不变
    let (name, market): (Option<String>, String) = conn
        .query_row(
            "SELECT name, market FROM instruments WHERE id='inst-sh'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("名称-600519"));
    assert_eq!(market, "sh");
    // 未新增标的
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn incremental_sync_skips_non_stock_holdings() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-fund", "110011", "fund", "CNY", "sh");
    insert_holding(&conn, "acc-3", "inst-bond", "019547", "bond", "CNY", "sh");

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 2, "基金/债券持仓应计入跳过统计");
    assert_eq!(result.message, "已同步 1 只，跳过 2 只");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(130280));
    assert_eq!(
        market_price_of(&conn, "inst-fund"),
        None,
        "非股票持仓不写价格"
    );
    assert_eq!(market_price_of(&conn, "inst-bond"), None);
}

#[test]
fn incremental_sync_keeps_old_price_when_suspended() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    // 停牌股已有旧价
    upsert_market_price(&conn, "inst-sz", 888, "CNY").unwrap();

    // 600519 正常价；000001 停牌（f2 无效 → None）
    let prices = [("600519", Some(130280.0)), ("000001", None)];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "停牌/无效价应计入跳过且不中断同步");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(130280));
    assert_eq!(
        market_price_of(&conn, "inst-sz"),
        Some(888),
        "停牌应保留旧价"
    );
}

#[test]
fn incremental_sync_counts_missing_response_as_skipped() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    // mock 只返回 600001：600002 查询无果（响应缺失）→ 计入跳过
    let prices = [("600001", Some(1000.0))];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(market_price_of(&conn, "inst-a"), Some(1000));
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_skips_unknown_market() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-ok", "600519", "stock", "CNY", "sh");
    // 市场未知的持仓股票（如手动创建未设市场）：无法构造 secid，计入跳过
    insert_holding(
        &conn, "acc-2", "inst-unk", "NVDA", "stock", "USD", "unknown",
    );

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "市场未知应计入跳过");
    assert_eq!(market_price_of(&conn, "inst-unk"), None);
}

#[test]
fn incremental_sync_is_idempotent() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let prices = [("600519", Some(130280.0))];
    let mut fetch = mock_fetch(&prices);
    let first = do_incremental_sync_with(&conn, &mut fetch).unwrap();
    assert_eq!(first.synced, 1);

    let mut fetch = mock_fetch(&prices);
    let second = do_incremental_sync_with(&conn, &mut fetch).unwrap();
    assert_eq!(second.synced, 1);

    // 重复调用不产生重复价格行（每标的一条，覆盖更新）
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(130280));
}

#[test]
fn incremental_sync_dedupes_same_instrument_across_accounts() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    // 同一标的在另一账户也有持仓：应去重为一只、只查一次
    insert_account(&conn, "acc-2", "CNY");
    insert_lot(&conn, "acc-2", "inst-sh", "CNY");

    let prices = [("600519", Some(1000.0))];
    let mut fetch = mock_fetch(&prices);
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(1000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn incremental_sync_batches_by_fifty() {
    let conn = setup_db();
    // 55 只股票：应拆为 2 批（50 + 5），每批 secid 数不超 ULIST_BATCH_SIZE
    for i in 0..55 {
        let symbol = format!("{:06}", 600000 + i);
        insert_holding(
            &conn,
            &format!("acc-{i}"),
            &format!("inst-{i}"),
            &symbol,
            "stock",
            "CNY",
            "sh",
        );
    }

    let mut batch_sizes: Vec<usize> = Vec::new();
    let mut fetch = |secids: &str| {
        let codes: Vec<&str> = secids.split(',').collect();
        assert!(codes.len() <= ULIST_BATCH_SIZE);
        batch_sizes.push(codes.len());
        Ok(codes
            .iter()
            .map(|secid| {
                let code = secid.split('.').nth(1).unwrap().to_string();
                StockItem {
                    code,
                    name: "名称".into(),
                    price: Some(1000.0),
                }
            })
            .collect())
    };
    let result = do_incremental_sync_with(&conn, &mut fetch).unwrap();

    assert_eq!(result.synced, 55);
    assert_eq!(batch_sizes, vec![50, 5]);
}

#[test]
fn incremental_sync_propagates_fetch_error() {
    let conn = setup_db();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let mut fetch = |_: &str| Err(AppError::Io("模拟网络失败".into()));
    let err = do_incremental_sync_with(&conn, &mut fetch).unwrap_err();
    assert!(err.to_string().contains("模拟网络失败"));
}
