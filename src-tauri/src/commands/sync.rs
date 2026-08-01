use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::AppError;
use crate::models::SyncProgress;

// 行情接口路径。东财 clist 接口同一数据结构分布在多个主机，按顺序尝试，失败自动切换下一个。
const API_PATH: &str = "/api/qt/clist/get";
// 优先使用延迟行情主机池：push2 实时主机曾被东财对该出口 IP 触发风控（连接重置），
// push2delay 返回相同数据结构且对批量访问更稳定；延迟行情对全量标的同步足够。
const API_HOSTS: &[&str] = &[
    "https://push2delay.eastmoney.com",
    "https://12.push2delay.eastmoney.com",
    "https://21.push2delay.eastmoney.com",
    "https://60.push2delay.eastmoney.com",
    "https://90.push2delay.eastmoney.com",
    "https://push2.eastmoney.com",
];
const PAGE_SIZE: usize = 500;
// 东方财富公开行情接口限频约 60 次/分钟（1 次/秒），此处留更多余量并串行访问。
// 出口 IP 会被 onegate WAF 间歇性限流（返回 200 非 JSON 拦截页或 429），限流窗口约 2-4 分钟自动恢复。
const REQUEST_INTERVAL: Duration = Duration::from_millis(2000);
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const THROTTLE_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_THROTTLE_RETRIES: u32 = 6;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 重试策略：传输层错误走短退避，风控限流（429 / 200 非 JSON）走长冷却等待窗口过去。
#[derive(Clone, Copy)]
struct RetryConfig {
    max_retries: u32,
    base_backoff: Duration,
    max_throttle_retries: u32,
    throttle_cooldown: Duration,
}

impl RetryConfig {
    fn production() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            base_backoff: BASE_BACKOFF,
            max_throttle_retries: MAX_THROTTLE_RETRIES,
            throttle_cooldown: THROTTLE_COOLDOWN,
        }
    }
}

/// 串行限速器：保证相邻两次 HTTP 请求之间至少间隔 interval。
struct Pacer {
    last: Option<Instant>,
    interval: Duration,
}

impl Pacer {
    fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
        }
    }

    fn wait(&mut self) {
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                thread::sleep(self.interval - elapsed);
            }
        }
        self.last = Some(Instant::now());
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new(REQUEST_INTERVAL)
    }
}

struct MarketConfig {
    code: &'static str,
    fs: &'static str,
    name: &'static str,
    currency: &'static str,
}

const MARKETS: &[MarketConfig] = &[
    MarketConfig {
        code: "sh",
        fs: "m:1+t:2,m:1+t:23",
        name: "沪市",
        currency: "CNY",
    },
    MarketConfig {
        code: "sz",
        fs: "m:0+t:6,m:0+t:80",
        name: "深市",
        currency: "CNY",
    },
    MarketConfig {
        code: "hk",
        fs: "m:128+t:3,m:128+t:4",
        name: "港股",
        currency: "HKD",
    },
];

type ExistingInstrument = (String, Option<String>, String);

/// 行情接口返回的单个股票条目（字段 f12=代码, f14=名称, f2=价格原始值）。
/// 注意 f2 的隐含小数位因市场而异：A 股 2 位（f2=951 表示 9.51），港股 3 位（f2=475200 表示 475.200），
/// 因此这里保留原始 f2，换算成分在 `f2_to_cents` 按市场处理。
/// get_total 请求只带 fields=f12，响应条目可能缺 f14/f2，因此名称与价格均可缺省。
#[derive(Debug, Deserialize)]
struct StockItem {
    #[serde(rename = "f12")]
    code: String,
    #[serde(rename = "f14", default)]
    name: String,
    #[serde(rename = "f2", default, deserialize_with = "deserialize_f2")]
    price: Option<f64>,
}

fn deserialize_f2<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    let raw = match value {
        serde_json::Value::Number(n) => n.as_f64(),
        _ => None,
    };
    Ok(raw.filter(|&p| p > 0.0))
}

/// 将原始 f2 换算为整数分：A 股 f2=价格×100（×1 即得分），港股 f2=价格×1000（÷10 得分）。
fn f2_to_cents(raw: f64, market_code: &str) -> i64 {
    if market_code == "hk" {
        (raw / 10.0).round() as i64
    } else {
        raw.round() as i64
    }
}

/// 行情列表接口整体响应。
#[derive(Debug, Deserialize)]
struct ClistResponse {
    data: ClistData,
}

#[derive(Debug, Deserialize)]
struct ClistData {
    total: Option<u64>,
    diff: Option<DiffField>,
}

/// data.diff 东财既可能返回按序号 key 的对象，也可能返回数组。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DiffField {
    Object(HashMap<String, StockItem>),
    Array(Vec<StockItem>),
}

impl DiffField {
    fn into_items(self) -> Vec<StockItem> {
        let mut items: Vec<StockItem> = match self {
            DiffField::Object(map) => {
                let mut pairs: Vec<_> = map.into_iter().collect();
                pairs.sort_by_key(|(k, _)| k.parse::<usize>().unwrap_or(usize::MAX));
                pairs.into_iter().map(|(_, v)| v).collect()
            }
            DiffField::Array(items) => items,
        };
        items.retain(|s| !s.code.is_empty() && !s.name.is_empty());
        items
    }
}

/// 发送请求并解析 JSON，按序尝试多个主机，对传输错误做短退避、对限流拦截做长冷却重试。
fn request_json(
    client: &reqwest::blocking::Client,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
) -> Result<ClistResponse, AppError> {
    request_json_from_hosts(
        client,
        params,
        API_HOSTS,
        RetryConfig::production(),
        pacer,
        ctx,
    )
}

fn request_json_from_hosts(
    client: &reqwest::blocking::Client,
    params: &[(&str, &str)],
    hosts: &[&str],
    cfg: RetryConfig,
    pacer: &mut Pacer,
    ctx: &str,
) -> Result<ClistResponse, AppError> {
    let mut failures: Vec<String> = Vec::new();
    for host in hosts {
        let url = format!("{host}{API_PATH}");
        match request_json_with_retry(client, &url, params, pacer, ctx, cfg) {
            Ok(resp) => return Ok(resp),
            Err(e) => failures.push(format!("{host}: {e}")),
        }
    }
    Err(AppError::Io(format!(
        "全部行情主机请求失败: {}",
        failures.join("; ")
    )))
}

fn request_json_with_retry(
    client: &reqwest::blocking::Client,
    url: &str,
    params: &[(&str, &str)],
    pacer: &mut Pacer,
    ctx: &str,
    cfg: RetryConfig,
) -> Result<ClistResponse, AppError> {
    let mut transport_attempts = 0u32;
    let mut throttle_attempts = 0u32;
    loop {
        pacer.wait();
        let resp = match client
            .get(url)
            .query(params)
            .timeout(REQUEST_TIMEOUT)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                transport_attempts += 1;
                if transport_attempts <= cfg.max_retries {
                    tracing::warn!(ctx = %ctx, attempt = transport_attempts, error = %e, "HTTP 请求失败，准备重试");
                    thread::sleep(cfg.base_backoff * (1u32 << (transport_attempts - 1)));
                    continue;
                }
                tracing::error!(ctx = %ctx, error = %e, "HTTP 请求失败");
                return Err(AppError::Io(format!("HTTP 请求失败: {e}")));
            }
        };

        let status = resp.status();
        let content_encoding = resp
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap_or("?").to_string())
            .unwrap_or_default();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap_or("?").to_string())
            .unwrap_or_default();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            throttle_attempts += 1;
            if throttle_attempts <= cfg.max_throttle_retries {
                tracing::warn!(ctx = %ctx, attempt = throttle_attempts, "触发接口限流(429)，冷却后重试");
                thread::sleep(cfg.throttle_cooldown);
                continue;
            }
            return Err(AppError::Io("接口限流(429)，请稍后再试".into()));
        }

        let bytes = match resp.bytes() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(ctx = %ctx, error = %e, "读取响应失败");
                thread::sleep(cfg.throttle_cooldown);
                continue;
            }
        };
        match serde_json::from_slice::<ClistResponse>(&bytes) {
            Ok(json) => return Ok(json),
            Err(e) => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(120)]);
                throttle_attempts += 1;
                if throttle_attempts <= cfg.max_throttle_retries {
                    tracing::warn!(
                        ctx = %ctx, attempt = throttle_attempts, status = %status,
                        content_type = %content_type, content_encoding = %content_encoding,
                        body_head = %head, error = %e,
                        "响应解析失败（疑似被风控拦截），冷却后重试"
                    );
                    thread::sleep(cfg.throttle_cooldown);
                    continue;
                }
                tracing::error!(
                    ctx = %ctx, status = %status, content_type = %content_type,
                    content_encoding = %content_encoding, body_head = %head, error = %e,
                    "响应解析失败"
                );
                return Err(AppError::Parse(format!("JSON 解析失败: {e}")));
            }
        }
    }
}

fn fetch_page(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    market: &MarketConfig,
    page: usize,
) -> Result<Vec<StockItem>, AppError> {
    tracing::debug!(market = %market.name, page = %page, "获取股票数据页");
    let page_str = page.to_string();
    let size_str = PAGE_SIZE.to_string();
    let params = [
        ("fs", market.fs),
        ("pn", page_str.as_str()),
        ("pz", size_str.as_str()),
        ("fields", "f12,f14,f2"),
    ];
    let resp = request_json(
        client,
        &params,
        pacer,
        &format!("fetch_page:{}({})", market.name, page),
    )?;
    resp.data
        .diff
        .map(DiffField::into_items)
        .ok_or_else(|| AppError::Parse("响应中缺少 data.diff 字段".into()))
}

fn get_total(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    market: &MarketConfig,
) -> Result<usize, AppError> {
    let params = [
        ("fs", market.fs),
        ("pn", "1"),
        ("pz", "1"),
        ("fields", "f12"),
    ];
    let resp = request_json(
        client,
        &params,
        pacer,
        &format!("get_total:{}", market.name),
    )?;

    resp.data
        .total
        .map(|t| t as usize)
        .ok_or_else(|| AppError::Parse("响应中缺少 data.total 字段".into()))
}

fn upsert_market_price(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    price_cents: i64,
    currency: &str,
) -> Result<(), AppError> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(new_uuid);
    let now = now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?7,?8,?9) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![id, instrument_id, price_cents, currency, now, now, now, 1, device_id()],
    )?;
    Ok(())
}

fn build_existing_instruments(
    conn: &rusqlite::Connection,
) -> Result<HashMap<String, ExistingInstrument>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, name, market FROM instruments WHERE instrument_type='stock'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            (
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ),
        ))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (symbol, entry) = row?;
        map.insert(symbol, entry);
    }
    Ok(map)
}

fn do_sync(conn: &rusqlite::Connection, app: &AppHandle) -> Result<(usize, usize), AppError> {
    let mut total_inserted = 0usize;
    let mut total_updated = 0usize;

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut pacer = Pacer::default();

    let mut existing_map = build_existing_instruments(conn)?;

    for market in MARKETS {
        let total = match get_total(&client, &mut pacer, market) {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit(
                    "sync-instruments:progress",
                    SyncProgress {
                        current: 0,
                        total: 0,
                        market: String::new(),
                        done: true,
                        total_inserted: 0,
                        total_updated: 0,
                        error: Some(format!("获取{}总数失败: {e}", market.name)),
                    },
                );
                return Err(e);
            }
        };

        let pages = total.div_ceil(PAGE_SIZE);
        let mut processed = 0usize;

        for page in 1..=pages {
            let items = fetch_page(&client, &mut pacer, market, page)?;
            for item in &items {
                if let Some((existing_id, existing_name, existing_market)) =
                    existing_map.get(&item.code)
                {
                    let name_changed = item.name != existing_name.as_deref().unwrap_or("");
                    let market_changed = market.code != existing_market.as_str();
                    if name_changed || market_changed {
                        let now = now_iso();
                        conn.execute(
                            "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                            params![item.name, market.code, now, existing_id],
                        )?;
                        total_updated += 1;
                    }
                    if let Some(raw) = item.price {
                        let price = f2_to_cents(raw, market.code);
                        upsert_market_price(conn, existing_id, price, market.currency)?;
                    }
                } else {
                    let id = new_uuid();
                    let now = now_iso();
                    conn.execute(
                        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
                         VALUES (?1,?2,'stock',?3,?4,?5,?6,?7,?8,?9)",
                        params![
                            id,
                            item.code,
                            item.name,
                            market.currency,
                            market.code,
                            now,
                            now,
                            1,
                            device_id()
                        ],
                    )?;
                    total_inserted += 1;
                    if let Some(raw) = item.price {
                        let price = f2_to_cents(raw, market.code);
                        upsert_market_price(conn, &id, price, market.currency)?;
                    }
                    existing_map.insert(
                        item.code.clone(),
                        (id.clone(), Some(item.name.clone()), market.code.to_string()),
                    );
                }
                processed += 1;
            }

            let _ = app.emit(
                "sync-instruments:progress",
                SyncProgress {
                    current: processed,
                    total,
                    market: market.code.to_string(),
                    done: false,
                    total_inserted,
                    total_updated,
                    error: None,
                },
            );
        }
    }

    let _ = app.emit(
        "sync-instruments:progress",
        SyncProgress {
            current: 0,
            total: 0,
            market: String::new(),
            done: true,
            total_inserted,
            total_updated,
            error: None,
        },
    );

    Ok((total_inserted, total_updated))
}

#[tauri::command]
pub fn sync_instruments(
    db: tauri::State<'_, crate::db::DbState>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let conn = db.conn.clone();

    thread::spawn(move || {
        let conn_guard = match conn.lock() {
            Ok(g) => g,
            Err(e) => {
                let _ = app.emit(
                    "sync-instruments:progress",
                    SyncProgress {
                        current: 0,
                        total: 0,
                        market: String::new(),
                        done: true,
                        total_inserted: 0,
                        total_updated: 0,
                        error: Some(format!("数据库锁定失败: {e}")),
                    },
                );
                return;
            }
        };

        if let Err(e) = do_sync(&conn_guard, &app) {
            tracing::error!(error = %e, "股票同步失败");
            let _ = app.emit(
                "sync-instruments:progress",
                SyncProgress {
                    current: 0,
                    total: 0,
                    market: String::new(),
                    done: true,
                    total_inserted: 0,
                    total_updated: 0,
                    error: Some(e.to_string()),
                },
            );
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
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
        let json =
            request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(3, 3))
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
        let json =
            request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(3, 3))
                .unwrap();
        assert_eq!(json.data.total, Some(9));
    }

    #[test]
    fn request_json_returns_error_after_429_exhausted() {
        let url = spawn_http_server(|_| (429, "rate limited".into()));
        let client = reqwest::blocking::Client::new();
        let mut pacer = Pacer::new(Duration::ZERO);
        let params = [("fs", "test")];
        let err =
            request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(2, 2))
                .unwrap_err();
        assert!(err.to_string().contains("429"));
    }

    #[test]
    fn request_json_returns_error_when_connection_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/x", listener.local_addr().unwrap());
        drop(listener);
        let client = reqwest::blocking::Client::new();
        let mut pacer = Pacer::new(Duration::ZERO);
        let params = [("fs", "test")];
        let err =
            request_json_with_retry(&client, &url, &params, &mut pacer, "test", fast_cfg(2, 0))
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
        let err =
            request_json_from_hosts(&client, &params, &hosts, fast_cfg(0, 0), &mut pacer, "test")
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
        let json: ClistResponse = serde_json::from_str(
            r#"{"data":{"diff":[{"f12":"600000","f14":"浦发银行","f2":951}]}}"#,
        )
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
            serde_json::from_str(r#"{"data":{"total":2461,"diff":{"0":{"f12":"600000"}}}}"#)
                .unwrap();
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

    fn do_sync_with_items(
        conn: &Connection,
        market_code: &str,
        currency: &str,
        items: &[StockItem],
    ) -> Result<(usize, usize), AppError> {
        let mut total_inserted = 0usize;
        let mut total_updated = 0usize;
        let mut existing_map = build_existing_instruments(conn)?;

        for item in items {
            if let Some((existing_id, existing_name, existing_market)) =
                existing_map.get(&item.code)
            {
                let name_changed = item.name != existing_name.as_deref().unwrap_or("");
                let market_changed = market_code != existing_market.as_str();
                if name_changed || market_changed {
                    let now = now_iso();
                    conn.execute(
                        "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                        params![item.name, market_code, now, existing_id],
                    )?;
                    total_updated += 1;
                }
                if let Some(raw) = item.price {
                    let price = f2_to_cents(raw, market_code);
                    upsert_market_price(conn, existing_id, price, currency)?;
                }
            } else {
                let id = new_uuid();
                let now = now_iso();
                conn.execute(
                    "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
                     VALUES (?1,?2,'stock',?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        id,
                        item.code,
                        item.name,
                        currency,
                        market_code,
                        now,
                        now,
                        1,
                        device_id()
                    ],
                )?;
                total_inserted += 1;
                if let Some(raw) = item.price {
                    let price = f2_to_cents(raw, market_code);
                    upsert_market_price(conn, &id, price, currency)?;
                }
                existing_map.insert(
                    item.code.clone(),
                    (id.clone(), Some(item.name.clone()), market_code.to_string()),
                );
            }
        }

        Ok((total_inserted, total_updated))
    }
}
