use std::collections::HashMap;
use std::thread;

use rusqlite::params;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::AppError;
use crate::models::SyncProgress;

const API_BASE: &str = "https://push2.eastmoney.com/api/qt/clist/get";
const PAGE_SIZE: usize = 500;

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

struct StockItem {
    code: String,
    name: String,
    price: Option<i64>,
}

fn fetch_page(market: &MarketConfig, page: usize) -> Result<Vec<StockItem>, AppError> {
    tracing::debug!(market = %market.name, page = %page, "获取股票数据页");
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))?;

    let url = format!(
        "{}?fs={}&pn={}&pz={}&fields=f12,f14,f2",
        API_BASE, market.fs, page, PAGE_SIZE
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| {
            tracing::error!(market = %market.name, page = %page, error = %e, "HTTP 请求失败");
            AppError::Io(format!("HTTP 请求失败: {e}"))
        })?;

    let json: Value = resp
        .json()
        .map_err(|e| {
            tracing::warn!(market = %market.name, page = %page, error = %e, "JSON 解析失败");
            AppError::Parse(format!("JSON 解析失败: {e}"))
        })?;

    let diff = json["data"]["diff"]
        .as_array()
        .ok_or_else(|| AppError::Parse("响应中缺少 data.diff 字段".into()))?;

    let items: Vec<StockItem> = diff
        .iter()
        .filter_map(|item| {
            let code = item["f12"].as_str()?;
            let name = item["f14"].as_str()?;
            if code.is_empty() || name.is_empty() {
                return None;
            }
            let price = match &item["f2"] {
                Value::Number(n) => n.as_f64().or(n.as_i64().map(|i| i as f64)),
                Value::String(s) if s == "-" => None,
                _ => None,
            };
            let price_cents = price.map(|p| (p * 100.0).round() as i64);
            Some(StockItem {
                code: code.to_string(),
                name: name.to_string(),
                price: price_cents.filter(|&c| c > 0),
            })
        })
        .collect();

    Ok(items)
}

fn get_total(market: &MarketConfig) -> Result<usize, AppError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Io(e.to_string()))?;

    let url = format!("{}?fs={}&pn=1&pz=1&fields=f12", API_BASE, market.fs);

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| AppError::Io(format!("HTTP 请求失败: {e}")))?;

    let json: Value = resp
        .json()
        .map_err(|e| AppError::Parse(format!("JSON 解析失败: {e}")))?;

    json["data"]["total"]
        .as_u64()
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

    let mut existing_map = build_existing_instruments(conn)?;

    for market in MARKETS {
        let total = match get_total(market) {
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
            let items = fetch_page(market, page)?;
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
                    if let Some(price) = item.price {
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
                    if let Some(price) = item.price {
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
                price: Some(1234),
            },
            StockItem {
                code: "000002".into(),
                name: "万科A".into(),
                price: Some(1500),
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
            price: Some(500),
        }];
        do_sync_with_items(&conn, "sh", "CNY", &existing).unwrap();

        let updated = vec![StockItem {
            code: "000001".into(),
            name: "平安银行".into(),
            price: Some(1234),
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
            price: Some(1000),
        }];
        do_sync_with_items(&conn, "sh", "CNY", &first).unwrap();

        let second = vec![StockItem {
            code: "000001".into(),
            name: "平安银行".into(),
            price: Some(2000),
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
                if let Some(price) = item.price {
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
                if let Some(price) = item.price {
                    upsert_market_price(conn, &id, price, currency)?;
                }
                existing_map.insert(
                    item.code.clone(),
                    (
                        id.clone(),
                        Some(item.name.clone()),
                        market_code.to_string(),
                    ),
                );
            }
        }

        Ok((total_inserted, total_updated))
    }
}
