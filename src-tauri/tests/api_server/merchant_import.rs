use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{
    batch_body, body_to_bytes, count_active_transactions, count_rows, create_account_via_api,
    get_json, post_batch, put_merchant_via_api, put_transaction_via_api, setup_app,
};

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
    assert!(list[0].get("icon").is_none(), "商户响应不应含 icon 字段");
    assert!(list[0].get("color").is_none(), "商户响应不应含 color 字段");
}

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
    assert_eq!(items[0]["merchant_id"], items[1]["merchant_id"]);
    assert_eq!(items[1]["merchant_id"], items[2]["merchant_id"]);
}

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

#[tokio::test]
async fn test_batch_import_transfer_with_merchant_name_attaches_merchant() {
    let (app, conn) = setup_app();
    let from = create_account_via_api(&app, "现金账户").await;
    let to = create_account_via_api(&app, "银行账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-08-01","merchant_name":"魏有鼎"}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(
        results[0]["success"], true,
        "transfer 携带商户应成功: {results:?}"
    );
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "merchants"),
        1,
        "按名即建商户"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let merchant_id = items[0]["merchant_id"].as_str().expect("转账行应携带商户");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let hit = merchants
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == merchant_id)
        .expect("转账行的商户应在商户列表中");
    assert_eq!(hit["name"], "魏有鼎");
}

#[tokio::test]
async fn test_batch_import_transfer_reuses_merchant_by_name() {
    let (app, conn) = setup_app();
    let from = create_account_via_api(&app, "现金账户").await;
    let to = create_account_via_api(&app, "银行账户").await;

    let results = post_batch(
        &app,
        batch_body(
            &[
                &format!(
                    r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-08-01","merchant_name":"魏有鼎","idempotency_key":"k1"}}"#
                ),
                &format!(
                    r#"{{"kind":"transfer","amount_cents":2000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-08-02","merchant_name":"魏有鼎","idempotency_key":"k2"}}"#
                ),
            ],
            None,
        ),
    )
    .await;
    assert!(results.iter().all(|r| r["success"] == true));
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "merchants"),
        1,
        "同名复用不分裂"
    );
}

#[tokio::test]
async fn test_update_transfer_with_merchant_name_attaches_merchant() {
    let (app, conn) = setup_app();
    let from = create_account_via_api(&app, "现金账户").await;
    let to = create_account_via_api(&app, "银行账户").await;

    let created = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-08-01"}}"#
            )],
            None,
        ),
    )
    .await;
    let txn_id = created[0]["id"].as_str().unwrap();

    let body = format!(
        r#"{{"kind":"transfer","amount_cents":1500,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-08-01","merchant_name":"魏有鼎"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, txn_id, &body).await;
    assert_eq!(status, StatusCode::OK, "PUT transfer 带商户应成功");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let merchant_id = updated["merchant_id"].as_str().expect("应携带商户引用");
    assert_eq!(count_rows(&conn.lock().unwrap(), "merchants"), 1);

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    assert_eq!(merchants.as_array().unwrap()[0]["id"], merchant_id);
}

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

#[tokio::test]
async fn test_update_transaction_with_merchant_name_creates_then_reuses() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

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

#[tokio::test]
async fn test_put_merchant_renames() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    post_batch(
        &app,
        batch_body(
            &[&expense_with_merchant(
                &account_id,
                "2026-08-01",
                "京东商城-京东白条",
                None,
            )],
            None,
        ),
    )
    .await;

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let merchant_id = merchants.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, txs_before) = get_json(&app, "/api/v1/transactions").await;
    let tx_merchant_id = txs_before["items"][0]["merchant_id"].clone();

    let (status, bytes) = put_merchant_via_api(&app, &merchant_id, r#"{"name":"京东"}"#).await;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["id"], merchant_id, "返回更新后完整商户");
    assert_eq!(updated["name"], "京东");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let list = merchants.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "京东");

    let (_, txs) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(
        txs["items"][0]["merchant_id"], tx_merchant_id,
        "改名不迁移交易引用（ADR-0028）"
    );
}

#[tokio::test]
async fn test_put_merchant_name_omitted_keeps_original() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
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
    let merchant_id = merchants.as_array().unwrap()[0]["id"].as_str().unwrap();

    let (status, bytes) = put_merchant_via_api(&app, merchant_id, r#"{}"#).await;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["name"], "盒马", "name 省略即保持原值");
}

#[tokio::test]
async fn test_put_merchant_unknown_id_returns_coded_404() {
    let (app, _) = setup_app();
    let (status, bytes) =
        put_merchant_via_api(&app, "no-such-merchant", r#"{"name":"京东"}"#).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert_eq!(err["code"], "merchant.not-found");
    assert_eq!(err["message"], "商户不存在: no-such-merchant");
}

#[tokio::test]
async fn test_put_merchant_collision_returns_coded_400() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    post_batch(
        &app,
        batch_body(
            &[
                &expense_with_merchant(&account_id, "2026-08-01", "盒马", Some("k1")),
                &expense_with_merchant(&account_id, "2026-08-02", "永辉", Some("k2")),
            ],
            None,
        ),
    )
    .await;
    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    let yonghui_id = merchants
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "永辉")
        .expect("应包含永辉")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, bytes) = put_merchant_via_api(&app, &yonghui_id, r#"{"name":"盒马"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(err["code"], "merchant.already-exists");
    assert_eq!(err["message"], "商户已存在: 盒马");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    assert_eq!(merchants.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_put_merchant_trims_name_and_rejects_blank() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
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
    let merchant_id = merchants.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, bytes) = put_merchant_via_api(&app, &merchant_id, r#"{"name":"  京东  "}"#).await;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["name"], "京东", "入库前修剪首尾空白");

    let (status, bytes) = put_merchant_via_api(&app, &merchant_id, r#"{"name":"   "}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "merchant.name-required");

    let (_, merchants) = get_json(&app, "/api/v1/merchants").await;
    assert_eq!(
        merchants.as_array().unwrap()[0]["name"],
        "京东",
        "失败改名原名保持"
    );
}
