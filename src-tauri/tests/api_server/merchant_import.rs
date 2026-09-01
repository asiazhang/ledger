//! AI 导入商户契约（issue #194 / ADR-0028）：
//! - `GET /api/v1/merchants` 暴露在用商户列表（供 AI 复用已有商户）；
//! - 交易提交体接受 `merchant_name`（商户名字符串）：后端精确匹配在用商户名，
//!   命中复用、未命中即建——归一化责任收口在后端，AI 不负责商户去重；
//! - 幂等重放不产生重复交易、不产生碎商户。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{
    batch_body, body_to_bytes, count_active_transactions, count_rows, create_account_via_api,
    get_json, post_batch, put_transaction_via_api, setup_app,
};

/// 批量导入一行带商户名的支出。
fn expense_with_merchant(
    account_id: &str,
    date: &str,
    merchant_name: &str,
    key: Option<&str>,
) -> String {
    let key_part = match key {
        Some(k) => format!(r#","idempotency_key":"{k}""#),
        None => String::new(),
    };
    format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"{date}","merchant_name":"{merchant_name}"{key_part}}}"#
    )
}

/// 商户列表端点：空表启动不 seed（ADR-0028），导入带商户名后出现在列表。
#[tokio::test]
async fn test_get_merchants_lists_merchants_created_by_import() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let (status, merchants) = get_json(&app, "/api/v1/merchants").await;
    assert_eq!(status, StatusCode::OK);
    assert!(merchants.as_array().unwrap().is_empty(), "空表启动不 seed");

    post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let list = merchants.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "盒马");
    assert_eq!(list[0]["is_deleted"], false);
    // 商户契约回归「名字字典」（issue #223）：响应不含已退役的 icon/color 字段。
    assert!(list[0].get("icon").is_none(), "商户响应不应含 icon 字段");
    assert!(list[0].get("color").is_none(), "商户响应不应含 color 字段");
}

/// 带商户名导入落库：交易行解析出 merchant_id，读回可按商户关联。
#[tokio::test]
async fn test_import_with_merchant_name_attaches_merchant() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], true);

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let merchant_id = items[0]["merchant_id"].as_str().expect("应携带商户");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let hit = merchants
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == merchant_id)
        .expect("交易的商户应在商户列表中");
    assert_eq!(hit["name"], "盒马");
}

/// 同批多行同名只建一个商户（未命中即建、命中复用，同批不分裂）。
#[tokio::test]
async fn test_same_merchant_name_in_one_batch_creates_single_merchant() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[
                &expense_with_merchant(&account_id, "2026-08-01", "盒马", Some("k1")),
                &expense_with_merchant(&account_id, "2026-08-02", "盒马", Some("k2")),
                &expense_with_merchant(&account_id, "2026-08-03", "  盒马  ", Some("k3")),
            ],
            None,
        ),
    )
    .await;
    assert!(results.iter().all(|r| r["success"] == true));

    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 1);
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    // 三行指向同一商户（trim 后精确匹配）。
    assert_eq!(items[0]["merchant_id"], items[1]["merchant_id"]);
    assert_eq!(items[1]["merchant_id"], items[2]["merchant_id"]);
}

/// 跨批复用：第二次导入同名命中第一次建的商户（AI 不负责去重）。
#[tokio::test]
async fn test_later_import_reuses_existing_merchant_by_name() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let first = post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;
    assert_eq!(first[0]["success"], true);
    let first_merchant_id: String = {
        let (_, list) = get_json(&app, "/api/v1/transactions").await;
        list["items"][0]["merchant_id"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let second = post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-15",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;
    assert_eq!(second[0]["success"], true);
    assert!(second[0]["id"].as_str().is_some(), "不同交易应新写入");
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 1);

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1]["merchant_id"], first_merchant_id, "复用已有商户");
}

/// 幂等重放：同批带幂等键重跑全部去重跳过，不产生重复交易、不产生碎商户。
#[tokio::test]
async fn test_idempotent_replay_no_merchant_fragmentation() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = batch_body(
        &[
            &expense_with_merchant(&account_id, "2026-08-01", "盒马", Some("row:1")),
            &expense_with_merchant(&account_id, "2026-08-02", "盒马", Some("row:2")),
        ],
        None,
    );
    let first = post_batch(&app, body.clone()).await;
    assert!(
        first
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == false)
    );

    let second = post_batch(&app, body).await;
    assert!(
        second
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == true && r["id"].is_string()),
        "重跑应全部按幂等键命中已有交易"
    );

    assert_eq!(count_active_transactions(&conn.lock().unwrap()), 2);
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 1);
}

/// transfer 携带商户名被行为层拒绝，且**不**先建商户（kind 收口在解析之前）。
#[tokio::test]
async fn test_transfer_with_merchant_name_rejected_without_creating_merchant() {
    let (app, conn) = setup_app();

    let results = post_batch(
        &app,
        batch_body(
            &[r#"{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"x","to_account_id":"y","date":"2026-08-01","merchant_name":"盒马"}"#],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], false);
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("不能携带商户"),
        "应报「不能携带商户」，实际: {results:?}"
    );
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 0);
    assert_eq!(count_active_transactions(&conn.lock().unwrap()), 0);
}

/// `merchant_id` 与 `merchant_name` 同时提供属请求错误（歧义，逐条校验失败不影响其他行）。
#[tokio::test]
async fn test_merchant_id_and_name_mutually_exclusive() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-08-01","merchant_id":"m1","merchant_name":"盒马"}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], false);
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("不可同时提供"),
        "应报「不可同时提供」，实际: {results:?}"
    );
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 0);
    assert_eq!(count_active_transactions(&conn.lock().unwrap()), 0);
}

/// 商户名为空白 → 逐条校验失败（不落库、不建商户）。
#[tokio::test]
async fn test_blank_merchant_name_rejected() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-08-01","merchant_name":"   "}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], false);
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("商户名不能为空"),
        "应报「商户名不能为空」，实际: {results:?}"
    );
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 0);
    assert_eq!(count_active_transactions(&conn.lock().unwrap()), 0);
}

/// 导入知识同步覆盖商户约定：`merchant_name` 携带、复用指引、kind 范围与幂等不碎。
#[tokio::test]
async fn test_import_knowledge_covers_merchant_convention() {
    let (app, _) = setup_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/knowledge")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    let knowledge = String::from_utf8(bytes).unwrap();
    assert!(knowledge.contains("merchant_name"), "应说明商户名字段");
    assert!(
        knowledge.contains("/api/v1/merchants"),
        "应指引用商户列表端点"
    );
    assert!(knowledge.contains("未命中"), "应说明未命中即建");
}

/// 校验失败的行**不**建商户：未命中名字的即建推迟到行内校验全部通过之后，
/// 金额非法的行报 success:false 且不残留无引用商户（不碎商户）。
#[tokio::test]
async fn test_invalid_row_with_merchant_name_creates_no_merchant() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"expense","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-08-01","merchant_name":"盒马"}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], false);
    assert!(results[0]["error"].as_str().unwrap().contains("大于 0"));
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 0);
    assert_eq!(count_active_transactions(&conn.lock().unwrap()), 0);
}

/// refund 携带商户名被忽略（继承原支出商户），不解析、不即建——否则即建商户必成孤儿。
#[tokio::test]
async fn test_refund_with_merchant_name_inherits_and_creates_no_merchant() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let seed = post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;
    let expense_id = seed[0]["id"].as_str().unwrap().to_string();

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"refund","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-08-05","refund_of_transaction_id":"{expense_id}","merchant_name":"永辉"}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], true, "退款应成功: {results:?}");
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "merchants"),
        1,
        "不新建商户"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let refund = items
        .iter()
        .find(|t| t["kind"] == "refund")
        .expect("应包含退款");
    assert_eq!(
        refund["merchant_id"], items[1]["merchant_id"],
        "退款继承原支出商户"
    );
}

/// 修改路径（PUT）同样接受商户名：解析出的 id 与该行当前商户相同即保持历史引用。
#[tokio::test]
async fn test_update_transaction_with_merchant_name() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let created = post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "盒马",
                None,
            )],
            None,
        ),
    )
    .await;
    let txn_id = created[0]["id"].as_str().unwrap().to_string();

    // 改名指向已有商户（精确匹配复用）。
    let update_body = format!(
        r#"{{"kind":"expense","amount_cents":1200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-08-01","merchant_name":"永辉"}}"#
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/transactions/{txn_id}"))
                .header("content-type", "application/json")
                .body(Body::from(update_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let list = merchants.as_array().unwrap();
    assert_eq!(list.len(), 2, "新建「永辉」，「盒马」保留");
    let updated: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
    let hit = list
        .iter()
        .find(|m| m["id"] == updated["merchant_id"])
        .expect("修改后的交易应指向解析出的商户");
    assert_eq!(hit["name"], "永辉");
}

/// 更新端点同走商户名归一化（行为层同一入口）：改携带新商户名 → 200 且商户即建并
/// 进列表；再改命中名字 → 复用（商户数不变）。`app: None`（集成测试）跳过信号发射
/// 分支，写路径语义不变（issue #331 两壳接线，ADR-0044）。
#[tokio::test]
async fn test_update_transaction_with_merchant_name_creates_then_reuses() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

    // 改为携带新商户名：200，交易引用即建的商户，商户列表出现该行。
    let body = format!(
        r#"{{"kind":"expense","amount_cents":900,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-10","merchant_name":"盒马"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, id, &body).await;
    assert_eq!(status, StatusCode::OK, "PUT 即建商户应成功");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let merchant_id = updated["merchant_id"].as_str().expect("应携带商户引用");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let list = merchants.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], merchant_id);
    assert_eq!(list[0]["name"], "盒马");

    // 再改为命中名字「盒马」：复用既有商户，列表仍一行。
    let body = format!(
        r#"{{"kind":"expense","amount_cents":900,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-11","merchant_name":"盒马"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, id, &body).await;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["merchant_id"], merchant_id, "命中复用同一商户");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    assert_eq!(merchants.as_array().unwrap().len(), 1, "复用不新建");
}
