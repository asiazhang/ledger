use axum::http::StatusCode;

use crate::common::{
    batch_body, create_account_via_api, create_category_via_api, dates_of, get_json, get_status,
    items_of, post_batch, seed_readback_transactions, setup_app,
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
async fn test_get_transactions_filters_by_kinds_set() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let category_id = create_category_via_api(&app, r#"{"name":"餐饮","kind":"expense"}"#).await;

    // 带分类支出 + 其退款（继承分类）；无分类收入与转账（转账天然无分类）
    let expense = format!(
        r#"{{"kind":"expense","amount_cents":100,"currency_code":"CNY","account_id":"{account_id}","category_id":"{category_id}","date":"2026-05-01"}}"#
    );
    let income = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-02"}}"#
    );
    let created = post_batch(&app, batch_body(&[&expense, &income], None)).await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");
    let expense_id = created[0]["id"].as_str().unwrap();
    let refund = format!(
        r#"{{"kind":"refund","amount_cents":50,"currency_code":"CNY","account_id":"{account_id}","refund_of_transaction_id":"{expense_id}","date":"2026-05-03"}}"#
    );
    let created = post_batch(&app, batch_body(&[&refund], None)).await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");

    // 类型集合命中集合内各 kind（含带分类退款），排除无分类收入；逗号分隔单参数绑定
    let (status, body) = get_json(&app, "/api/v1/transactions?kinds=expense,refund").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2, "应只命中支出与退款: {body:?}");
    assert!(
        txs.iter()
            .all(|t| t["kind"] == "expense" || t["kind"] == "refund")
    );
    assert_eq!(body["total"], 2);

    // 类型集合 × 仅无分类 AND 组合：支出/退款都带分类 → 空集
    let (status, body) = get_json(
        &app,
        "/api/v1/transactions?kinds=expense,refund&uncategorized_only=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "AND 组合应为空集: {body:?}");
    assert_eq!(body["total"], 0);

    // 仅无分类单独调用行为不变（语义纯 NULL，不限定类型）
    let (_, body) = get_json(&app, "/api/v1/transactions?uncategorized_only=true").await;
    assert_eq!(body["total"], 1, "仅无分类应命中无分类收入: {body:?}");

    // 既有单值 kind 参数行为不变（只增不改）
    let (_, body) = get_json(&app, "/api/v1/transactions?kind=expense").await;
    assert_eq!(body["total"], 1);

    // 集合外字面量：非法值 4xx（与单值 kind 同规）；Query 反序列化拒绝，响应体非 JSON 契约
    let status = get_status(&app, "/api/v1/transactions?kinds=expense,bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
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

/// 行携带来源字段（spec #704 / issue #706 tracer bullet：保单分支）：
/// 挂单保费返回 `{kind: "policy", entity_id, display_name: 险种名, status: null}`；
/// 软删保单的历史引用照常返回名称并携带 `status: "deleted"`；
/// 无保单交易 `source` 为 `null`。保单/保司行直插（policy 域无 HTTP 端点，先例商户）。
#[tokio::test]
async fn test_get_transactions_includes_policy_source() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    let policy_id = "pol-1";
    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('mch-1','平安保险','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO policies (id,merchant_id,policy_number,product_name,start_date,end_date,\
             coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'mch-1','P2026-201','重疾险','2026-01-01','2036-01-01',NULL,NULL,NULL,\
             '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![policy_id],
        )
        .unwrap();
    }

    let with_policy = format!(
        r#"{{"kind":"expense","amount_cents":300000,"currency_code":"CNY","account_id":"{account_id}","policy_id":"{policy_id}","date":"2026-02-01"}}"#
    );
    let without_policy = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&with_policy, &without_policy], None)).await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);

    // 挂单保费：来源 = 保单（实体 id + 险种名，在册无状态标注）
    let premium = txs
        .iter()
        .find(|t| t["policy_id"] == policy_id)
        .expect("挂单保费应返回");
    assert_eq!(
        premium["source"],
        serde_json::json!({
            "kind": "policy",
            "entity_id": policy_id,
            "display_name": "重疾险",
            "status": serde_json::Value::Null,
        }),
        "挂单保费来源应为保单: {premium:?}"
    );

    // 无保单交易：来源为空
    let plain = txs.iter().find(|t| t["policy_id"].is_null()).unwrap();
    assert!(
        plain["source"].is_null(),
        "无挂单交易来源应为 null: {plain:?}"
    );

    // 软删保单：历史引用照常返回名称 + 已删除状态（引用保留不置空，ADR-0051 决策 5）
    {
        let c = conn.lock().unwrap();
        c.execute(
            "UPDATE policies SET is_deleted=1 WHERE id=?1",
            rusqlite::params![policy_id],
        )
        .unwrap();
    }
    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&body);
    let premium = txs
        .iter()
        .find(|t| t["policy_id"] == policy_id)
        .expect("软删保单的历史流水应照常返回");
    assert_eq!(premium["source"]["display_name"], "重疾险");
    assert_eq!(premium["source"]["status"], "deleted");
}

/// 行携带计划来源（spec #704 / issue #707 计划三形态分支）：期次生成的交易返回
/// `{kind: "subscription", entity_id: 计划 id, display_name: 计划名（备注）,
/// status}`；已取消计划携带 `status: "cancelled"`；无期次链接的交易 `source`
/// 为 `null`。计划/期次行直插（scheduled 域无 HTTP 端点，先例保单直插）。
#[tokio::test]
async fn test_get_transactions_includes_plan_source() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    // 先经既有 batch 端点落两笔交易（一挂期次、一不挂），再直插计划与期次行
    let linked = format!(
        r#"{{"kind":"expense","amount_cents":3000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-02-01"}}"#
    );
    let plain = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&linked, &plain], None)).await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");
    let linked_txn_id = created[0]["id"].as_str().unwrap().to_string();

    let plan_id = "plan-1";
    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO scheduled_transactions \
             (id,kind,status,account_id,category_id,amount_cents,currency_code,\
             recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
             created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'subscription','cancelled',?2,NULL,3000,'CNY',\
             'monthly',1,NULL,'2026-02-01','视频会员',\
             '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![plan_id, account_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO scheduled_transaction_occurrences \
             (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
             created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('occ-1',?1,'2026-02-01','completed',?2,3000,\
             '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![plan_id, linked_txn_id],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);

    // 期次生成的交易：来源 = 订阅计划（实体 id + 计划名，已取消标注）
    let from_plan = txs
        .iter()
        .find(|t| t["id"] == linked_txn_id.as_str())
        .expect("期次生成的交易应返回");
    assert_eq!(
        from_plan["source"],
        serde_json::json!({
            "kind": "subscription",
            "entity_id": plan_id,
            "display_name": "视频会员",
            "status": "cancelled",
        }),
        "期次生成的交易来源应为已取消订阅计划: {from_plan:?}"
    );

    // 无期次链接的交易：来源为空
    let plain_txn = txs
        .iter()
        .find(|t| t["id"] != linked_txn_id.as_str())
        .unwrap();
    assert!(
        plain_txn["source"].is_null(),
        "无来源交易 source 应为 null: {plain_txn:?}"
    );
}
