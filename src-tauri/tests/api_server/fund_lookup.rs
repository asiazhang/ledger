//! 基金按代码查询 HTTP 端点（`GET /api/v1/funds/{code}`，issue #304 / ADR-0039 决策 2）。
//!
//! 只断言外部行为：命中返回名称/东财分类/最新净值/净值日期（净值按 API 价格刻度
//! 万分之一元投影）、净值未公布为 null、格式非法（非 6 位）不发起网络请求即 400、
//! 查无此码 400 中文错误、开放 API 契约自描述覆盖。东财访问经注入桩离线驱动。

use std::collections::HashMap;

use axum::http::StatusCode;

use crate::common::{FundStubHit, get_json, setup_app_with_fund_stub};

/// 命中表：000001 → 华夏成长混合 / 混合型-灵活 / 净值 1.2345 元 @ 2026-08-28。
fn stub_hit() -> HashMap<String, FundStubHit> {
    HashMap::from([(
        "000001".to_string(),
        FundStubHit {
            name: "华夏成长混合",
            fund_class: "混合型-灵活",
            nav: Some((1.2345, "2026-08-28")),
        },
    )])
}

// ---------------------------------------------------------------------------
// 命中：名称 / 类型 / 最新净值 / 净值日期（净值按万分之一元刻度投影）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_fund_returns_name_class_nav_and_nav_date() {
    let (app, _conn, _calls) = setup_app_with_fund_stub(stub_hit());

    let (status, body) = get_json(&app, "/api/v1/funds/000001").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], "000001");
    assert_eq!(body["name"], "华夏成长混合", "应返回东财权威名称");
    assert_eq!(body["fund_class"], "混合型-灵活", "应返回东财基金分类");
    assert_eq!(
        body["nav_cents"], 12345,
        "净值 1.2345 元应投影为万分之一元刻度 12345"
    );
    assert_eq!(body["nav_date"], "2026-08-28");
}

#[tokio::test]
async fn test_lookup_fund_with_unpublished_nav_returns_null_nav_fields() {
    let hits = HashMap::from([(
        "012345".to_string(),
        FundStubHit {
            name: "新发基金",
            fund_class: "混合型",
            nav: None,
        },
    )]);
    let (app, _conn, _calls) = setup_app_with_fund_stub(hits);

    let (status, body) = get_json(&app, "/api/v1/funds/012345").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "新发基金");
    assert!(
        body["nav_cents"].is_null() && body["nav_date"].is_null(),
        "未公布净值时 nav_cents/nav_date 应为 null: {body}"
    );
}

// ---------------------------------------------------------------------------
// 错误：格式非法（不发起网络请求）与查无此码，均为 400 中文错误
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_fund_with_malformed_code_rejects_without_network() {
    let (app, _conn, calls) = setup_app_with_fund_stub(stub_hit());

    for bad in ["12345", "1234567", "1234A6", "abcdef"] {
        let (status, err) = get_json(&app, &format!("/api/v1/funds/{bad}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} 应 400");
        assert_eq!(err["kind"], "Invalid", "{bad} 错误应为统一形状: {err}");
        assert!(
            err["message"]
                .as_str()
                .unwrap()
                .contains("基金代码须为 6 位数字"),
            "{bad} 错误信息应为中文格式提示，实际: {err}"
        );
    }
    assert!(
        calls.lock().unwrap().is_empty(),
        "格式非法应在发起网络请求前拒绝"
    );
}

#[tokio::test]
async fn test_lookup_fund_with_unknown_code_returns_400_chinese_error() {
    let (app, _conn, calls) = setup_app_with_fund_stub(stub_hit());

    let (status, body) = get_json(&app, "/api/v1/funds/999999").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["kind"], "Invalid");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("查无基金代码 999999"),
        "查无此码应返回中文错误，实际: {body}"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["999999".to_string()],
        "查无此码路径应已发起一次东财查询"
    );
}

// ---------------------------------------------------------------------------
// 开放 API 契约自描述
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openapi_doc_covers_fund_lookup_endpoint() {
    let (app, _conn, _calls) = setup_app_with_fund_stub(stub_hit());
    let (_, doc) = crate::common::get_json(&app, "/api/v1/openapi.json").await;

    let get_op = &doc["paths"]["/api/v1/funds/{code}"]["get"];
    assert!(get_op["summary"].is_string(), "OpenAPI 应包含基金查询端点");
    let description = get_op["description"].as_str().unwrap_or_default();
    for expected in ["6 位", "nav_cents", "nav_date", "fund_class", "查无此码"] {
        assert!(
            description.contains(expected),
            "端点自述应说明 {expected} 语义"
        );
    }

    let params = get_op["parameters"]
        .as_array()
        .expect("查询端点应声明 path 参数");
    assert!(
        params.iter().any(|p| p["name"] == "code"),
        "查询端点应声明 code 路径参数"
    );

    let responses = get_op["responses"].as_object().unwrap();
    assert!(responses.contains_key("200"), "应声明 200 响应");
    assert!(responses.contains_key("400"), "应声明 400 响应");
    assert_eq!(
        responses["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/FundLookup",
        "200 响应体应为 FundLookup schema 引用"
    );

    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let lookup = schemas.get("FundLookup").expect("应包含 FundLookup schema");
    let props = lookup["properties"].as_object().unwrap();
    for field in ["code", "name", "fund_class", "nav_cents", "nav_date"] {
        assert!(props.contains_key(field), "FundLookup 应包含字段 {field}");
    }
}
