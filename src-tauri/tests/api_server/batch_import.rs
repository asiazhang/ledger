use axum::body::Body;
use axum::http::{Request, StatusCode};
use tauri_app_lib::test_support;
use tower::ServiceExt;

use crate::common::{
    batch_body, body_to_bytes, count_active_transactions, create_account_via_api,
    delete_transaction_via_api, get_first_category_id, get_json, post_batch,
    put_transaction_via_api, setup_app,
};

#[tokio::test]
async fn test_batch_create_transactions_all_success() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}},
                {{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}},
                {{"kind":"income","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}
            ]
        }}"#
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let results: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r["success"], true);
        assert_eq!(r["duplicate"], false);
        assert!(!r["id"].as_str().unwrap_or("").is_empty());
    }
}

#[tokio::test]
async fn test_batch_create_transactions_partial_failure() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}},
                {{"kind":"income","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}},
                {{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}},
                {{"kind":"transfer","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-04"}}
            ]
        }}"#
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let results: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["success"], true);
    assert_eq!(results[0]["duplicate"], false);
    assert_eq!(results[1]["success"], false);
    assert!(results[1]["error"].as_str().unwrap().contains("大于 0"));
    assert_eq!(results[2]["success"], true);
    assert_eq!(results[3]["success"], false);
    assert!(results[3]["error"].as_str().unwrap().contains("目标账户"));

    let count: i64 = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_batch_create_transactions_invalid_json_returns_400() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from("not-json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_kind_enum_rejects_unknown_at_api_boundary() {
    let (app, _) = setup_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(batch_body(
                    &[r#"{"kind":"bonus","amount_cents":100,"currency_code":"CNY","account_id":"a","date":"2026-01-01"}"#],
                    Some(true),
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "非法 kind 应整批 4xx，实际: {}",
        response.status()
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/transactions?kinds=bonus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "非法 kinds 过滤参数应 4xx，实际: {}",
        response.status()
    );
}

#[tokio::test]
async fn test_batch_create_dividend_persists_cash_leg_and_instrument_link() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "银行卡").await;
    test_support::seed_instrument(
        &conn.lock().unwrap(),
        "inst-div-1078",
        "502010",
        "证券基金",
        "CNY",
        "unknown",
    );

    let body = batch_body(
        &[&format!(
            r#"{{"kind":"dividend","amount_cents":3000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04","instrument_id":"inst-div-1078"}}"#
        )],
        None,
    );
    let results = post_batch(&app, body).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["success"], true, "dividend 应落库: {results:?}");
    assert_eq!(results[0]["duplicate"], false);

    let list = get_json(&app, "/api/v1/transactions").await;
    let items = list.1["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "dividend");
    assert_eq!(items[0]["amount_cents"], 3000);
    assert_eq!(items[0]["source"]["kind"], "instrument");
    assert_eq!(items[0]["source"]["entity_id"], "inst-div-1078");
}

#[tokio::test]
async fn test_batch_create_dividend_guards_return_readable_errors() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "银行卡").await;

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"dividend","amount_cents":3000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04"}}"#
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
            .contains("分红必须指定标的"),
        "缺标的应返回可读中文错误，实际: {results:?}"
    );

    let results = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"dividend","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04","instrument_id":"inst-x"}}"#
            )],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], false);

    assert_eq!(
        count_active_transactions(&conn.lock().unwrap()),
        0,
        "被拒绝的 dividend 不应落库"
    );
}

#[tokio::test]
async fn test_batch_buy_sell_with_missing_instrument_rejected_with_readable_error() {
    let (app, conn) = setup_app();
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-295",
        "证券账户",
        "investment",
        "CNY",
        0,
    );
    let account_id = "acc-inv-295";

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-01","instrument_id":"inst-not-exist","quantity":10.0,"price_cents":1500,"fee_cents":0}},
                {{"kind":"sell","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-02","instrument_id":"inst-not-exist","quantity":5.0,"price_cents":1600,"fee_cents":0}}
            ]
        }}"#
    );

    let results = post_batch(&app, body).await;
    assert_eq!(results.len(), 2);
    for r in &results {
        assert_eq!(r["success"], false, "引用不存在标的应逐行失败: {r}");
        assert_eq!(r["duplicate"], false);
        let err = r["error"].as_str().unwrap();
        assert!(err.contains("标的不存在"), "应报标的不存在供回自纠: {r}");
        assert!(!err.contains("数据库错误"), "不应再报外键数据库错误: {r}");
    }

    let count = conn.lock().unwrap();
    assert_eq!(
        count_active_transactions(&count),
        0,
        "被拒的 buy/sell 不应落库"
    );
}

#[tokio::test]
async fn test_update_transaction_to_dividend_rejected_by_kind_change_guard() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let created = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-01"}}"#
            )],
            None,
        ),
    )
    .await;
    let txn_id = created[0]["id"].as_str().unwrap().to_string();

    let update_body = format!(
        r#"{{"kind":"dividend","amount_cents":60,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04"}}"#
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
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = body_to_bytes(response.into_body()).await;
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.dividend-kind-change-forbidden", "{err}");
    assert!(
        err["message"].as_str().unwrap().contains("分红"),
        "拒绝文案应对准分红，实际: {err}"
    );

    let list = get_json(&app, "/api/v1/transactions").await;
    let items = list.1["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "expense");
}

#[tokio::test]
async fn test_update_dividend_in_place_updates_amount() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "银行卡").await;
    test_support::seed_instrument(
        &conn.lock().unwrap(),
        "inst-div-upd",
        "502010",
        "证券基金",
        "CNY",
        "unknown",
    );
    let created = post_batch(
        &app,
        batch_body(
            &[&format!(
                r#"{{"kind":"dividend","amount_cents":3000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04","instrument_id":"inst-div-upd"}}"#
            )],
            None,
        ),
    )
    .await;
    let txn_id = created[0]["id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/transactions/{txn_id}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"kind":"dividend","amount_cents":5000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04","instrument_id":"inst-div-upd"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let list = get_json(&app, "/api/v1/transactions").await;
    let items = list.1["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "dividend");
    assert_eq!(items[0]["amount_cents"], 5000);
    assert_eq!(items[0]["source"]["kind"], "instrument");
    assert_eq!(items[0]["source"]["entity_id"], "inst-div-upd");
}

#[tokio::test]
async fn test_batch_same_batch_twice_marks_all_duplicates_and_keeps_row_count() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx1 = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let tx2 = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx1, &tx2], None)).await;
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == false)
    );

    let second = post_batch(&app, batch_body(&[&tx1, &tx2], None)).await;
    assert_eq!(second.len(), 2);
    assert!(
        second
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == true && r["id"].is_null()),
        "第二次导入应全部命中重复"
    );

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 2, "重复导入不应增加库中行数");
}

#[tokio::test]
async fn test_batch_dedup_false_writes_duplicates() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(first[0]["duplicate"], false);

    let second = post_batch(&app, batch_body(&[&tx], Some(false))).await;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["duplicate"], false);
    assert!(
        !second[0]["id"].as_str().unwrap_or("").is_empty(),
        "dedup=false 应重复写入"
    );

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_batch_dedup_ignores_note_and_category_change() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let category_id = get_first_category_id(&app).await;

    let base = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let with_note = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02","note":"改了备注"}}"#
    );
    let with_category = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02","category_id":"{category_id}"}}"#
    );

    post_batch(&app, batch_body(&[&base], None)).await;
    let second = post_batch(&app, batch_body(&[&with_note], None)).await;
    assert_eq!(second[0]["duplicate"], true, "仅改备注应命中重复");
    let third = post_batch(&app, batch_body(&[&with_category], None)).await;
    assert_eq!(third[0]["duplicate"], true, "仅改分类应命中重复");

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_batch_dedup_not_hit_when_amount_account_date_change() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let other_account_id = create_account_via_api(&app, "另一账户").await;

    let base = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let diff_amount = format!(
        r#"{{"kind":"expense","amount_cents":600,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let diff_account = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{other_account_id}","date":"2026-07-02"}}"#
    );
    let diff_date = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}"#
    );

    post_batch(&app, batch_body(&[&base], None)).await;

    for changed in [&diff_amount, &diff_account, &diff_date] {
        let result = post_batch(&app, batch_body(&[changed], None)).await;
        assert_eq!(
            result[0]["duplicate"], false,
            "改金额/账户/日期不应命中重复"
        );
        assert!(!result[0]["id"].as_str().unwrap_or("").is_empty());
    }
}

#[tokio::test]
async fn test_batch_dedup_soft_deleted_then_reimport_writes_again() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = first[0]["id"].as_str().unwrap().to_string();

    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE transactions SET is_deleted=1 WHERE id=?1",
            rusqlite::params![id],
        )
        .unwrap();
    }

    let second = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(second[0]["duplicate"], false, "软删除后重跑应重新写入");
    assert!(!second[0]["id"].as_str().unwrap_or("").is_empty());

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_batch_dedup_keeps_dedup_hash_unchanged() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    post_batch(&app, batch_body(&[&tx], None)).await;

    let hash: Option<String> = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT dedup_hash FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(hash.is_some(), "导入后应写入 dedup_hash");

    let second = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(second[0]["duplicate"], true);
    let hash_after: Option<String> = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT dedup_hash FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(hash, hash_after, "dedup_hash 导入后保持不变");
}

#[tokio::test]
async fn test_batch_create_convert_unpacks_leg_fields_and_lands_carry() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-api", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(&conn, "inst-cv-out", "006793", "转出基金", "CNY", "unknown");
        test_support::seed_instrument(&conn, "inst-cv-in", "519700", "转入基金", "CNY", "unknown");
    }

    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-api","date":"2026-01-10","instrument_id":"inst-cv-out","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");

    let convert = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-api","date":"2026-02-01","instrument_id":"inst-cv-out","quantity":10.0,"to_instrument_id":"inst-cv-in","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let results = post_batch(&app, batch_body(&[convert], None)).await;
    assert_eq!(results[0]["success"], true, "转换应落账: {results:?}");
    assert_eq!(results[0]["duplicate"], false);

    let convert_id = results[0]["id"].as_str().unwrap().to_string();
    let (kind, amount_cents): (String, i64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT kind, amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![convert_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(kind, "convert");
    assert_eq!(amount_cents, 1000, "行金额锚点 = FIFO 结转成本");

    let (to_instrument, to_quantity, out_amount, in_amount): (String, f64, i64, i64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT to_instrument_id, to_quantity, out_amount_cents, in_amount_cents \
             FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![convert_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    };
    assert_eq!(to_instrument, "inst-cv-in");
    assert!((to_quantity - 10.0).abs() < 1e-9);
    assert_eq!(out_amount, 1100);
    assert_eq!(in_amount, 1100);

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    let convert_row = items
        .iter()
        .find(|row| row["kind"] == "convert")
        .expect("列表应含转换行");
    assert_eq!(convert_row["convert"]["out_amount_cents"], 1100);
    assert_eq!(convert_row["convert"]["to_instrument_id"], "inst-cv-in");
    let buy_row = items
        .iter()
        .find(|row| row["kind"] == "buy")
        .expect("列表应含买入行");
    assert!(buy_row["convert"].is_null(), "非转换行 convert 应为 null");
}

#[tokio::test]
async fn test_batch_create_convert_guards_return_coded_errors() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-guard", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-cv-g-out",
            "006793",
            "转出基金",
            "CNY",
            "unknown",
        );
        test_support::seed_instrument(
            &conn,
            "inst-cv-g-in",
            "519700",
            "转入基金",
            "CNY",
            "unknown",
        );
    }

    let missing_to = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-guard","date":"2026-02-01","instrument_id":"inst-cv-g-out","quantity":1.0,"out_amount_cents":100,"in_amount_cents":100}"#;
    let same_instrument = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-guard","date":"2026-02-02","instrument_id":"inst-cv-g-out","quantity":1.0,"to_instrument_id":"inst-cv-g-out","to_quantity":1.0,"out_amount_cents":100,"in_amount_cents":100}"#;
    let oversell = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-guard","date":"2026-02-03","instrument_id":"inst-cv-g-out","quantity":5.0,"to_instrument_id":"inst-cv-g-in","to_quantity":5.0,"out_amount_cents":500,"in_amount_cents":500}"#;

    let results = post_batch(
        &app,
        batch_body(&[missing_to, same_instrument, oversell], None),
    )
    .await;
    assert_eq!(results.len(), 3);
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("转换必须指定转入标的"),
        "{:?}",
        results[0]
    );
    assert!(
        results[1]["error"]
            .as_str()
            .unwrap()
            .contains("转出标的与转入标的不能相同"),
        "{:?}",
        results[1]
    );
    assert!(
        results[2]["error"]
            .as_str()
            .unwrap()
            .contains("可卖出数量不足"),
        "{:?}",
        results[2]
    );
    for r in &results {
        assert_eq!(r["success"], false);
    }

    let conn = conn.lock().unwrap();
    assert_eq!(count_active_transactions(&conn), 0, "被拒的转换不应落库");
    let conversions: i64 = conn
        .query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(conversions, 0);
}

#[tokio::test]
async fn test_batch_create_split_adjusts_holdings_and_keeps_balances() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-split-api", "股票户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-split-api",
            "502010",
            "证券基金",
            "CNY",
            "unknown",
        );
    }

    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-split-api","date":"2026-01-10","instrument_id":"inst-split-api","quantity":100.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");

    let (_, balances_before) = get_json(&app, "/api/v1/accounts/balances").await;

    let split = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-split-api","date":"2026-02-01","instrument_id":"inst-split-api","quantity":50.0}"#;
    let results = post_batch(&app, batch_body(&[split], None)).await;
    assert_eq!(results[0]["success"], true, "份额调整应落账: {results:?}");
    assert_eq!(results[0]["duplicate"], false);
    let split_id = results[0]["id"].as_str().unwrap().to_string();

    let (kind, amount_cents, amount_native_cents): (String, i64, i64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT kind, amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(kind, "split");
    assert_eq!(amount_cents, 0, "split 无现金腿：行金额恒 0");
    assert_eq!(amount_native_cents, 0);

    let (action, quantity, price_cents): (String, f64, Option<i64>) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT action, quantity, price_cents FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(action, "split");
    assert!((quantity - 50.0).abs() < 1e-9);
    assert_eq!(price_cents, None, "split 行 price_cents 留 NULL");

    let (holding_qty, cost_basis): (f64, i64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings \
             WHERE account_id='acc-split-api' AND instrument_id='inst-split-api'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert!(
        (holding_qty - 150.0).abs() < 1e-6,
        "持仓应为 100 + 50 = 150，实际 {holding_qty}"
    );
    assert!(
        (cost_basis - 10000).abs() <= 1,
        "批次总成本展示值应与调整前一致（亚分舍入 ±1 分），实际 {cost_basis}"
    );

    let (_, balances_after) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(
        balances_before, balances_after,
        "split 落账前后全部账户余额应完全不变"
    );

    let sell = r#"{"kind":"sell","amount_cents":0,"currency_code":"CNY","account_id":"acc-split-api","date":"2026-03-01","instrument_id":"inst-split-api","quantity":150.0,"price_cents":10000,"fee_cents":0}"#;
    let sold = post_batch(&app, batch_body(&[sell], None)).await;
    assert_eq!(
        sold[0]["success"], true,
        "重述后的批次应可全额清仓: {sold:?}"
    );
    let realized: i64 = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales s \
             JOIN transactions t ON t.id = s.sell_transaction_id \
             WHERE t.account_id='acc-split-api' AND s.lot_id IN \
             (SELECT id FROM security_lots WHERE instrument_id='inst-split-api')",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        realized, 5000,
        "Σ已实现应为 15000 − 10000 = 5000 分（成本总量不因 split 漂移）"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions?kinds=split").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "按 kinds=split 读回应命中落账行");
    assert_eq!(items[0]["kind"], "split");
    assert_eq!(items[0]["amount_cents"], 0);
}

#[tokio::test]
async fn test_batch_create_reverse_split_shrinks_holding_and_keeps_balances() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-rs-api", "股票户", "investment", "CNY", 0);
        test_support::seed_instrument(&conn, "inst-rs-api", "502010", "证券基金", "CNY", "unknown");
    }

    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-rs-api","date":"2026-01-10","instrument_id":"inst-rs-api","quantity":100.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");

    let (_, balances_before) = get_json(&app, "/api/v1/accounts/balances").await;

    let split = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-rs-api","date":"2026-02-01","instrument_id":"inst-rs-api","quantity":-40.0}"#;
    let results = post_batch(&app, batch_body(&[split], None)).await;
    assert_eq!(results[0]["success"], true, "缩股应落账: {results:?}");
    let split_id = results[0]["id"].as_str().unwrap().to_string();

    let amount_cents: i64 = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(amount_cents, 0, "缩股无现金腿：行金额恒 0");

    let (action, quantity): (String, f64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT action, quantity FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(action, "split");
    assert!(
        (quantity + 40.0).abs() < 1e-9,
        "扩展行保留带符号 Δ，实际 {quantity}"
    );

    let (holding_qty, cost_basis): (f64, i64) = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings \
             WHERE account_id='acc-rs-api' AND instrument_id='inst-rs-api'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert!(
        (holding_qty - 60.0).abs() < 1e-6,
        "持仓应为 100 − 40 = 60，实际 {holding_qty}"
    );
    assert!(
        (cost_basis - 10000).abs() <= 1,
        "批次总成本展示值应与调整前一致（亚分舍入 ±1 分），实际 {cost_basis}"
    );

    let (_, balances_after) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(
        balances_before, balances_after,
        "缩股落账前后全部账户余额应完全不变"
    );
    let realized: i64 = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales s \
             JOIN transactions t ON t.id = s.sell_transaction_id \
             WHERE t.account_id='acc-rs-api'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(realized, 0, "缩股本身不产生任何已实现盈亏");
}

#[tokio::test]
async fn test_batch_create_split_guards_return_coded_errors() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-sp-guard", "股票户", "investment", "CNY", 0);
        test_support::seed_account(&conn, "acc-sp-cash", "现金户", "cash", "CNY", 0);
        test_support::seed_instrument(&conn, "inst-sp-g", "502010", "证券基金", "CNY", "unknown");
        test_support::seed_instrument(
            &conn,
            "inst-sp-none",
            "161725",
            "无持仓基金",
            "CNY",
            "unknown",
        );
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-01-10","instrument_id":"inst-sp-g","quantity":100.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");

    let zero_holding = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-none","quantity":10.0}"#;
    let over_shrink = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":-150.0}"#;
    let with_fee = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":10.0,"fee_cents":100}"#;
    let with_amount = r#"{"kind":"split","amount_cents":500,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":10.0}"#;
    let with_price = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":10.0,"price_cents":10000}"#;
    let with_funding = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":10.0,"funding_account_id":"acc-sp-cash"}"#;
    let with_merchant = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-guard","date":"2026-02-01","instrument_id":"inst-sp-g","quantity":10.0,"merchant_name":"京东"}"#;

    let results = post_batch(
        &app,
        batch_body(
            &[
                zero_holding,
                over_shrink,
                with_fee,
                with_amount,
                with_price,
                with_funding,
                with_merchant,
            ],
            None,
        ),
    )
    .await;
    assert_eq!(results.len(), 7);
    let errors: Vec<&str> = results
        .iter()
        .map(|r| r["error"].as_str().unwrap_or(""))
        .collect();
    assert!(errors[0].contains("有在用持仓"), "{errors:?}");
    assert!(errors[1].contains("缩股"), "{errors:?}");
    assert!(errors[2].contains("不接受手续费"), "{errors:?}");
    assert!(errors[3].contains("金额必须为 0"), "{errors:?}");
    assert!(errors[4].contains("不可提供单价"), "{errors:?}");
    assert!(errors[5].contains("不能携带出资账户"), "{errors:?}");
    assert!(errors[6].contains("不能携带商户"), "{errors:?}");
    for r in &results {
        assert_eq!(r["success"], false);
    }

    let conn = conn.lock().unwrap();
    let splits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE action='split'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(splits, 0, "被拒的份额调整不应落库");
}

#[tokio::test]
async fn test_split_update_delete_via_api() {
    let (app, _conn) = setup_app();
    {
        let conn = _conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-sp-put", "股票户", "investment", "CNY", 0);
        test_support::seed_instrument(&conn, "inst-sp-p", "502010", "证券基金", "CNY", "unknown");
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-01-10","instrument_id":"inst-sp-p","quantity":100.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true);
    let buy_id = created[0]["id"].as_str().unwrap().to_string();

    let split = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-02-01","instrument_id":"inst-sp-p","quantity":50.0}"#;
    let results = post_batch(&app, batch_body(&[split], None)).await;
    assert_eq!(results[0]["success"], true, "{results:?}");
    let split_id = results[0]["id"].as_str().unwrap().to_string();

    let as_split_more = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-02-05","instrument_id":"inst-sp-p","quantity":100.0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &split_id, as_split_more).await;
    assert_eq!(status, StatusCode::OK, "就地修改应放行: {bytes:?}");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["kind"], "split");
    assert_eq!(updated["id"], split_id);
    let (_, list) = get_json(&app, "/api/v1/transactions?kinds=split").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], split_id);

    let as_buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-01-10","instrument_id":"inst-sp-p","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &split_id, as_buy).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.split-kind-change-forbidden");

    let as_split = r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-02-01","instrument_id":"inst-sp-p","quantity":5.0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &buy_id, as_split).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.split-kind-change-forbidden");

    let (status, _) = delete_transaction_via_api(&app, &split_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = get_json(&app, "/api/v1/transactions?kinds=split").await;
    assert!(
        list["items"].as_array().unwrap().is_empty(),
        "删除后按 kinds=split 不再命中"
    );

    let results = post_batch(
        &app,
        batch_body(
            &[
                r#"{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-03-01","instrument_id":"inst-sp-p","quantity":100.0}"#,
                r#"{"kind":"sell","amount_cents":0,"currency_code":"CNY","account_id":"acc-sp-put","date":"2026-03-02","instrument_id":"inst-sp-p","quantity":50.0,"price_cents":10000,"fee_cents":0}"#,
            ],
            None,
        ),
    )
    .await;
    assert_eq!(results[0]["success"], true, "{results:?}");
    assert_eq!(results[1]["success"], true, "{results:?}");
    let guarded_split_id = results[0]["id"].as_str().unwrap().to_string();
    let (status, bytes) = put_transaction_via_api(&app, &guarded_split_id, as_split_more).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.split-consumed-update");
    let (status, bytes) = delete_transaction_via_api(&app, &guarded_split_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.split-consumed-delete");
}
