//! 标的幂等创建 HTTP 端点（`POST /api/v1/instruments`，issue #296 / ADR-0037）。
//!
//! 只断言外部行为：find-or-create 幂等（自然键（symbol, type），命中静默复用并按需
//! 更新名称/市场、返回既有 id）、201 + 裸 id 响应形状（照账户/分类创建先例）、
//! 创建行来源标记 = `'manual'`、报价币种缺省按市场推导（沪深→CNY、港→HKD、
//! 未知→CNY，显式传参可覆盖）、类型五类全开、错误为统一错误形状中文信息、
//! 开放 API 契约自描述。泛型断言以非 fund 类型为代表；fund 类型的东财增强
//! （回填权威名称/净值、查无此码拒绝、不可达降级）见 instrument_create_fund.rs
//! （issue #304 / ADR-0039）。

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use rusqlite::params;
use tower::ServiceExt;

use crate::common::{body_to_bytes, setup_app};

/// POST /api/v1/instruments，返回（状态码，原始响应体）。
/// 响应体不在此处反序列化：201 为裸 id 字符串、4xx 可能非 JSON，由各测试自行解析。
async fn post_instrument(app: &Router, body: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/instruments")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

/// 响应体应为裸 id 字符串（非 JSON 对象——`{"id": …}` 之类包装在此即解析失败）。
fn id_of(bytes: &[u8]) -> String {
    serde_json::from_slice(bytes).expect("响应体应为裸 id 字符串")
}

/// 按自然键（symbol, type）查行字段（currency_code, market, source）。
fn fields_of(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    symbol: &str,
    kind: &str,
) -> (String, String, String) {
    conn.lock()
        .unwrap()
        .query_row(
            "SELECT currency_code, market, source FROM instruments \
             WHERE symbol=?1 AND instrument_type=?2",
            params![symbol, kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

// ---------------------------------------------------------------------------
// 201 + 裸 id；创建行来源 = 'manual'
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_instrument_returns_201_with_bare_id_and_manual_source() {
    let (app, conn) = setup_app();

    let body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh","currency_code":"CNY"}"#;
    let (status, bytes) = post_instrument(&app, body).await;

    assert_eq!(status, StatusCode::CREATED);
    let id = id_of(&bytes);
    assert!(!id.is_empty(), "返回的 ID 不应为空");

    let (currency_code, market, source) = fields_of(&conn, "600519", "stock");
    assert_eq!(currency_code, "CNY");
    assert_eq!(market, "sh");
    assert_eq!(source, "manual", "AI 创建行来源标记应为手动");
}

// ---------------------------------------------------------------------------
// find-or-create 幂等：命中静默复用并按需更新名称/市场
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_instrument_idempotent_returns_same_id_without_new_row() {
    let (app, conn) = setup_app();

    let first_body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh"}"#;
    let (first_status, first_bytes) = post_instrument(&app, first_body).await;
    assert_eq!(first_status, StatusCode::CREATED);
    let first_id = id_of(&first_bytes);

    // 重跑同自然键：名称/市场有变也复用（按需更新），仍返回 201 + 既有 id。
    let second_body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台A","market":"sz"}"#;
    let (second_status, second_bytes) = post_instrument(&app, second_body).await;
    assert_eq!(second_status, StatusCode::CREATED);
    let second_id = id_of(&second_bytes);

    assert_eq!(first_id, second_id, "同（symbol, 类型）应返回同一 id");
    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='600519' AND instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "幂等重放不应产生字典碎片");

    let (_, market, _) = fields_of(&conn, "600519", "stock");
    assert_eq!(market, "sz", "命中复用应按需更新市场");

    let name: Option<String> = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT name FROM instruments WHERE symbol='600519' AND instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("贵州茅台A"), "命中复用应按需更新名称");
}

#[tokio::test]
async fn test_create_instrument_distinguishes_type_for_same_symbol() {
    let (app, conn) = setup_app();

    let (stock_status, stock_bytes) = post_instrument(
        &app,
        r#"{"symbol":"000001","type":"stock","name":"平安银行"}"#,
    )
    .await;
    assert_eq!(stock_status, StatusCode::CREATED);
    let (etf_status, etf_bytes) =
        post_instrument(&app, r#"{"symbol":"000001","type":"etf"}"#).await;
    assert_eq!(etf_status, StatusCode::CREATED);

    let stock_id = id_of(&stock_bytes);
    let etf_id = id_of(&etf_bytes);
    assert_ne!(stock_id, etf_id, "同码异类型应为不同标的（自然键含类型）");
    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='000001'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "同码异类型应各自成行");
}

// ---------------------------------------------------------------------------
// 类型五类全开（不经自建标的的 UI 白名单；泛型断言以非 fund 类型为代表）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_instrument_all_types_open_without_ui_whitelist() {
    let (app, conn) = setup_app();

    // stock 是自建标的 UI 白名单排除的类型——AI 面可建即为「不经白名单」的直接证据。
    for (symbol, kind) in [
        ("600519", "stock"),
        ("019547", "bond"),
        ("510300", "etf"),
        ("稳稳地幸福", "other"),
    ] {
        let body = format!(r#"{{"symbol":"{symbol}","type":"{kind}"}}"#);
        let (status, bytes) = post_instrument(&app, &body).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "类型 {kind} 应可创建（五类全开）"
        );
        assert!(!id_of(&bytes).is_empty());
    }
    let count: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 4, "四个非 fund 类型应各自成行");
}

// ---------------------------------------------------------------------------
// 报价币种缺省推导（沪深→CNY、港→HKD、未知→CNY），显式传参可覆盖
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_instrument_derives_currency_from_market() {
    let (app, conn) = setup_app();

    // 沪深→人民币
    let (status, bytes) =
        post_instrument(&app, r#"{"symbol":"600519","type":"stock","market":"sh"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(!id_of(&bytes).is_empty());
    let (sz_status, _) =
        post_instrument(&app, r#"{"symbol":"000001","type":"stock","market":"sz"}"#).await;
    assert_eq!(sz_status, StatusCode::CREATED);
    // 港→港币
    post_instrument(&app, r#"{"symbol":"00700","type":"stock","market":"hk"}"#).await;

    assert_eq!(fields_of(&conn, "600519", "stock").0, "CNY", "沪→人民币");
    assert_eq!(fields_of(&conn, "000001", "stock").0, "CNY", "深→人民币");
    assert_eq!(fields_of(&conn, "00700", "stock").0, "HKD", "港→港币");
}

#[tokio::test]
async fn test_create_instrument_without_market_defaults_unknown_and_cny() {
    let (app, conn) = setup_app();

    let (status, _) = post_instrument(&app, r#"{"symbol":"稳稳地幸福","type":"other"}"#).await;
    assert_eq!(status, StatusCode::CREATED);

    let (currency_code, market, _) = fields_of(&conn, "稳稳地幸福", "other");
    assert_eq!(market, "unknown", "market 缺省应为 unknown");
    assert_eq!(currency_code, "CNY", "未知市场推导为人民币");
}

#[tokio::test]
async fn test_create_instrument_explicit_currency_overrides_derivation() {
    let (app, conn) = setup_app();

    let (status, _) = post_instrument(
        &app,
        r#"{"symbol":"00700","type":"stock","market":"hk","currency_code":"USD"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (currency_code, _, _) = fields_of(&conn, "00700", "stock");
    assert_eq!(currency_code, "USD", "显式传参应覆盖市场推导");
}

// ---------------------------------------------------------------------------
// 错误形状：统一 {kind, message} 中文信息
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_instrument_with_blank_symbol_returns_400_unified_shape() {
    let (app, _) = setup_app();

    let (status, bytes) = post_instrument(&app, r#"{"symbol":"   ","type":"stock"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid", "错误应为统一形状的 kind 字段");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("标的代码不能为空"),
        "错误信息应为中文，实际: {}",
        err["message"]
    );
}

/// 请求体格式错误（缺必填字段 / type 非法枚举值）：与账户创建先例一致，
/// 由 axum Json extractor 拒绝返回 422（格式错误不是业务校验，不占统一错误形状）。
#[tokio::test]
async fn test_create_instrument_with_malformed_body_returns_422() {
    let (app, _) = setup_app();

    let (missing_status, _) = post_instrument(&app, r#"{"type":"stock"}"#).await;
    assert_eq!(
        missing_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "缺 symbol 应 422"
    );

    let (bad_type_status, _) = post_instrument(&app, r#"{"symbol":"600519","type":"bogus"}"#).await;
    assert_eq!(
        bad_type_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "非法 type 应 422"
    );
}

// ---------------------------------------------------------------------------
// 开放 API 契约自描述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_doc_covers_instruments_create_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let post = &doc["paths"]["/api/v1/instruments"]["post"];
    assert!(post["summary"].is_string(), "OpenAPI 应包含标的创建端点");
    let description = post["description"].as_str().unwrap_or_default();
    for expected in ["幂等", "currency_code", "market", "manual"] {
        assert!(
            description.contains(expected),
            "端点自述应说明 {expected} 语义"
        );
    }

    let request = &post["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(
        request["$ref"], "#/components/schemas/InstrumentCreateInput",
        "请求体 schema 应为 InstrumentCreateInput"
    );
    let responses = post["responses"].as_object().unwrap();
    assert!(responses.contains_key("201"), "应声明 201 响应");
    assert!(responses.contains_key("400"), "应声明 400 响应");

    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let input = schemas
        .get("InstrumentCreateInput")
        .expect("应包含 InstrumentCreateInput schema");
    let props = input["properties"].as_object().unwrap();
    for field in ["symbol", "type", "name", "market", "currency_code"] {
        assert!(
            props.contains_key(field),
            "InstrumentCreateInput 应包含字段 {field}"
        );
    }
    let required = input["required"].as_array().unwrap();
    for field in ["symbol", "type"] {
        assert!(required.iter().any(|r| r == field), "{field} 应声明为必填");
    }
    assert!(
        !required.iter().any(|r| r == "currency_code"),
        "currency_code 应可缺省（按市场推导）"
    );
    // type 引用闭集枚举组件（五类小写字符串，与 GET 端点共享同一 InstrumentType schema）
    assert_eq!(
        props["type"]["$ref"], "#/components/schemas/InstrumentType",
        "type 字段应为 InstrumentType 闭集枚举引用"
    );
}
