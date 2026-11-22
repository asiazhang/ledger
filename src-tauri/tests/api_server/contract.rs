use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, get_json, setup_app};

async fn fetch_contract() -> serde_json::Value {
    let (app, _) = setup_app();
    let (status, doc) = get_json(&app, "/api/v1/contract").await;
    assert_eq!(status, StatusCode::OK);
    doc
}

#[tokio::test]
async fn contract_dialect_header() {
    let doc = fetch_contract().await;
    assert_eq!(doc["v"], "ledger-contract-1", "方言版本标记");
    assert_eq!(
        doc["base"], "http://127.0.0.1:9527/api/v1",
        "base 地址落明（端点路径省略 /api/v1 前缀）"
    );
    let legend = doc["legend"].as_str().expect("图例应为字符串");
    for frag in ["str/i64/f64/bool/obj", "T[]", "后缀?", "字段值[类型,说明]"] {
        assert!(legend.contains(frag), "图例应解释 {frag}");
    }
    assert!(
        doc["endpoints"]
            .as_array()
            .expect("endpoints 数组在位")
            .len()
            >= 19
    );
    assert!(doc["schemas"].as_object().expect("schemas 对象在位").len() >= 26);
}

#[tokio::test]
async fn contract_covers_all_endpoints() {
    let doc = fetch_contract().await;
    let endpoints = doc["endpoints"].as_array().expect("endpoints 数组");

    let expected: &[(&str, &str)] = &[
        ("GET", "/accounts"),
        ("POST", "/accounts"),
        ("PUT", "/accounts/{id}"),
        ("DELETE", "/accounts/{id}"),
        ("GET", "/accounts/balances"),
        ("GET", "/categories"),
        ("POST", "/categories"),
        ("DELETE", "/categories/{id}"),
        ("GET", "/currencies"),
        ("GET", "/instruments"),
        ("POST", "/instruments"),
        ("GET", "/funds/{code}"),
        ("GET", "/stocks/{code}"),
        ("GET", "/merchants"),
        ("GET", "/transactions"),
        ("POST", "/transactions/batch"),
        ("DELETE", "/transactions/{id}"),
        ("PUT", "/transactions/{id}"),
        ("GET", "/import/knowledge"),
        ("GET", "/import/knowledge/investment"),
    ];
    for (method, path) in expected {
        let hit = endpoints.iter().any(|e| {
            e["m"] == *method
                && e["p"] == *path
                && e["s"].as_str().map(|s| !s.is_empty()).unwrap_or(false)
        });
        assert!(hit, "方言应包含端点 {method} {path}（summary 非空）");
    }
}

#[tokio::test]
async fn contract_request_body_and_responses() {
    let doc = fetch_contract().await;
    let endpoints = doc["endpoints"].as_array().unwrap();

    let find = |m: &str, p: &str| {
        endpoints
            .iter()
            .find(|e| e["m"] == m && e["p"] == p)
            .unwrap_or_else(|| panic!("方言应包含端点 {m} {p}"))
    };

    let batch = find("POST", "/transactions/batch");
    assert_eq!(batch["body"], "TransactionBatchInput");
    assert_eq!(batch["res"]["200"], "CreateTransactionResult[]");
    assert_eq!(batch["res"]["400"], "ErrorResponse");

    assert_eq!(
        find("GET", "/transactions")["res"]["200"],
        "TransactionListResult"
    );

    let delete = find("DELETE", "/transactions/{id}");
    assert_eq!(delete["res"]["204"], "-", "无响应体以 - 占位");
    assert_eq!(delete["res"]["404"], "ErrorResponse");
    assert!(
        delete["res"].get("400").is_none(),
        "删除不再声明部分卖出 400（级联删除退场，issue #940）"
    );

    let put = find("PUT", "/accounts/{id}");
    assert_eq!(put["body"], "AccountUpdateInput");
    assert_eq!(put["res"]["200"], "Account");

    assert_eq!(find("GET", "/import/knowledge")["res"]["200"], "-");
    assert_eq!(
        find("GET", "/import/knowledge/investment")["res"]["200"],
        "-"
    );
}

#[tokio::test]
async fn contract_kind_enum_is_closed_lowercase_set() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    let kind = &schemas["TransactionKind"];
    assert_eq!(
        kind.as_str()
            .map(|s| s.to_owned())
            .ok_or("应为字符串表达式")
            .unwrap(),
        "income|expense|transfer|refund|buy|sell|dividend|split|convert",
        "kind 枚举应为闭集的 9 个小写值"
    );
    let tx_kind = &schemas["Transaction"]["kind"];
    assert_eq!(
        tx_kind[0], "TransactionKind",
        "Transaction.kind 应引用 schema 名"
    );
}

#[tokio::test]
async fn contract_batch_wrapper_and_duplicate_field() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    let batch = &schemas["TransactionBatchInput"];
    assert_eq!(
        batch["transactions"], "TransactionInput[]",
        "transactions 必填"
    );
    assert_eq!(
        batch["dedup?"], "bool?",
        "dedup 应可缺省（? 后缀即 optionality 标记；默认 true）"
    );

    let result = &schemas["CreateTransactionResult"];
    assert_eq!(result["duplicate"], "bool", "duplicate 标记在位");
    assert_eq!(result["success"], "bool");
    assert_eq!(result["id?"], "str?", "重复命中返回已有 id、否则 null");

    assert_eq!(
        &schemas["Account"]["is_hidden"][0], "bool",
        "黑洞账户契约（is_hidden）在位"
    );
}

#[tokio::test]
async fn contract_update_input_omits_idempotency_key() {
    let doc = fetch_contract().await;
    let upd = &doc["schemas"]["UpdateTransactionInput"];
    assert!(!upd["kind"].is_null(), "kind 在位");
    assert!(!upd["amount_cents"].is_null(), "amount_cents 在位");
    assert!(
        upd.get("idempotency_key").is_none(),
        "修改请求体不应含 idempotency_key（幂等键不可编辑）"
    );
}

#[tokio::test]
async fn contract_investment_fields_keep_descriptions() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    for schema_name in ["TransactionInput", "UpdateTransactionInput"] {
        let props = &schemas[schema_name];
        let expected_types = [
            ("instrument_id?", "str?"),
            ("quantity?", "f64?"),
            ("price_cents?", "i64?"),
            ("fee_cents?", "i64?"),
        ];
        for (field, ty) in expected_types {
            let value = props[field]
                .as_array()
                .unwrap_or_else(|| panic!("{schema_name}.{field} 应为 [类型, 说明] 元组"));
            assert_eq!(value[0], ty, "{schema_name}.{field} 类型表达式");
            let description = value[1].as_str().expect("应带描述");
            assert!(
                !description.trim().is_empty(),
                "{schema_name}.{field} 应保留中文描述（投资四字段语义锁）"
            );
        }
    }

    let merchant = &schemas["TransactionInput"]["merchant_name?"];
    let merchant_desc = merchant[1].as_str().unwrap();
    assert!(
        merchant_desc.contains("命中复用、未命中即建") && merchant_desc.contains("merchant_id"),
        "商户名字段描述语义原文保留"
    );
}

#[tokio::test]
async fn contract_type_expressions_cover_all_forms() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    assert_eq!(
        schemas["AccountType"],
        "cash|bank|credit|ewallet|investment|debt|receivable|other"
    );
    assert_eq!(schemas["InstrumentType"], "stock|fund|bond|etf|other");

    assert_eq!(schemas["TransactionListResult"]["items"], "Transaction[]");
    assert_eq!(
        schemas["TransactionBatchInput"]["transactions"],
        "TransactionInput[]"
    );

    assert_eq!(schemas["TransactionInput"]["amount_cents"], "i64");
    assert_eq!(schemas["TransactionInput"]["merchant_name?"][0], "str?");
    assert_eq!(
        schemas["AccountInput"]["initial_balance_cents?"], "i64?",
        "可空整数 = i64?"
    );
    assert_eq!(
        schemas["TransactionInput"]["quantity?"][0], "f64?",
        "可空浮点（数量可含小数）= f64?"
    );

    assert_eq!(schemas["Transaction"]["source?"][0], "TransactionSource?");
    assert!(
        !schemas["Transaction"]["source?"][1]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "来源字段描述保留（元组第二位）"
    );

    let kind = &schemas["TransactionSource"]["kind"];
    assert_eq!(kind[0], "TransactionSourceKind");
    assert!(!kind[1].as_str().unwrap_or_default().is_empty());
}

#[tokio::test]
async fn contract_strips_provenance_while_openapi_keeps_it() {
    let (app, _) = setup_app();
    let (_, dialect) = get_json(&app, "/api/v1/contract").await;
    let (_, openapi) = get_json(&app, "/api/v1/openapi.json").await;

    let dialect_text = serde_json::to_string(&dialect).unwrap();
    assert!(!dialect_text.contains("issue #"), "方言应剥离 issue 引用");
    assert!(!dialect_text.contains("ADR-"), "方言应剥离 ADR 引用");

    let openapi_text = serde_json::to_string(&openapi).unwrap();
    assert!(
        openapi_text.contains("ADR-"),
        "标准 OpenAPI 投影保留出处引用（同一源，双投影分叉点）"
    );

    let fund_desc = dialect["schemas"]["FundLookup"]["code"][1]
        .as_str()
        .unwrap_or_default();
    let _ = fund_desc;
    assert!(
        !dialect["schemas"]["ErrorResponse"]["kind"].is_null(),
        "ErrorResponse 字段在位（schema 级描述退役后字段语义仍完整）"
    );
}

#[tokio::test]
async fn contract_transaction_schemas_carry_funding_account() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    for name in ["TransactionInput", "UpdateTransactionInput", "Transaction"] {
        let field = &schemas[name]["funding_account_id?"];
        assert!(!field.is_null(), "{name} 应带出可选出资账户字段");
        assert_eq!(field[0], "str?", "{name}.funding_account_id 应为可选字符串");
    }

    let desc = schemas["TransactionInput"]["funding_account_id?"][1]
        .as_str()
        .unwrap_or_default();
    assert!(
        desc.contains("buy/sell"),
        "字段描述应说明仅 buy/sell 可携带，实际: {desc}"
    );
}

#[tokio::test]
async fn contract_transaction_input_quantity_describes_split_semantics() {
    let doc = fetch_contract().await;
    let desc = doc["schemas"]["TransactionInput"]["quantity?"][1]
        .as_str()
        .unwrap_or_default();

    for frag in [
        "带符号份额增量",
        "折算",
        "`−` 缩股且 |Δ| 小于当前持仓",
        "必须 ≠ 0",
        "无现金腿",
        "`price_cents` 不提供、`fee_cents` 只能为 0",
    ] {
        assert!(
            desc.contains(frag),
            "quantity 字段描述应带出 split 语义 {frag:?}，实际: {desc}"
        );
    }
}

#[tokio::test]
async fn contract_size_within_budget() {
    let (app, _) = setup_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/contract")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    assert!(
        bytes.len() <= 31 * 1024,
        "紧凑契约方言应保持在预算内（当前 {} 字节，预算 31KB）",
        bytes.len()
    );
}

#[tokio::test]
async fn contract_is_data_source_neutral() {
    let doc = fetch_contract().await;
    let text = serde_json::to_string(&doc).unwrap();
    for banned in ["东财", "东方财富", "eastmoney"] {
        let at = text.find(banned).map(|i| {
            let mut s = i.saturating_sub(120);
            let mut e = (i + banned.len() + 80).min(text.len());
            while !text.is_char_boundary(s) {
                s -= 1;
            }
            while !text.is_char_boundary(e) {
                e -= 1;
            }
            format!("…{}…", &text[s..e])
        });
        assert!(
            !text.contains(banned),
            "契约方言应数据源中立，不得出现数据源名 {banned:?}（ADR-0130）：{at:?}"
        );
    }
}

#[tokio::test]
async fn contract_list_transactions_describes_hide_investment_related_param() {
    let doc = fetch_contract().await;
    let endpoint = doc["endpoints"]
        .as_array()
        .expect("endpoints 数组")
        .iter()
        .find(|e| e["m"] == "GET" && e["p"] == "/transactions")
        .expect("方言应包含 GET /transactions");
    let desc = endpoint["d"].as_str().expect("端点描述应为字符串");
    assert!(
        desc.contains("hide_investment_related"),
        "GET /transactions 契约自述应包含可选参数 hide_investment_related，实际: {desc}"
    );
}

#[tokio::test]
async fn contract_transaction_schemas_carry_purchases() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    for name in ["TransactionPurchase", "TransactionPurchaseInput"] {
        assert!(schemas.contains_key(name), "应带出购买项条目 schema {name}");
    }
    for name in ["TransactionInput", "UpdateTransactionInput", "Transaction"] {
        let has_field = schemas[name]
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k == "purchases" || k == "purchases?");
        assert!(has_field, "{name} 应带出购买项字段");
    }

    let desc = schemas["TransactionPurchaseInput"]["unit_price_cents?"][1]
        .as_str()
        .unwrap_or_default();
    assert!(
        desc.contains("留空"),
        "单价字段描述应说明可空语义（价格拿不到就留空），实际: {desc}"
    );
}
