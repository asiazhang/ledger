//! 股票按代码查询 HTTP 端点（`GET /api/v1/stocks/{code}`，issue #693 / ADR-0081
//! 决策 1）。
//!
//! 只断言外部行为：沪深港命中投影（权威名称/精确市场/币种/最新价/价格日期/类型
//! 提示，价格按万分之一元刻度）、6 位代码免传 market 推断沪深、5 位数字港股命中
//! 与补零归一、北交所与参数矛盾/无法推断 400 中文错误且不发起网络、查无此码 400、
//! 网络故障 500、开放 API 契约自描述覆盖。东财访问经注入桩离线驱动。

use std::collections::HashMap;

use axum::http::StatusCode;

use tauri_app_lib::investment::InstrumentType;

use crate::common::{
    StockStubHit, get_json, setup_app_with_stock_fetch, setup_app_with_stock_stub,
};

/// 命中表：沪 600519 贵州茅台 / 深 000001 平安银行 / 港 00700 腾讯控股 /
/// 沪 ETF 510300（类型提示 etf）。价格为万分之一元刻度。
fn stub_hits() -> HashMap<String, StockStubHit> {
    HashMap::from([
        (
            "sh/600519".to_string(),
            StockStubHit {
                name: "贵州茅台",
                price: Some((13_300_000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
            },
        ),
        (
            "sz/000001".to_string(),
            StockStubHit {
                name: "平安银行",
                price: Some((115_500, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
            },
        ),
        (
            "hk/00700".to_string(),
            StockStubHit {
                name: "腾讯控股",
                price: Some((4_428_000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
            },
        ),
        (
            "sh/510300".to_string(),
            StockStubHit {
                name: "沪深300ETF华泰柏瑞",
                price: Some((46_160, "2026-09-04")),
                kind_hint: InstrumentType::Etf,
            },
        ),
    ])
}

// ---------------------------------------------------------------------------
// 命中：权威名称 / 精确市场 / 币种 / 最新价 / 价格日期 / 类型提示
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_stock_returns_full_projection() {
    let (app, _conn, _calls) = setup_app_with_stock_stub(stub_hits());

    let (status, body) = get_json(&app, "/api/v1/stocks/600519").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], "600519");
    assert_eq!(body["name"], "贵州茅台", "应返回东财权威名称");
    assert_eq!(body["market"], "sh", "应返回精确市场");
    assert_eq!(body["currency_code"], "CNY", "沪市推导人民币");
    assert_eq!(
        body["price_cents"], 13300000,
        "最新价应按万分之一元刻度投影"
    );
    assert_eq!(body["price_date"], "2026-09-04", "应返回价格日期");
    assert_eq!(body["kind_hint"], "stock", "应返回类型提示");
}

#[tokio::test]
async fn test_lookup_stock_sz_hit_with_and_without_explicit_market() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    // 免传 market：6 位 0 开头推断深市。
    let (status, body) = get_json(&app, "/api/v1/stocks/000001").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["market"], "sz");
    assert_eq!(body["name"], "平安银行");
    assert_eq!(body["currency_code"], "CNY");

    // 显式 market 与推断一致：同样命中（显式传参不改变语义）。
    let (status, body) = get_json(&app, "/api/v1/stocks/000001?market=sz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["market"], "sz");

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("sz".to_string(), "000001".to_string()),
            ("sz".to_string(), "000001".to_string()),
        ],
        "两次查询均应以（深市，代码）发起东财请求"
    );
}

#[tokio::test]
async fn test_lookup_stock_etf_hint_projected() {
    let (app, _conn, _calls) = setup_app_with_stock_stub(stub_hits());

    let (status, body) = get_json(&app, "/api/v1/stocks/510300").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind_hint"], "etf", "类型提示应投影东财特征探测结果");
    assert_eq!(body["name"], "沪深300ETF华泰柏瑞");
    assert_eq!(body["market"], "sh", "场内基金仍按交易所落精确市场");
}

// ---------------------------------------------------------------------------
// 港股：5 位数字命中与短数字补零归一
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_stock_hk_five_digit_and_zero_padding() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    // 5 位数字港股：免传 market 命中，币种推导港币。
    let (status, body) = get_json(&app, "/api/v1/stocks/00700").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], "00700");
    assert_eq!(body["market"], "hk");
    assert_eq!(body["currency_code"], "HKD");
    assert_eq!(body["price_cents"], 4428000);

    // 短数字港股：左补零归一后命中同一标的，响应返回归一化代码。
    let (status, body) = get_json(&app, "/api/v1/stocks/700").await;
    assert_eq!(status, StatusCode::OK, "短数字应按港股补零命中: {body}");
    assert_eq!(body["code"], "00700", "响应应返回归一化代码");
    assert_eq!(body["market"], "hk");
    assert_eq!(body["name"], "腾讯控股");

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("hk".to_string(), "00700".to_string()),
            ("hk".to_string(), "00700".to_string()),
        ],
        "两次查询均应以补零归一后的代码发起东财请求"
    );
}

// ---------------------------------------------------------------------------
// 400 矩阵：北交所 / 参数矛盾 / 市场不支持 / 无法推断（均不发起网络）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_stock_beijing_exchange_code_rejected_without_network() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    for code in ["430047", "830799"] {
        let (status, err) = get_json(&app, &format!("/api/v1/stocks/{code}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code} 应 400");
        assert_eq!(err["code"], "stock.bse-unsupported", "{code} 错误码: {err}");
        assert!(
            err["message"].as_str().unwrap().contains("北交所"),
            "北交所错误应为中文明示，实际: {err}"
        );
        assert_eq!(err["kind"], "Invalid");
    }
    assert!(
        calls.lock().unwrap().is_empty(),
        "北交所拒绝应在发起网络请求前"
    );
}

#[tokio::test]
async fn test_lookup_stock_market_conflict_rejected_without_network() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    for uri in [
        "/api/v1/stocks/600519?market=sz", // 沪市形态传深市
        "/api/v1/stocks/000001?market=sh", // 深市形态传沪市
        "/api/v1/stocks/00700?market=sz",  // 港股形态传深市
        "/api/v1/stocks/600519?market=hk", // 沪市形态传港股
    ] {
        let (status, err) = get_json(&app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} 应 400");
        assert_eq!(err["code"], "stock.market-conflict", "{uri} 错误码: {err}");
        assert!(
            err["message"].as_str().unwrap().contains("矛盾"),
            "矛盾错误应为中文提示，实际: {err}"
        );
    }
    assert!(calls.lock().unwrap().is_empty(), "参数矛盾应不发起网络");
}

#[tokio::test]
async fn test_lookup_stock_unsupported_market_rejected() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    // 美股三市场属标的 market 闭集但本端点未开放查询（T4 落地）：显式 400 暂不支持。
    for market in ["nasdaq", "nyse", "amex"] {
        let (status, err) = get_json(&app, &format!("/api/v1/stocks/600519?market={market}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{market} 应 400");
        assert_eq!(
            err["code"], "stock.market-unsupported",
            "{market} 错误码: {err}"
        );
        assert!(
            err["message"].as_str().unwrap().contains("暂不支持"),
            "应中文说明暂不支持，实际: {err}"
        );
    }
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_lookup_stock_unresolvable_code_rejected_without_network() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    // 字母 ticker（美股遍历 T4 落地）与闭集外数字形态：缺省 market 无法推断。
    for code in ["AAPL", "900001", "1234567"] {
        let (status, err) = get_json(&app, &format!("/api/v1/stocks/{code}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code} 应 400");
        assert_eq!(
            err["code"], "stock.code-unresolvable",
            "{code} 错误码: {err}"
        );
        assert!(
            err["message"].as_str().unwrap().contains("market"),
            "应提示显式传 market，实际: {err}"
        );
    }
    assert!(calls.lock().unwrap().is_empty(), "无法推断应不发起网络");
}

// ---------------------------------------------------------------------------
// 查无此码 400 / 网络故障 500
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_stock_unknown_code_returns_400_chinese_error() {
    let (app, _conn, calls) = setup_app_with_stock_stub(stub_hits());

    // 699999：6 开头推断沪市、可发起查询，但东财查无此码。
    let (status, err) = get_json(&app, "/api/v1/stocks/699999").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(err["code"], "sync.stock-not-found");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("查无股票代码 699999"),
        "查无此码应返回中文错误，实际: {err}"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("sh".to_string(), "699999".to_string())],
        "查无此码路径应已发起一次东财查询"
    );
}

#[tokio::test]
async fn test_lookup_stock_network_failure_returns_500() {
    // 东财不可达桩：Io 错误上抛（与生产网络故障同形状），端点应 500。
    let (app, _conn) =
        setup_app_with_stock_fetch(Some(std::sync::Arc::new(|_market: &str, _code: &str| {
            Err(tauri_app_lib::error::AppError::Io("东财网络不可达".into()))
        })));

    let (status, err) = get_json(&app, "/api/v1/stocks/600519").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "网络故障应 500: {err}"
    );
    assert_eq!(err["kind"], "Io", "错误应为统一形状的 Io: {err}");
}

// ---------------------------------------------------------------------------
// 开放 API 契约自描述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_doc_covers_stock_lookup_endpoint() {
    let (app, _conn, _calls) = setup_app_with_stock_stub(stub_hits());
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let get_op = &doc["paths"]["/api/v1/stocks/{code}"]["get"];
    assert!(get_op["summary"].is_string(), "OpenAPI 应包含股票查询端点");
    let description = get_op["description"].as_str().unwrap_or_default();
    for expected in [
        "market",
        "推断",
        "kind_hint",
        "查无此码",
        "万分之一元",
        "北交所",
    ] {
        assert!(
            description.contains(expected),
            "端点自述应说明 {expected} 语义"
        );
    }

    let params = get_op["parameters"].as_array().expect("查询端点应声明参数");
    assert!(
        params
            .iter()
            .any(|p| p["name"] == "code" && p["in"] == "path"),
        "应声明 code 路径参数"
    );
    assert!(
        params
            .iter()
            .any(|p| p["name"] == "market" && p["in"] == "query"),
        "应声明 market 查询参数（可选）"
    );

    let responses = get_op["responses"].as_object().unwrap();
    assert!(responses.contains_key("200"), "应声明 200 响应");
    assert!(responses.contains_key("400"), "应声明 400 响应");
    assert!(responses.contains_key("500"), "应声明 500 响应");
    assert_eq!(
        responses["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/StockLookup",
        "200 响应体应为 StockLookup schema 引用"
    );

    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let lookup = schemas
        .get("StockLookup")
        .expect("应包含 StockLookup schema");
    let props = lookup["properties"].as_object().unwrap();
    for field in [
        "code",
        "name",
        "market",
        "currency_code",
        "price_cents",
        "price_date",
        "kind_hint",
    ] {
        assert!(props.contains_key(field), "StockLookup 应包含字段 {field}");
    }
}
