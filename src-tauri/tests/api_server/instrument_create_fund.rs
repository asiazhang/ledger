//! 标的创建端点的 fund 增强（`POST /api/v1/instruments`，issue #304 / ADR-0039 决策 3）。
//!
//! 只断言外部行为：fund + 真实 6 位代码经东财校验——命中回填权威名称并落最新
//! 净值现价（万分之一元刻度 + 净值日期）、查无此码 400 拒绝且不产生标的行、
//! 网络不可达降级为提交名称 + 真实代码建行（不阻塞）；降级重放不覆盖既有权威
//! 名称；名称充代码（非 6 位）与其他类型不发起网络请求；幂等重放返回同一 id。
//! 东财访问经注入桩离线驱动。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::StatusCode;
use rusqlite::params;

use tauri_app_lib::api_server::FundDetailFetcher;
use tauri_app_lib::error::AppError;

use crate::common::{FundStubHit, post_instrument, setup_app_with_fund_stub};

/// fund 标的行与现价行的断言投影：name / market / source 与可选现价
/// （price_cents, nav_date, source）。
struct FundRow {
    name: Option<String>,
    market: String,
    source: String,
    price: Option<(i64, Option<String>, String)>,
}

/// 查询 fund 标的行（name, market, source）与现价行（price_cents, nav_date, source）。
fn fund_row(conn: &Arc<Mutex<rusqlite::Connection>>, symbol: &str) -> FundRow {
    let conn = conn.lock().unwrap();
    let (name, market, source) = conn
        .query_row(
            "SELECT name, market, source FROM instruments WHERE symbol=?1 AND instrument_type='fund'",
            params![symbol],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap_or_else(|_| panic!("fund 标的行应存在: {symbol}"));
    let price = conn
        .query_row(
            "SELECT p.price_cents, p.nav_date, p.source FROM market_prices p \
             JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1 AND i.instrument_type='fund'",
            params![symbol],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .ok();
    FundRow {
        name,
        market,
        source,
        price,
    }
}

/// 命中表：000001 → 华夏成长混合 / 混合型-灵活 / 净值 1.318 元 @ 2026-08-28。
fn stub_hit() -> HashMap<String, FundStubHit> {
    HashMap::from([(
        "000001".to_string(),
        FundStubHit {
            name: "华夏成长混合",
            fund_class: "混合型-灵活",
            nav: Some((1.318, "2026-08-28")),
        },
    )])
}

// ---------------------------------------------------------------------------
// 东财命中：权威名称回填 + 净值落现价
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_fund_with_known_code_backfills_authoritative_name_and_price() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    // AI 提交的名称有误（抄错），后端应以东财权威名称为准。
    let body = r#"{"symbol":"000001","type":"fund","name":"华夏成长混合A（错误抄写）"}"#;
    let (status, bytes) = post_instrument(&app, body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");
    assert!(!id.is_empty());

    let row = fund_row(&conn, "000001");
    assert_eq!(
        row.name.as_deref(),
        Some("华夏成长混合"),
        "东财可达时应回填权威名称，而非 AI 抄写名"
    );
    assert_eq!(row.market, "unknown", "场外基金市场恒 unknown（ADR-0038）");
    assert_eq!(row.source, "manual");
    let (price_cents, nav_date, price_source) = row.price.expect("东财命中应落现价缓存");
    assert_eq!(price_cents, 13180, "净值 1.318 元 = 万分之一元刻度 13180");
    assert_eq!(
        nav_date.as_deref(),
        Some("2026-08-28"),
        "现价应携带净值日期"
    );
    assert_eq!(price_source, "eastmoney");
    assert_eq!(*calls.lock().unwrap(), vec!["000001".to_string()]);
}

#[tokio::test]
async fn test_create_fund_with_known_code_but_no_nav_creates_without_price() {
    let hits = HashMap::from([(
        "012345".to_string(),
        FundStubHit {
            name: "新发基金",
            fund_class: "混合型",
            nav: None,
        },
    )]);
    let (app, conn, calls) = setup_app_with_fund_stub(hits);

    let (status, bytes) = post_instrument(&app, r#"{"symbol":"012345","type":"fund"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = fund_row(&conn, "012345");
    assert_eq!(
        row.name.as_deref(),
        Some("新发基金"),
        "无净值仍应回填权威名称"
    );
    assert!(row.price.is_none(), "未取到净值不应落现价");
    assert_eq!(*calls.lock().unwrap(), vec!["012345".to_string()]);
}

// ---------------------------------------------------------------------------
// 查无此码：拒绝创建，不产生标的行
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_fund_with_unknown_code_rejects_without_row() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"999999","type":"fund","name":"不存在的基金"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("查无基金代码 999999"),
        "查无此码应显式报错，实际: {err}"
    );

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "查无此码不应产生标的行");
    assert_eq!(*calls.lock().unwrap(), vec!["999999".to_string()]);
}

// ---------------------------------------------------------------------------
// 网络不可达：降级为提交名称 + 真实代码建行；重放不覆盖既有权威名称
// ---------------------------------------------------------------------------

/// 状态开关桩：`down=true` 模拟东财网络不可达（Io），否则按命中表返回。
fn toggle_stub(
    hits: HashMap<String, FundStubHit>,
    down: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
) -> FundDetailFetcher {
    Arc::new(move |code: &str| {
        calls.lock().unwrap().push(code.to_string());
        if down.load(Ordering::SeqCst) {
            return Err(AppError::Io("东财网络不可达".into()));
        }
        match hits.get(code) {
            Some(hit) => Ok(tauri_app_lib::models::FundDetail {
                code: code.to_string(),
                name: hit.name.to_string(),
                fund_class: hit.fund_class.to_string(),
                nav: hit
                    .nav
                    .map(|(nav, nav_date)| tauri_app_lib::models::FundNav {
                        nav,
                        nav_date: nav_date.to_string(),
                    }),
            }),
            None => Err(AppError::Invalid(format!(
                "查无基金代码 {code}，请核对后重试"
            ))),
        }
    })
}

/// 带状态开关桩的一步装配：返回 (router, 连接, 不可达开关, 调用记录)，
/// 供「先命中后不可达」等跨请求状态切换测试使用。
type ToggleStubApp = (
    Router,
    Arc<Mutex<rusqlite::Connection>>,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<String>>>,
);

fn setup_app_with_toggle_stub(hits: HashMap<String, FundStubHit>) -> ToggleStubApp {
    let down = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fetch = toggle_stub(hits, down.clone(), calls.clone());
    let (app, conn) = crate::common::setup_app_with_fund_fetch(Some(fetch));
    (app, conn, down, calls)
}

#[tokio::test]
async fn test_create_fund_degrades_to_ai_name_when_network_unreachable() {
    let (app, conn, down, calls) = setup_app_with_toggle_stub(stub_hit());
    down.store(true, Ordering::SeqCst);

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"000001","type":"fund","name":"华夏成长混合"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "网络不可达应降级建行而非拒绝");
    let _: String = serde_json::from_slice(&bytes).unwrap();

    // 降级行：真实代码 + AI 提交名称、字典形态与按代码即拉同款（unknown/manual），无现价。
    let row = fund_row(&conn, "000001");
    assert_eq!(
        row.name.as_deref(),
        Some("华夏成长混合"),
        "降级行用 AI 提交名称"
    );
    assert_eq!(row.market, "unknown");
    assert_eq!(row.source, "manual");
    assert!(row.price.is_none(), "网络不可达无净值可落，不应有现价行");
    assert_eq!(*calls.lock().unwrap(), vec!["000001".to_string()]);
}

#[tokio::test]
async fn test_create_fund_degrades_without_ai_name_creates_code_only_row() {
    let (app, conn, down, _calls) = setup_app_with_toggle_stub(stub_hit());
    down.store(true, Ordering::SeqCst);

    // 降级 + 未提交名称：代码可用即建行（name 为 NULL），后续净值通道照常服务。
    let (status, bytes) = post_instrument(&app, r#"{"symbol":"000001","type":"fund"}"#).await;
    assert_eq!(status, StatusCode::CREATED, "降级不因缺名称被阻塞");
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = fund_row(&conn, "000001");
    assert!(
        row.name.is_none(),
        "降级且无 AI 名称时应产生无名称行（后续可经净值回填或人工补录）"
    );
    assert!(row.price.is_none());
}

#[tokio::test]
async fn test_create_fund_degrade_replay_keeps_existing_authoritative_name() {
    let (app, conn, down, calls) = setup_app_with_toggle_stub(stub_hit());

    // 第一笔：东财可达 → 权威名称回填。
    let (status, bytes) = post_instrument(&app, r#"{"symbol":"000001","type":"fund"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).unwrap();

    // 第二笔：东财不可达 + AI 提交了另一个名称 → 降级建行成功、返回同一 id，
    // 既有权威名称不被 AI 名称覆盖。
    down.store(true, Ordering::SeqCst);
    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"000001","type":"fund","name":"账单抄写名（降级）"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "降级路径应成功建行/复用");
    let replay_id: String = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay_id, id, "降级重放应幂等复用同一标的");

    let row = fund_row(&conn, "000001");
    assert_eq!(
        row.name.as_deref(),
        Some("华夏成长混合"),
        "降级重放不得用 AI 名称覆盖既有东财权威名称"
    );
    assert!(row.price.is_some(), "既有现价不被降级重放破坏");
    assert_eq!(calls.lock().unwrap().len(), 2, "两笔各发起一次东财尝试");
}

// ---------------------------------------------------------------------------
// 名称充代码（非 6 位）与其他类型：不发起网络请求
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_fund_with_name_as_code_skips_eastmoney_lookup() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    // 源数据无代码：名称充代码建行（自然键防碎），不触发东财校验、无现价。
    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"某雪球私募一号","type":"fund","name":"某雪球私募一号"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    let row = fund_row(&conn, "某雪球私募一号");
    assert_eq!(row.name.as_deref(), Some("某雪球私募一号"));
    assert_eq!(row.market, "unknown");
    assert!(row.price.is_none(), "名称充代码的基金行不进净值通道");
    assert!(
        calls.lock().unwrap().is_empty(),
        "非 6 位 symbol 不应发起东财请求"
    );
}

#[tokio::test]
async fn test_create_non_fund_type_skips_eastmoney_lookup() {
    let (app, _conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    assert!(
        calls.lock().unwrap().is_empty(),
        "非 fund 类型不应发起东财请求（股票创建不受增强影响）"
    );
}

// ---------------------------------------------------------------------------
// 幂等重放：同自然键返回同一 id，现价不重复落行
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_fund_replay_returns_same_id_without_price_fragments() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    let body = r#"{"symbol":"000001","type":"fund","name":"华夏成长混合"}"#;
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
// 契约锁：fund 增强语义写入端点自述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_create_endpoint_documents_fund_enhancement() {
    let (app, _conn, _calls) = setup_app_with_fund_stub(stub_hit());
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let description = &doc["paths"]["/api/v1/instruments"]["post"]["description"];
    let description = description.as_str().unwrap_or_default();
    for expected in [
        "fund 类型增强",
        "权威名称",
        "查无此码",
        "降级",
        "名称充代码",
    ] {
        assert!(
            description.contains(expected),
            "创建端点自述应说明 fund 增强: {expected}"
        );
    }
}
