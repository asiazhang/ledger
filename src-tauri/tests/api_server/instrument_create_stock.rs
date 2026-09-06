//! 标的创建端点的 stock 增强（`POST /api/v1/instruments`，issue #694 / ADR-0081 决策 2）。
//!
//! 只断言外部行为：stock + 可解析真实代码经东财校验——命中回填权威名称并落最新
//! 价现价（万分之一元刻度）、查无此码 400 拒绝且不产生标的行、网络不可达降级为
//! 提交名称 + 真实代码 + 解析市场建行（**市场保留**——股票行情通道只依赖市场+代码，
//! 降级行价格同步仍可达）；北交所代码与真实代码形态的 market 矛盾显式 400 不建行；
//! 名称充代码兜底与美股 ticker（本接缝未开放）不发起网络请求；降级重放不覆盖既有
//! 权威名称；幂等重放返回同一 id。东财访问经注入桩离线驱动。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::StatusCode;
use rusqlite::params;

use tauri_app_lib::api_server::StockQuoteFetcher;
use tauri_app_lib::error::AppError;
use tauri_app_lib::investment::{InstrumentType, StockQuote};

use crate::common::{StockStubHit, get_json, post_instrument, setup_app_with_stock_stub};

/// stock 标的行与现价行的断言投影：name / market / source 与可选现价
/// (price_cents, priced_at 非空, nav_date, source)。
struct StockRow {
    name: Option<String>,
    market: String,
    source: String,
    price: Option<(i64, bool, Option<String>, Option<String>)>,
}

/// 查询 stock 标的行（name, market, source）与现价行。
fn stock_row(conn: &Arc<Mutex<rusqlite::Connection>>, symbol: &str) -> StockRow {
    let conn = conn.lock().unwrap();
    let (name, market, source) = conn
        .query_row(
            "SELECT name, market, source FROM instruments WHERE symbol=?1 AND instrument_type='stock'",
            params![symbol],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap_or_else(|_| panic!("stock 标的行应存在: {symbol}"));
    let price = conn
        .query_row(
            "SELECT p.price_cents, p.priced_at IS NOT NULL, p.nav_date, p.source \
             FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1 AND i.instrument_type='stock'",
            params![symbol],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .ok();
    StockRow {
        name,
        market,
        source,
        price,
    }
}

/// 命中表：sh/600519 → 贵州茅台 / 最新价 15.00 元（万分之一元 150000）。
fn stub_hit() -> HashMap<String, StockStubHit> {
    HashMap::from([(
        "sh/600519".to_string(),
        StockStubHit {
            name: "贵州茅台",
            price: Some((150000, "2026-09-04")),
            kind_hint: InstrumentType::Stock,
        },
    )])
}

// ---------------------------------------------------------------------------
// 东财命中：权威名称回填 + 最新价落现价
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_stock_with_known_code_backfills_authoritative_name_and_price() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    // AI 提交的名称有误（账单抄写名），后端应以东财权威名称为准。
    let body = r#"{"symbol":"600519","type":"stock","market":"sh","name":"贵州茅台A（账单抄写）"}"#;
    let (status, bytes) = post_instrument(&app, body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");
    assert!(!id.is_empty());

    let row = stock_row(&conn, "600519");
    assert_eq!(
        row.name.as_deref(),
        Some("贵州茅台"),
        "东财可达时应回填权威名称，而非 AI 抄写名"
    );
    assert_eq!(row.market, "sh", "应落解析市场");
    assert_eq!(row.source, "manual");
    let (price_cents, priced, nav_date, price_source) = row.price.expect("东财命中应落现价缓存");
    assert_eq!(
        price_cents, 150000,
        "最新价 15.00 元 = 万分之一元刻度 150000"
    );
    assert!(priced, "priced_at = 写入时刻（同步通道同口径）");
    assert_eq!(nav_date, None, "nav_date 是场外基金语义，股票恒 None");
    assert_eq!(price_source.as_deref(), Some("eastmoney"));
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sh".to_string(), "600519".to_string())]
    );
}

#[tokio::test]
async fn test_create_stock_without_market_infers_and_creates_with_resolved_market() {
    let mut hits = stub_hit();
    hits.insert(
        "sz/000001".to_string(),
        StockStubHit {
            name: "平安银行",
            price: Some((115600, "2026-09-04")),
            kind_hint: InstrumentType::Stock,
        },
    );
    let (app, conn, calls) = setup_app_with_stock_stub(hits);

    // 缺省 market：按代码形态推断深市，创建落精确市场与现价。
    let (status, bytes) = post_instrument(&app, r#"{"symbol":"000001","type":"stock"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = stock_row(&conn, "000001");
    assert_eq!(row.name.as_deref(), Some("平安银行"), "应回填权威名称");
    assert_eq!(row.market, "sz", "缺省 market 应按形态推断并落精确市场");
    let (price_cents, ..) = row.price.expect("应落现价");
    assert_eq!(price_cents, 115600);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sz".to_string(), "000001".to_string())],
        "推断市场应作为行情查询键"
    );
}

#[tokio::test]
async fn test_create_etf_typed_instrument_gets_same_enhancement() {
    let mut hits = stub_hit();
    hits.insert(
        "sh/510300".to_string(),
        StockStubHit {
            name: "沪深300ETF",
            price: Some((398500, "2026-09-04")),
            kind_hint: InstrumentType::Etf,
        },
    );
    let (app, conn, calls) = setup_app_with_stock_stub(hits);

    // 导入知识教 AI 按类型提示填 type：etf 提交同样走增强（场内两类型同属行情
    // 通道），回填权威名称 + 落现价；类型以提交为准落库，不被改写。
    let (status, bytes) = post_instrument(&app, r#"{"symbol":"510300","type":"etf"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let conn = conn.lock().unwrap();
    let (name, row_type, price_cents): (String, String, i64) = conn
        .query_row(
            "SELECT i.name, i.instrument_type, p.price_cents FROM instruments i \
             JOIN market_prices p ON p.instrument_id = i.id \
             WHERE i.symbol='510300' AND i.instrument_type='etf'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("etf 行 + 现价应存在");
    assert_eq!(name, "沪深300ETF", "应回填东财权威名称");
    assert_eq!(row_type, "etf", "类型按提交落库");
    assert_eq!(price_cents, 398500, "etf 命中同样落最新价现价");
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sh".to_string(), "510300".to_string())]
    );
}

// ---------------------------------------------------------------------------
// 查无此码：拒绝创建，不产生标的行
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_stock_with_unknown_code_rejects_without_row() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"600999","type":"stock","market":"sh","name":"不存在的股票"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(
        err["code"], "sync.stock-not-found",
        "查无此码应复用查询端点同一码化错误"
    );
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("查无股票代码 600999"),
        "查无此码应显式报错，实际: {err}"
    );

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "查无此码不应产生标的行");
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sh".to_string(), "600999".to_string())]
    );
}

// ---------------------------------------------------------------------------
// 网络不可达：降级为提交名称 + 真实代码 + 解析市场建行（市场保留、价格通道可达）
// ---------------------------------------------------------------------------

/// 状态开关桩：`down=true` 模拟东财网络不可达（Io），否则按命中表返回。
fn toggle_stub(
    hits: HashMap<String, StockStubHit>,
    down: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
) -> StockQuoteFetcher {
    Arc::new(move |market: &str, code: &str| {
        calls
            .lock()
            .unwrap()
            .push((market.to_string(), code.to_string()));
        if down.load(Ordering::SeqCst) {
            return Err(AppError::Io("东财网络不可达".into()));
        }
        match hits.get(&format!("{market}/{code}")) {
            Some(hit) => Ok(StockQuote {
                code: code.to_string(),
                name: hit.name.to_string(),
                market: market.to_string(),
                price_cents: hit.price.map(|(p, _)| p),
                price_date: hit.price.map(|(_, d)| d.to_string()),
                kind_hint: hit.kind_hint,
            }),
            None => Err(AppError::codedp(
                "sync.stock-not-found",
                format!("查无股票代码 {code}，请核对后重试"),
                &[code],
            )),
        }
    })
}

/// 带状态开关桩的一步装配：返回 (router, 连接, 不可达开关, 调用记录)。
type ToggleStubApp = (
    Router,
    Arc<Mutex<rusqlite::Connection>>,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<(String, String)>>>,
);

fn setup_app_with_toggle_stub(hits: HashMap<String, StockStubHit>) -> ToggleStubApp {
    let down = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fetch = toggle_stub(hits, down.clone(), calls.clone());
    let (app, conn) = crate::common::setup_app_with_stock_fetch(Some(fetch));
    (app, conn, down, calls)
}

#[tokio::test]
async fn test_create_stock_degrades_preserving_market_when_network_unreachable() {
    let (app, conn, down, calls) = setup_app_with_toggle_stub(stub_hit());
    down.store(true, Ordering::SeqCst);

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"600519","type":"stock","market":"sh","name":"贵州茅台"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "网络不可达应降级建行而非拒绝");
    let _: String = serde_json::from_slice(&bytes).unwrap();

    // 降级行：真实代码 + AI 提交名称 + 解析市场保留（与基金恒 unknown 的镜像差异）——
    // 行情通道只依赖（市场，代码），market 保留即价格通道可达。
    let row = stock_row(&conn, "600519");
    assert_eq!(
        row.name.as_deref(),
        Some("贵州茅台"),
        "降级行用 AI 提交名称"
    );
    assert_eq!(row.market, "sh", "降级必须保留解析市场");
    assert_eq!(row.source, "manual");
    assert!(row.price.is_none(), "网络不可达无价可落，不应有现价行");
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sh".to_string(), "600519".to_string())]
    );

    // 价格通道可达：行情恢复（down 复位）后，降级行携带的（市场，代码）经查询
    // 端点照常命中——market 保留即通道可达。
    down.store(false, Ordering::SeqCst);
    let (status, lookup) = get_json(&app, "/api/v1/stocks/600519?market=sh").await;
    assert_eq!(status, StatusCode::OK, "降级行的（市场，代码）应行情可达");
    assert_eq!(lookup["name"], "贵州茅台");
    assert_eq!(lookup["price_cents"], 150000);
}

#[tokio::test]
async fn test_create_stock_degrades_without_ai_name_creates_code_only_row() {
    let (app, conn, down, _calls) = setup_app_with_toggle_stub(stub_hit());
    down.store(true, Ordering::SeqCst);

    // 降级 + 未提交名称：代码 + 市场可用即建行（name 为 NULL），行情恢复后可回填。
    let (status, bytes) =
        post_instrument(&app, r#"{"symbol":"600519","type":"stock","market":"sh"}"#).await;
    assert_eq!(status, StatusCode::CREATED, "降级不因缺名称被阻塞");
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = stock_row(&conn, "600519");
    assert!(row.name.is_none(), "降级且无 AI 名称时应产生无名称行");
    assert_eq!(row.market, "sh");
    assert!(row.price.is_none());
}

#[tokio::test]
async fn test_create_stock_degrade_replay_keeps_existing_authoritative_name() {
    let (app, conn, down, calls) = setup_app_with_toggle_stub(stub_hit());

    // 第一笔：东财可达 → 权威名称回填 + 落价。
    let (status, bytes) =
        post_instrument(&app, r#"{"symbol":"600519","type":"stock","market":"sh"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).unwrap();

    // 第二笔：东财不可达 + AI 提交另一名称 → 降级建行成功、返回同一 id，
    // 既有权威名称不被 AI 名称覆盖。
    down.store(true, Ordering::SeqCst);
    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"600519","type":"stock","market":"sh","name":"账单抄写名（降级）"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "降级路径应成功建行/复用");
    let replay_id: String = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay_id, id, "降级重放应幂等复用同一标的");

    let row = stock_row(&conn, "600519");
    assert_eq!(
        row.name.as_deref(),
        Some("贵州茅台"),
        "降级重放不得用 AI 名称覆盖既有东财权威名称"
    );
    assert!(row.price.is_some(), "既有现价不被降级重放破坏");
    assert_eq!(calls.lock().unwrap().len(), 2, "两笔各发起一次东财尝试");
}

// ---------------------------------------------------------------------------
// 显式边界：北交所与 market 矛盾拒绝建行；兜底形态不触网
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_beijing_code_rejects_without_row_or_network() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"832000","type":"stock","name":"北交所标的"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        err["code"], "stock.bse-unsupported",
        "应复用查询端点同一码化边界"
    );
    assert!(
        err["message"].as_str().unwrap().contains("832000"),
        "报错应含代码，实际: {err}"
    );

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "北交所代码不应产生标的行");
    assert!(calls.lock().unwrap().is_empty(), "拒绝路径不发起网络请求");
}

#[tokio::test]
async fn test_create_stock_with_conflicting_market_rejects() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    // 600519 是沪市 6 开头代码，显式 market=sz 自相矛盾：400 拒绝，不建错挂行情的行。
    let (status, bytes) =
        post_instrument(&app, r#"{"symbol":"600519","type":"stock","market":"sz"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "stock.market-conflict");

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "矛盾参数不应产生标的行");
    assert!(calls.lock().unwrap().is_empty(), "拒绝路径不发起网络请求");
}

#[tokio::test]
async fn test_create_stock_with_name_as_code_skips_eastmoney_lookup() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    // 源数据确无代码：名称充代码兜底建行（自然键防碎），不触发东财校验、无现价。
    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"某雪球私募一号","type":"stock","name":"某雪球私募一号"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = stock_row(&conn, "某雪球私募一号");
    assert_eq!(row.name.as_deref(), Some("某雪球私募一号"));
    assert_eq!(row.market, "unknown");
    assert!(row.price.is_none(), "名称充代码的行不进行情通道");
    assert!(
        calls.lock().unwrap().is_empty(),
        "非代码形态不应发起东财请求"
    );
}

#[tokio::test]
async fn test_create_us_ticker_skips_eastmoney_lookup_until_us_markets_land() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    // 美股 ticker 属本接缝未开放形态（T4 议题）：按提交参数直接建行（market 保留），
    // 不触发东财校验、不拒绝——本票不改变该形态的既有可建行为。
    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"AAPL","type":"stock","market":"nasdaq"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = stock_row(&conn, "AAPL");
    assert_eq!(row.market, "nasdaq", "通用路径保留调用方 market");
    assert!(row.price.is_none());
    assert!(
        calls.lock().unwrap().is_empty(),
        "未开放形态不应发起东财请求"
    );
}

// ---------------------------------------------------------------------------
// 幂等重放：同自然键返回同一 id，现价不重复落行
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_stock_replay_returns_same_id_without_price_fragments() {
    let (app, conn, calls) = setup_app_with_stock_stub(stub_hit());

    let body = r#"{"symbol":"600519","type":"stock","market":"sh"}"#;
    let (first_status, first_bytes) = post_instrument(&app, body).await;
    assert_eq!(first_status, StatusCode::CREATED);
    let first_id: String = serde_json::from_slice(&first_bytes).unwrap();

    let (second_status, second_bytes) = post_instrument(&app, body).await;
    assert_eq!(second_status, StatusCode::CREATED);
    let second_id: String = serde_json::from_slice(&second_bytes).unwrap();

    assert_eq!(first_id, second_id, "幂等重放应返回同一 id");
    let price_rows: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(price_rows, 1, "重放应刷新现价而非产生碎片行");
    assert_eq!(calls.lock().unwrap().len(), 2, "两次创建各发起一次东财校验");
}

// ---------------------------------------------------------------------------
// 契约锁：stock 增强语义写入端点自述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_create_endpoint_documents_stock_enhancement() {
    let (app, _conn, _calls) = setup_app_with_stock_stub(stub_hit());
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let description = &doc["paths"]["/api/v1/instruments"]["post"]["description"];
    let description = description.as_str().unwrap_or_default();
    for expected in [
        "stock 类型增强",
        "权威名称",
        "落最新价现价",
        "查无此码",
        "降级",
        "保留解析市场",
        "名称充代码",
        "北交所",
    ] {
        assert!(
            description.contains(expected),
            "创建端点自述应说明 stock 增强: {expected}"
        );
    }
}
