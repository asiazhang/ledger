//! 标的搜索 HTTP 端点（`GET /api/v1/instruments`，issue #294 / ADR-0037）。
//!
//! 只断言外部行为：统一模糊搜索命中语义（代码/名称子串、拼音首字母、词条 AND、
//! 大小写不敏感）、`limit` 封顶返回与命中总数、缺省/空 `query` 返回 400、
//! `market`/`type` 过滤（同码异类型消歧）、完整 Instrument 形状与 symbol 排序、
//! 开放 API 契约自描述。标的经核心创建接缝直接入字典（幂等创建端点属 issue #296，
//! 本文件不依赖）。

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rusqlite::Connection;
use tower::ServiceExt;

use tauri_app_lib::investment::create_instrument;
use tauri_app_lib::models::{InstrumentInput, InstrumentType};

use crate::common::{body_to_bytes, get_json, setup_app};

/// 经核心创建接缝直接入字典（不发行情、不涉同步；币种固定 CNY 与断言无关）。
fn seed_instrument(
    conn: &Arc<Mutex<Connection>>,
    symbol: &str,
    kind: InstrumentType,
    name: Option<&str>,
    market: &str,
) {
    let conn = conn.lock().unwrap();
    create_instrument(
        &conn,
        InstrumentInput {
            symbol: symbol.to_string(),
            kind,
            name: name.map(str::to_string),
            currency_code: "CNY".to_string(),
            market: Some(market.to_string()),
        },
    )
    .unwrap();
}

/// 批量播种共享代码前缀的标的（symbol = `{prefix}{零填充序号}`）。
fn seed_symbol_series(conn: &Arc<Mutex<Connection>>, prefix: &str, count: usize) {
    for i in 1..=count {
        seed_instrument(
            conn,
            &format!("{prefix}{i:06}"),
            InstrumentType::Stock,
            None,
            "sz",
        );
    }
}

/// query 词的百分号编码（中文/空格在 URI 中须编码；axum Query 负责解码）。
fn q(term: &str) -> String {
    let mut out = String::new();
    for b in term.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn items_of_body(body: &serde_json::Value) -> &[serde_json::Value] {
    body["items"]
        .as_array()
        .expect("应返回 {items, total} 结构")
}

// ---------------------------------------------------------------------------
// 命中语义（统一模糊搜索，ADR-0027）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_hits_by_symbol_substring_and_name_substring() {
    let (app, conn) = setup_app();
    seed_instrument(
        &conn,
        "000001",
        InstrumentType::Stock,
        Some("平安银行"),
        "sz",
    );
    seed_instrument(
        &conn,
        "600519",
        InstrumentType::Stock,
        Some("贵州茅台"),
        "sh",
    );
    seed_instrument(
        &conn,
        "00700",
        InstrumentType::Stock,
        Some("腾讯控股"),
        "hk",
    );

    // 代码子串命中
    let (status, body) = get_json(&app, "/api/v1/instruments?query=600519").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let items = items_of_body(&body);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["symbol"], "600519");

    // 名称子串命中（迁移流水里通常只有名称没有代码）
    let (status, body) = get_json(&app, &format!("/api/v1/instruments?query={}", q("茅台"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let items = items_of_body(&body);
    assert_eq!(items[0]["name"], "贵州茅台");
}

#[tokio::test]
async fn test_search_hits_by_pinyin_initials() {
    let (app, conn) = setup_app();
    seed_instrument(
        &conn,
        "600519",
        InstrumentType::Stock,
        Some("贵州茅台"),
        "sh",
    );
    seed_instrument(
        &conn,
        "00700",
        InstrumentType::Stock,
        Some("腾讯控股"),
        "hk",
    );

    // 拼音首字母子序列命中：贵州茅台 → gzmt、腾讯控股 → txkg
    for (term, symbol) in [("gzmt", "600519"), ("txkg", "00700")] {
        let (status, body) = get_json(&app, &format!("/api/v1/instruments?query={term}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 1, "词条 {term} 应命中");
        assert_eq!(items_of_body(&body)[0]["symbol"], symbol);
    }

    // 大小写不敏感
    let (status, body) = get_json(&app, "/api/v1/instruments?query=GZMT").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
}

#[tokio::test]
async fn test_search_multi_terms_are_anded() {
    let (app, conn) = setup_app();
    seed_instrument(
        &conn,
        "600519",
        InstrumentType::Stock,
        Some("贵州茅台"),
        "sh",
    );
    seed_instrument(&conn, "000858", InstrumentType::Stock, Some("五粮液"), "sz");

    // 「贵州 600519」两个词条都必须命中同一标的
    let uri = format!("/api/v1/instruments?query={}%20{}", q("贵州"), q("600519"));
    let (status, body) = get_json(&app, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(items_of_body(&body)[0]["symbol"], "600519");
}

#[tokio::test]
async fn test_search_no_hit_returns_empty_items_with_zero_total() {
    let (app, conn) = setup_app();
    seed_instrument(
        &conn,
        "600519",
        InstrumentType::Stock,
        Some("贵州茅台"),
        "sh",
    );

    let (status, body) = get_json(&app, "/api/v1/instruments?query=不存在zzz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
    assert!(items_of_body(&body).is_empty());
}

// ---------------------------------------------------------------------------
// 封顶返回与命中总数
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_default_limit_20_total_counts_all_hits() {
    let (app, conn) = setup_app();
    seed_symbol_series(&conn, "CAP", 25);

    let (status, body) = get_json(&app, "/api/v1/instruments?query=CAP").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        items_of_body(&body).len(),
        20,
        "limit 缺省应为 20（封顶返回）"
    );
    assert_eq!(body["total"], 25, "total 恒为命中总数，不受 limit 影响");
}

#[tokio::test]
async fn test_search_limit_truncates_items_but_not_total() {
    let (app, conn) = setup_app();
    seed_symbol_series(&conn, "CAP", 25);

    let (status, body) = get_json(&app, "/api/v1/instruments?query=CAP&limit=5").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of_body(&body).len(), 5);
    assert_eq!(body["total"], 25);

    // 小于 1 视为 1（下界收敛，行为锁）
    let (status, body) = get_json(&app, "/api/v1/instruments?query=CAP&limit=0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of_body(&body).len(), 1);
    assert_eq!(body["total"], 25);
}

#[tokio::test]
async fn test_search_limit_clamped_to_100() {
    let (app, conn) = setup_app();
    seed_symbol_series(&conn, "BIG", 105);

    let (status, body) = get_json(&app, "/api/v1/instruments?query=BIG&limit=500").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of_body(&body).len(), 100, "limit 上限收敛 100");
    assert_eq!(body["total"], 105);
}

// ---------------------------------------------------------------------------
// query 必填（搜索式而非全量列表）
// ---------------------------------------------------------------------------

fn assert_bad_request(status: StatusCode, body: &serde_json::Value, what: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{what} 应返回 400");
    assert!(
        body["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{what} 应返回中文错误信息"
    );
}

#[tokio::test]
async fn test_search_without_query_returns_400() {
    let (app, _) = setup_app();
    let (status, body) = get_json(&app, "/api/v1/instruments").await;
    assert_bad_request(status, &body, "缺 query");
}

#[tokio::test]
async fn test_search_empty_query_returns_400() {
    let (app, _) = setup_app();
    let (status, body) = get_json(&app, "/api/v1/instruments?query=").await;
    assert_bad_request(status, &body, "空 query");

    let (status, body) = get_json(&app, "/api/v1/instruments?query=%20%20").await;
    assert_bad_request(status, &body, "纯空白 query");
}

#[tokio::test]
async fn test_search_invalid_type_param_returns_400() {
    let (app, _) = setup_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/instruments?query=x&type=bogus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "非法 type 枚举值应返回 400"
    );
    let _ = body_to_bytes(response.into_body()).await;
}

// ---------------------------------------------------------------------------
// market / type 过滤
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_market_filter_narrows_hits() {
    let (app, conn) = setup_app();
    seed_instrument(&conn, "MKTAAA", InstrumentType::Stock, None, "sh");
    seed_instrument(&conn, "MKTBBB", InstrumentType::Stock, None, "hk");

    // 不过滤：两条都命中
    let (status, body) = get_json(&app, "/api/v1/instruments?query=MKT").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);

    // market 透传既有口径：精确过滤
    let (status, body) = get_json(&app, "/api/v1/instruments?query=MKT&market=hk").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(items_of_body(&body)[0]["symbol"], "MKTBBB");

    // 空串 market 视同未传（与既有接缝口径一致：空值不过滤）
    let (status, body) = get_json(&app, "/api/v1/instruments?query=MKT&market=").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);

    // 无此市场的标的
    let (status, body) = get_json(&app, "/api/v1/instruments?query=MKT&market=us").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn test_search_type_filter_disambiguates_same_symbol() {
    let (app, conn) = setup_app();
    // 同码异类型：基金 000001 vs 股票 000001（spec 例）
    seed_instrument(
        &conn,
        "000001",
        InstrumentType::Fund,
        Some("华夏成长混合"),
        "sz",
    );
    seed_instrument(
        &conn,
        "000001",
        InstrumentType::Stock,
        Some("平安银行"),
        "sz",
    );

    // 不过滤：两行并返
    let (status, body) = get_json(&app, "/api/v1/instruments?query=000001").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);

    // type=fund 消歧
    let (status, body) = get_json(&app, "/api/v1/instruments?query=000001&type=fund").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let items = items_of_body(&body);
    assert_eq!(items[0]["type"], "fund");
    assert_eq!(items[0]["name"], "华夏成长混合");

    // type=stock 消歧
    let (status, body) = get_json(&app, "/api/v1/instruments?query=000001&type=stock").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(items_of_body(&body)[0]["name"], "平安银行");

    // market + type 可组合
    let (status, body) = get_json(
        &app,
        "/api/v1/instruments?query=000001&type=stock&market=hk",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0, "股票 000001 在深市，hk 过滤后无命中");
}

// ---------------------------------------------------------------------------
// 响应形状与排序
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_search_returns_full_instrument_shape_sorted_by_symbol() {
    let (app, conn) = setup_app();
    // 插入顺序与 symbol 序相反，验证按 symbol 排序
    seed_instrument(&conn, "ORD003", InstrumentType::Bond, Some("丙债"), "sh");
    seed_instrument(&conn, "ORD001", InstrumentType::Stock, Some("甲股"), "sz");
    seed_instrument(&conn, "ORD002", InstrumentType::Etf, None, "sz");

    let (status, body) = get_json(&app, "/api/v1/instruments?query=ORD").await;
    assert_eq!(status, StatusCode::OK);
    let items = items_of_body(&body);
    let symbols: Vec<&str> = items
        .iter()
        .map(|i| i["symbol"].as_str().unwrap())
        .collect();
    assert_eq!(
        symbols,
        vec!["ORD001", "ORD002", "ORD003"],
        "应按 symbol 排序"
    );

    // 完整 Instrument 形状（含派生字段 price_cents / invested）
    let first = &items[0];
    for field in [
        "id",
        "symbol",
        "type",
        "name",
        "currency_code",
        "market",
        "created_at",
        "updated_at",
        "version",
        "device_id",
        "price_cents",
        "invested",
    ] {
        assert!(first.get(field).is_some(), "Instrument 应包含字段 {field}");
    }
    assert_eq!(first["type"], "stock");
    assert_eq!(first["price_cents"], serde_json::Value::Null);
    assert_eq!(first["invested"], false);
}

// ---------------------------------------------------------------------------
// 开放 API 契约自描述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_doc_covers_instruments_search_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;

    let get = &doc["paths"]["/api/v1/instruments"]["get"];
    assert!(get["summary"].is_string(), "OpenAPI 应包含标的搜索端点");
    let description = get["description"].as_str().unwrap_or_default();
    assert!(
        description.contains("query"),
        "端点自述应说明 query 必填语义"
    );

    let params = get["parameters"]
        .as_array()
        .expect("搜索端点应声明查询参数");
    let names: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
    for expected in ["query", "limit", "market", "type"] {
        assert!(names.contains(&expected), "应声明查询参数 {expected}");
    }

    let responses = get["responses"].as_object().unwrap();
    assert!(responses.contains_key("200"), "应声明 200 响应");
    assert!(
        responses.contains_key("400"),
        "应声明 400 响应（query 缺省）"
    );
    let response_200 = &responses["200"]["content"]["application/json"]["schema"];
    assert_eq!(
        response_200["$ref"], "#/components/schemas/InstrumentListResult",
        "响应 schema 应为 InstrumentListResult"
    );

    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let list_result = schemas
        .get("InstrumentListResult")
        .expect("应包含 InstrumentListResult schema");
    let props = list_result["properties"].as_object().unwrap();
    assert!(props.contains_key("items"));
    assert!(props.contains_key("total"));

    let instrument = schemas.get("Instrument").expect("应包含 Instrument schema");
    let props = instrument["properties"].as_object().unwrap();
    for field in [
        "id",
        "symbol",
        "type",
        "name",
        "currency_code",
        "market",
        "invested",
    ] {
        assert!(props.contains_key(field), "Instrument 应包含字段 {field}");
    }

    let kind = schemas
        .get("InstrumentType")
        .expect("应包含 InstrumentType schema");
    let enum_values: Vec<&str> = kind["enum"]
        .as_array()
        .expect("InstrumentType 应为字符串枚举")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        enum_values,
        vec!["stock", "fund", "bond", "etf", "other"],
        "类型枚举应为闭集 5 个小写字符串"
    );
}
