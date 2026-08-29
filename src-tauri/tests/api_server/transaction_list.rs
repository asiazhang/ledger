use axum::http::StatusCode;

use crate::common::{
    batch_body, create_account_via_api, dates_of, get_json, items_of, post_batch,
    seed_readback_transactions, setup_app,
};

#[tokio::test]
async fn test_get_transactions_returns_empty_list_when_none() {
    let (app, _) = setup_app();

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "无交易时应返回空 items");
    assert_eq!(body["total"], 0, "total 应为 0");
}

#[tokio::test]
async fn test_get_transactions_returns_all_undeleted_newest_first() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx_jan = format!(
        r#"{{"kind":"income","amount_cents":100,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-01"}}"#
    );
    let tx_mar = format!(
        r#"{{"kind":"expense","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let tx_feb = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-02-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx_jan, &tx_mar, &tx_feb], None)).await;

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 3);
    assert_eq!(body["total"], 3, "缺省返回全部时 total 应为全部条数");
    assert_eq!(txs[0]["date"], "2026-03-01");
    assert_eq!(txs[0]["amount_cents"], 300);
    assert_eq!(txs[1]["date"], "2026-02-01");
    assert_eq!(txs[1]["amount_cents"], 200);
    assert_eq!(txs[2]["date"], "2026-01-01");
    assert_eq!(txs[2]["amount_cents"], 100);
    for tx in txs {
        assert_eq!(tx["is_deleted"], false);
        assert_eq!(tx["account_id"], account_id);
    }
}

#[tokio::test]
async fn test_get_transactions_from_to_is_inclusive() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?from=2026-01-15&to=2026-02-15").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        dates_of(&body),
        vec!["2026-02-15", "2026-02-01", "2026-01-15"]
    );
    assert_eq!(body["total"], 3, "日期过滤后 total 应为过滤后总数");
    let sum: i64 = items_of(&body)
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 3600);
}

#[tokio::test]
async fn test_get_transactions_filters_by_account_id() {
    let (app, _) = setup_app();
    let (cash, bank) = seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, &format!("/api/v1/transactions?account_id={cash}")).await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 3);
    assert_eq!(body["total"], 3);
    assert!(txs.iter().all(|t| t["account_id"] == cash));
    assert!(txs.iter().all(|t| t["account_id"] != bank));
    let sum: i64 = txs
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 1600);
}

#[tokio::test]
async fn test_get_transactions_filters_by_kind() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?kind=expense").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);
    assert_eq!(body["total"], 2);
    assert!(txs.iter().all(|t| t["kind"] == "expense"));
    let sum: i64 = txs
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 700);
}

#[tokio::test]
async fn test_get_transactions_filters_by_merchant_id_query_param() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    // 商户行直插（merchant 域无 HTTP 端点，T7 前临时前置）
    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('mch-1','京东','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
    }

    let with_merchant = format!(
        r#"{{"kind":"expense","amount_cents":100,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-01","merchant_id":"mch-1"}}"#
    );
    let without_merchant = format!(
        r#"{{"kind":"expense","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-02"}}"#
    );
    let created = post_batch(&app, batch_body(&[&with_merchant, &without_merchant], None)).await;
    assert!(
        created.iter().all(|r| r["success"] == true),
        "写入应成功: {created:?}"
    );

    // 按商户过滤：只命中带 merchant_id 的一条，total 口径同步
    let (status, body) = get_json(&app, "/api/v1/transactions?merchant_id=mch-1").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 1, "应只返回该商户交易: {body:?}");
    assert_eq!(txs[0]["merchant_id"], "mch-1");
    assert_eq!(body["total"], 1);

    // 不带参数回全量
    let (_, all) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(all["total"], 2);
}

#[tokio::test]
async fn test_get_transactions_limit_truncates() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dates_of(&body), vec!["2026-03-01", "2026-02-15"]);
    assert_eq!(body["total"], 5, "limit 只截取 items，total 仍为过滤后总数");
}

#[tokio::test]
async fn test_get_transactions_pagination_returns_page_and_total() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let txs: Vec<String> = (1..=25)
        .map(|i| {
            format!(
                r#"{{"kind":"expense","amount_cents":{},"currency_code":"CNY","account_id":"{account_id}","date":"2026-06-{:02}"}}"#,
                i * 100,
                i
            )
        })
        .collect();
    let refs: Vec<&str> = txs.iter().map(String::as_str).collect();
    post_batch(&app, batch_body(&refs, None)).await;

    let (status, p1) = get_json(&app, "/api/v1/transactions?page=1&page_size=10").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of(&p1).len(), 10, "第 1 页应返回 10 条");
    assert_eq!(p1["total"], 25);

    let (_, p3) = get_json(&app, "/api/v1/transactions?page=3&page_size=10").await;
    assert_eq!(items_of(&p3).len(), 5, "第 3 页应返回剩余 5 条");
    assert_eq!(p3["total"], 25);
}

#[tokio::test]
async fn test_get_transactions_pagination_out_of_range_page() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-06-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx], None)).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?page=99&page_size=10").await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "超范围页码应返回空 items");
    assert_eq!(body["total"], 1, "total 仍为过滤后总数");
}

#[tokio::test]
async fn test_get_transactions_excludes_soft_deleted() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let keep = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-01"}}"#
    );
    let drop = format!(
        r#"{{"kind":"expense","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-02"}}"#
    );
    let created = post_batch(&app, batch_body(&[&keep, &drop], None)).await;
    let deleted_id = created[1]["id"].as_str().unwrap();
    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE transactions SET is_deleted=1 WHERE id=?1",
            rusqlite::params![deleted_id],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 1);
    assert_eq!(body["total"], 1);
    assert_eq!(txs[0]["amount_cents"], 1000);
    assert_eq!(txs[0]["date"], "2026-01-01");
}
