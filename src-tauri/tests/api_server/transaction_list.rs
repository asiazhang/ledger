use axum::http::StatusCode;
use ledger_investment::{InstrumentInput, InstrumentType, create_instrument};
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::FIXED_NOW;

use crate::common::{
    batch_body, create_account_via_api, create_account_via_api_with_type, create_category_via_api,
    dates_of, get_json, get_status, items_of, post_batch, seed_readback_transactions, setup_app,
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
async fn test_get_transactions_filters_by_kinds_single_value() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?kinds=expense").await;
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

    let (status, body) = get_json(&app, "/api/v1/transactions?kinds=expense,refund").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2, "应只命中支出与退款: {body:?}");
    assert!(
        txs.iter()
            .all(|t| t["kind"] == "expense" || t["kind"] == "refund")
    );
    assert_eq!(body["total"], 2);

    let (status, body) = get_json(
        &app,
        "/api/v1/transactions?kinds=expense,refund&uncategorized_only=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "AND 组合应为空集: {body:?}");
    assert_eq!(body["total"], 0);

    let (_, body) = get_json(&app, "/api/v1/transactions?uncategorized_only=true").await;
    assert_eq!(body["total"], 1, "仅无分类应命中无分类收入: {body:?}");

    let (_, body) = get_json(&app, "/api/v1/transactions?kind=expense").await;
    assert_eq!(body["total"], 3, "kind= 不再生效，应返回全量: {body:?}");

    let status = get_status(&app, "/api/v1/transactions?kinds=expense,bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_transactions_hide_investment_related_excludes_rows() {
    let (app, _) = setup_app();
    let bank = create_account_via_api_with_type(&app, "银行卡", "bank").await;
    let inv = create_account_via_api_with_type(&app, "投资账户", "investment").await;

    let topup = format!(
        r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{bank}","to_account_id":"{inv}","date":"2026-07-01"}}"#
    );
    let daily = format!(
        r#"{{"kind":"expense","amount_cents":300,"currency_code":"CNY","account_id":"{bank}","date":"2026-07-02"}}"#
    );
    let created = post_batch(&app, batch_body(&[&topup, &daily], None)).await;
    assert!(
        created.iter().all(|r| r["success"] == true),
        "写入应成功: {created:?}"
    );

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2, "缺省应返回全部行: {body:?}");

    let (status, body) = get_json(&app, "/api/v1/transactions?hide_investment_related=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1, "投资相关行应被排除: {body:?}");
    let txs = items_of(&body);
    assert_eq!(txs.len(), 1, "行集应与 total 同步收窄: {body:?}");
    assert_eq!(txs[0]["kind"], "expense", "保留行应为银行卡日常支出");

    let (status, body) = get_json(&app, "/api/v1/transactions?hide_investment_related=false").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2, "false 不收窄行集: {body:?}");

    let (status, body) = get_json(&app, "/api/v1/transactions?kinds=transfer").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1, "仅类型过滤时转账行照常返回: {body:?}");
    let (status, body) = get_json(
        &app,
        "/api/v1/transactions?kinds=transfer&hide_investment_related=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        items_of(&body).is_empty(),
        "类型 ∩ 偏好收窄应为空集: {body:?}"
    );
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn test_get_transactions_filters_by_merchant_id_query_param() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('mch-1','京东',?1,?2,1,'test',0)",
            rusqlite::params![FIXED_NOW, FIXED_NOW],
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

    let (status, body) = get_json(&app, "/api/v1/transactions?merchant_id=mch-1").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 1, "应只返回该商户交易: {body:?}");
    assert_eq!(txs[0]["merchant_id"], "mch-1");
    assert_eq!(body["total"], 1);

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

#[tokio::test]
async fn test_get_transactions_includes_policy_source() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    let policy_id = "pol-1";
    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('ins-1','测试保司',?1,?2,1,'test',0)",
            rusqlite::params![FIXED_NOW, FIXED_NOW],
        )
        .unwrap();
        c.execute(
            "INSERT INTO policies (id,insurer_id,policy_number,product_name,start_date,end_date,\
             coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'ins-1','P2026-201','重疾险','2026-01-01','2036-01-01',NULL,NULL,NULL,\
             ?2,?3,1,'test',0)",
            rusqlite::params![policy_id, FIXED_NOW, FIXED_NOW],
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

    let plain = txs.iter().find(|t| t["policy_id"].is_null()).unwrap();
    assert!(
        plain["source"].is_null(),
        "无挂单交易来源应为 null: {plain:?}"
    );

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

#[tokio::test]
async fn test_get_transactions_includes_plan_source() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

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
             ?3,?4,1,'test',0)",
            rusqlite::params![plan_id, account_id, FIXED_NOW, FIXED_NOW],
        )
        .unwrap();
        c.execute(
            "INSERT INTO scheduled_transaction_occurrences \
             (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
             created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('occ-1',?1,'2026-02-01','completed',?2,3000,\
             ?3,?4,1,'test',0)",
            rusqlite::params![plan_id, linked_txn_id, FIXED_NOW, FIXED_NOW],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);

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

    let plain_txn = txs
        .iter()
        .find(|t| t["id"] != linked_txn_id.as_str())
        .unwrap();
    assert!(
        plain_txn["source"].is_null(),
        "无来源交易 source 应为 null: {plain_txn:?}"
    );
}

#[tokio::test]
async fn test_get_transactions_includes_item_source() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;

    let with_item_1 = format!(
        r#"{{"kind":"expense","amount_cents":599900,"currency_code":"CNY","account_id":"{account_id}","date":"2026-02-01"}}"#
    );
    let with_item_2 = format!(
        r#"{{"kind":"expense","amount_cents":80000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-02-02"}}"#
    );
    let plain = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let created = post_batch(
        &app,
        batch_body(&[&with_item_1, &with_item_2, &plain], None),
    )
    .await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");
    let txn_id_1 = created[0]["id"].as_str().unwrap().to_string();
    let txn_id_2 = created[1]["id"].as_str().unwrap().to_string();

    {
        let c = conn.lock().unwrap();
        c.execute(
            "INSERT INTO items (id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,
             status,disposal_date,residual_value_cents,purchase_transaction_id,note,
             created_at,updated_at,version,device_id,is_deleted)
             VALUES ('item-1','手机','2026-02-01',599900,'CNY',599900,'in_use',NULL,NULL,?1,NULL,
             ?2,?3,1,'test',0),
             ('item-2','耳机','2026-02-02',80000,'CNY',80000,'disposed','2026-05-01',NULL,?4,NULL,
             ?2,?3,1,'test',0)",
            rusqlite::params![txn_id_1, FIXED_NOW, FIXED_NOW, txn_id_2],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 3);

    let in_use = txs.iter().find(|t| t["id"] == txn_id_1.as_str()).unwrap();
    assert_eq!(
        in_use["source"],
        serde_json::json!({
            "kind": "item",
            "entity_id": "item-1",
            "display_name": "手机",
            "status": serde_json::Value::Null,
        }),
        "购买交易来源应为溯源物品: {in_use:?}"
    );

    let disposed = txs.iter().find(|t| t["id"] == txn_id_2.as_str()).unwrap();
    assert_eq!(
        disposed["source"],
        serde_json::json!({
            "kind": "item",
            "entity_id": "item-2",
            "display_name": "耳机",
            "status": "disposed",
        }),
        "已处置物品的购买交易来源应携带已处置标注: {disposed:?}"
    );

    let plain_txn = txs
        .iter()
        .find(|t| t["id"] != txn_id_1.as_str() && t["id"] != txn_id_2.as_str())
        .unwrap();
    assert!(
        plain_txn["source"].is_null(),
        "未建物品交易 source 应为 null: {plain_txn:?}"
    );

    {
        let c = conn.lock().unwrap();
        c.execute("UPDATE items SET is_deleted=1 WHERE id='item-1'", [])
            .unwrap();
    }
    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&body);
    let unlinked = txs.iter().find(|t| t["id"] == txn_id_1.as_str()).unwrap();
    assert!(
        unlinked["source"].is_null(),
        "软删物品的购买交易来源应为 null: {unlinked:?}"
    );
}

#[tokio::test]
async fn test_get_transactions_includes_instrument_source() {
    let (app, conn) = setup_app();

    let account_id;
    let instrument_id;
    {
        let c = conn.lock().unwrap();
        account_id = test_support::seed_account(&c, "acc-inv", "证券", "investment", "CNY", 0);
        instrument_id = create_instrument(
            &c,
            InstrumentInput {
                symbol: "600519".to_string(),
                kind: InstrumentType::Stock,
                name: Some("招商银行".to_string()),
                currency_code: "CNY".to_string(),
                market: Some("sh".to_string()),
            },
        )
        .unwrap();
    }

    let buy = format!(
        r#"{{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","instrument_id":"{instrument_id}","quantity":2,"price_cents":100000,"date":"2026-02-01"}}"#
    );
    let plain = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&buy, &plain], None)).await;
    assert!(created.iter().all(|r| r["success"] == true), "{created:?}");
    let buy_txn_id = created[0]["id"].as_str().unwrap().to_string();

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);

    let buy_txn = txs.iter().find(|t| t["id"] == buy_txn_id.as_str()).unwrap();
    assert_eq!(
        buy_txn["source"],
        serde_json::json!({
            "kind": "instrument",
            "entity_id": instrument_id,
            "display_name": "600519 招商银行",
            "status": serde_json::Value::Null,
        }),
        "买入交易来源应为标的: {buy_txn:?}"
    );

    let plain_txn = txs.iter().find(|t| t["id"] != buy_txn_id.as_str()).unwrap();
    assert!(
        plain_txn["source"].is_null(),
        "无标的交易 source 应为 null: {plain_txn:?}"
    );
}

#[tokio::test]
async fn test_get_transactions_default_caps_at_page_size_100() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let txs: Vec<String> = (1..=150)
        .map(|i| {
            format!(
                r#"{{"kind":"expense","amount_cents":{},"currency_code":"CNY","account_id":"{account_id}","date":"2026-06-{:02}"}}"#,
                i * 100,
                i % 28 + 1
            )
        })
        .collect();
    let refs: Vec<&str> = txs.iter().map(String::as_str).collect();
    let created = post_batch(&app, batch_body(&refs, None)).await;
    assert!(
        created.iter().all(|r| r["success"] == true),
        "150 行应全部写入"
    );

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        items_of(&body).len(),
        100,
        "缺省应只返回前 100 条，不整表序列化"
    );
    assert_eq!(body["total"], 150, "total 恒为过滤后总数，供客户端翻页");

    let (_, p2) = get_json(&app, "/api/v1/transactions?page=2").await;
    assert_eq!(items_of(&p2).len(), 50, "第 2 页返回剩余 50 条");
    assert_eq!(p2["total"], 150);
}

#[tokio::test]
async fn test_get_transactions_page_size_over_cap_returns_coded_400() {
    let (app, _) = setup_app();

    let (status, err) = get_json(&app, "/api/v1/transactions?page_size=101").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["kind"], "Invalid", "码化参数错误归类 Invalid");
    assert_eq!(err["code"], "transaction.page-size-over-cap");
    assert_eq!(
        err["message"], "page_size 101 超过单页上限 100",
        "中文 message 与 zh 模板逐字一致"
    );
    assert_eq!(
        err["params"],
        serde_json::json!(["101", "100"]),
        "params 与消息中动态值顺序一致（请求值 → 上限）"
    );
}

#[tokio::test]
async fn test_get_transactions_limit_out_of_range_returns_coded_400() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    for limit in ["101", "-1"] {
        let (status, err) = get_json(&app, &format!("/api/v1/transactions?limit={limit}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "limit={limit} 应拒绝");
        assert_eq!(
            err["code"], "transaction.limit-out-of-range",
            "limit={limit}"
        );
        assert_eq!(
            err["message"],
            format!("limit {limit} 超出允许范围（0 到 100，负值无上限仅限 IPC）"),
            "limit={limit} 中文 message 与 zh 模板逐字一致"
        );
        assert_eq!(
            err["params"],
            serde_json::json!([limit, "100"]),
            "params 与消息中动态值顺序一致"
        );
    }

    let (status, body) = get_json(&app, "/api/v1/transactions?limit=100").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of(&body).len(), 5);
    let (status, _) = get_json(&app, "/api/v1/transactions?page_size=100").await;
    assert_eq!(status, StatusCode::OK);
}
