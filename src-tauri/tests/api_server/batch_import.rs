use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{
    batch_body, body_to_bytes, count_active_transactions, create_account_via_api,
    get_first_category_id, get_json, post_batch, setup_app,
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

/// 非法 kind 在 API 边界即被拒绝（issue #74：kind 为闭集枚举，反序列化阶段校验）：
/// - batch 请求体任一条 kind 非法 → 整批 4xx（请求体格式错误，axum Json rejection 为 422），
///   不是逐条 success:false；
/// - list 查询参数 kind 非法 → 4xx（400）。
/// 合法 kind 的成功路径不变（由其余测试覆盖）；断言只要求 4xx（用户传递参数错误），
/// 不绑定具体状态码。
#[tokio::test]
async fn test_kind_enum_rejects_unknown_at_api_boundary() {
    let (app, _) = setup_app();

    // batch：非法 kind → 整批 4xx
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

    // list：非法 kind 查询参数 → 4xx（Query rejection 响应体为纯文本，不走 get_json）
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/transactions?kind=bonus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "非法 kind 过滤参数应 4xx，实际: {}",
        response.status()
    );
}

/// issue #72：dividend/split 已声明但未实现，经交易接口（批量创建）显式「暂不支持」拒绝，
/// 且不落库——取代此前 writer::normalize 兜底的「仅处理通用交易类型」文案（唯一对外可观测变化）。
#[tokio::test]
async fn test_batch_create_dividend_and_split_rejected_with_not_supported() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "证券账户").await;

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"dividend","amount_cents":60,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-04"}},
                {{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-05"}}
            ]
        }}"#
    );

    let results = post_batch(&app, body).await;
    assert_eq!(results.len(), 2);
    for r in &results {
        assert_eq!(r["success"], false, "dividend/split 应拒绝: {r}");
        assert_eq!(r["duplicate"], false);
        assert!(
            r["error"].as_str().unwrap().contains("暂不支持"),
            "应返回明确的「暂不支持」错误，实际: {r}"
        );
    }

    let count = conn.lock().unwrap();
    assert_eq!(
        count_active_transactions(&count),
        0,
        "被拒绝的 dividend/split 不应落库"
    );
}

/// issue #72：把一笔已有普通交易修改为 dividend/split 同样显式拒绝（行为层单点分派覆盖修改路径），
/// 原交易保持不变。
#[tokio::test]
async fn test_update_transaction_to_dividend_rejected_with_not_supported() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    // 先创建一笔普通支出。
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
    assert!(
        err["message"].as_str().unwrap().contains("暂不支持"),
        "修改为 dividend 应报「暂不支持」，实际: {err}"
    );

    // 原交易保持不变（仍是 expense）。
    let list = get_json(&app, "/api/v1/transactions").await;
    let items = list.1["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "expense");
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

    // 重复导入命中重复后，dedup_hash 保持原值不变（编辑/同步无特殊处理）
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
