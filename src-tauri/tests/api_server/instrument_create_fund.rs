use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::StatusCode;
use rusqlite::params;

use ledger_infra::error::AppError;
use tauri_app_lib::api_server::FundQuoteFetcher;

use crate::common::{FundStubHit, post_instrument, setup_app_with_fund_stub};

struct FundRow {
    name: Option<String>,
    market: String,
    source: String,
    price: Option<(i64, Option<String>, String)>,
}

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

fn stub_hit() -> HashMap<String, FundStubHit> {
    HashMap::from([(
        "000001".to_string(),
        FundStubHit {
            name: "华夏成长混合",
            fund_class: "混合型-灵活",
            nav: Some((1.318, "2026-08-28")),
            money_fund: false,
        },
    )])
}

#[tokio::test]
async fn test_create_fund_with_known_code_backfills_authoritative_name_and_price() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    let body = r#"{"symbol":"000001","type":"fund","name":"华夏成长混合A（错误抄写）"}"#;
    let (status, bytes) = post_instrument(&app, body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");
    assert!(!id.is_empty());

    let row = fund_row(&conn, "000001");
    assert_eq!(
        row.name.as_deref(),
        Some("华夏成长混合"),
        "行情源可达时应回填权威名称，而非 AI 抄写名"
    );
    assert_eq!(row.market, "unknown", "场外基金市场恒 unknown（ADR-0038）");
    assert_eq!(row.source, "manual");
    let (price_cents, nav_date, price_source) = row.price.expect("行情命中应落现价缓存");
    assert_eq!(price_cents, 13180, "净值 1.318 元 = 万分之一元刻度 13180");
    assert_eq!(
        nav_date.as_deref(),
        Some("2026-08-28"),
        "现价应携带净值日期"
    );
    assert_eq!(price_source, "sina");
    assert_eq!(*calls.lock().unwrap(), vec!["000001".to_string()]);
}

#[tokio::test]
async fn test_create_terminated_fund_from_archive_channel_succeeds() {
    let hits = HashMap::from([(
        "002503".to_string(),
        FundStubHit {
            name: "中银腾利混合C",
            fund_class: "",
            nav: Some((1.144, "2023-09-18")),
            money_fund: false,
        },
    )]);
    let (app, conn, calls) = setup_app_with_fund_stub(hits);

    let (status, bytes) = post_instrument(&app, r#"{"symbol":"002503","type":"fund"}"#).await;
    assert_eq!(status, StatusCode::CREATED, "已终止基金不再是 400 拒绝");
    let id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");
    assert!(!id.is_empty());

    let row = fund_row(&conn, "002503");
    assert_eq!(
        row.name.as_deref(),
        Some("中银腾利混合C"),
        "名称取自档案通道"
    );
    assert_eq!(row.market, "unknown", "场外基金市场恒 unknown（ADR-0038）");
    let (price_cents, nav_date, _) = row.price.expect("档案通道命中应落最后一期净值现价");
    assert_eq!(price_cents, 11_440, "净值 1.1440 元 = 万分之一元刻度 11440");
    assert_eq!(nav_date.as_deref(), Some("2023-09-18"));
    assert_eq!(*calls.lock().unwrap(), vec!["002503".to_string()]);
}

#[tokio::test]
async fn test_create_fund_with_known_code_but_no_nav_creates_without_price() {
    let hits = HashMap::from([(
        "012345".to_string(),
        FundStubHit {
            name: "新发基金",
            fund_class: "混合型",
            nav: None,
            money_fund: false,
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
    assert_eq!(err["code"], "sync.fund-not-found");
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

fn toggle_stub(
    hits: HashMap<String, FundStubHit>,
    down: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
) -> FundQuoteFetcher {
    Arc::new(move |code: &str| {
        calls.lock().unwrap().push(code.to_string());
        let result = if down.load(Ordering::SeqCst) {
            Err(AppError::Io("行情源网络不可达".into()))
        } else {
            match hits.get(code) {
                Some(hit) => Ok(ledger_investment::Quote {
                    code: code.to_string(),
                    name: hit.name.to_string(),
                    price_cents: hit
                        .nav
                        .map(|(nav, _)| ledger_investment::prices::price_value_to_cents(nav)),
                    price_date: hit.nav.map(|(_, nav_date)| nav_date.to_string()),
                    market: None,
                    kind_hint: None,
                    fund_class: Some(hit.fund_class.to_string()),
                    nav_date: hit.nav.map(|(_, nav_date)| nav_date.to_string()),
                    constant_unit_price_cents: hit
                        .money_fund
                        .then(|| ledger_investment::prices::price_value_to_cents(1.0)),
                    price_source: ledger_investment::prices::SINA_PRICE_SOURCE,
                }),
                None => Err(AppError::codedp(
                    "sync.fund-not-found",
                    format!("查无基金代码 {code}，请核对后重试"),
                    &[code],
                )),
            }
        };
        crate::common::ready_quote(result)
    })
}

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

    let (status, bytes) = post_instrument(&app, r#"{"symbol":"000001","type":"fund"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).unwrap();

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
        "降级重放不得用 AI 名称覆盖既有权威名称"
    );
    assert!(row.price.is_some(), "既有现价不被降级重放破坏");
    assert_eq!(calls.lock().unwrap().len(), 2, "两笔各发起一次行情源校验");
}

#[tokio::test]
async fn test_create_fund_with_name_as_code_skips_remote_lookup() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

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
        "非 6 位 symbol 不应发起行情源请求"
    );
}

#[tokio::test]
async fn test_create_non_fund_type_skips_fund_lookup() {
    let (app, _conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"600519","type":"bond","name":"贵州茅台","market":"sh"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();

    assert!(
        calls.lock().unwrap().is_empty(),
        "非 fund 类型不应发起基金详情请求"
    );
}

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
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "两次创建各发起一次行情源校验"
    );
}

#[tokio::test]
async fn test_create_fund_with_market_rejects_without_row() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"某基金组合","type":"fund","name":"某基金组合","market":"sh"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(
        err["code"], "instrument.fund-market-forbidden",
        "fund 行携带市场应以稳定错误码拒绝（ADR-0050）"
    );

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='某基金组合'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "被拒的 fund 创建不应产生标的行");
    assert!(
        calls.lock().unwrap().is_empty(),
        "非 6 位 symbol 不应发起行情源请求"
    );
}

#[tokio::test]
async fn test_create_fund_without_market_creates_unknown_row() {
    let (app, conn, _calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"某基金组合A","type":"fund","name":"某基金组合A"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(fund_row(&conn, "某基金组合A").market, "unknown");

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"某基金组合B","type":"fund","name":"某基金组合B","market":"unknown"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let _: String = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(fund_row(&conn, "某基金组合B").market, "unknown");
}

#[tokio::test]
async fn test_create_six_digit_fund_with_market_rejects_without_lookup_or_row() {
    let (app, conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"000001","type":"fund","name":"华夏成长混合","market":"sh"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(err["code"], "instrument.fund-market-forbidden");

    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "被拒的 6 位 fund 创建不应产生标的行");
    assert!(
        calls.lock().unwrap().is_empty(),
        "入口守卫应先于行情源往返拒绝，不发起网络请求"
    );
}

#[tokio::test]
async fn test_openapi_create_endpoint_documents_fund_enhancement() {
    let (app, _conn, _calls) = setup_app_with_fund_stub(stub_hit());
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let description = &doc["paths"]["/api/v1/instruments"]["post"]["description"];
    let description = description.as_str().unwrap_or_default();
    for expected in ["fund/stock", "权威名称", "查无此码", "降级", "导入知识"] {
        assert!(
            description.contains(expected),
            "创建端点自述应说明 fund 增强: {expected}"
        );
    }
}

#[tokio::test]
async fn test_create_money_fund_marks_constant_price_with_empty_nav_date() {
    let hits = HashMap::from([(
        "000198".to_string(),
        FundStubHit {
            name: "天弘余额宝货币",
            fund_class: "货币型",
            nav: Some((1.0, "2026-09-17")),
            money_fund: true,
        },
    )]);
    let (app, conn, _calls) = setup_app_with_fund_stub(hits);

    let body = r#"{"symbol":"000198","type":"fund","name":"余额宝"}"#;
    let (status, bytes) = post_instrument(&app, body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    let (constant_cents, channel_count): (Option<i64>, i64) = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT constant_unit_price, \
             (SELECT COUNT(*) FROM instruments WHERE constant_unit_price IS NOT NULL) \
             FROM instruments WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(constant_cents, Some(10_000), "建档即打标恒定单位价格");
    assert_eq!(channel_count, 1);

    let (price_cents, nav_date, price_source) = fund_row(&conn, "000198")
        .price
        .expect("货基建档应落常量现价");
    assert_eq!(price_cents, 10_000, "恒定单位净值 1.0000");
    assert_eq!(nav_date, None, "净值日期列为空");
    assert_eq!(price_source, "sina");
}
