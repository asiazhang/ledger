use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, get_json, setup_app};

#[tokio::test]
async fn test_openapi_doc_covers_delete_transaction_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let delete = &doc["paths"]["/api/v1/transactions/{id}"]["delete"];
    assert!(delete["summary"].is_string(), "OpenAPI 应包含 DELETE 端点");
    let params = delete["parameters"]
        .as_array()
        .expect("DELETE 应声明 path 参数");
    assert!(
        params.iter().any(|p| p["name"] == "id"),
        "DELETE 端点应声明 id 路径参数"
    );
    let responses = delete["responses"].as_object().unwrap();
    assert!(responses.contains_key("204"), "应声明 204 响应");
    assert!(responses.contains_key("404"), "应声明 404 响应");
}

#[tokio::test]
async fn test_openapi_json_endpoint_returns_doc() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(doc["openapi"].as_str(), Some("3.1.0"));
    assert!(doc["info"]["title"].is_string());
    assert_eq!(doc["info"]["version"].as_str(), Some("0.1.0"));
}

#[tokio::test]
async fn test_openapi_doc_covers_all_endpoints() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().expect("应包含 paths 对象");

    let expected: &[(&str, &str)] = &[
        ("/api/v1/accounts", "get"),
        ("/api/v1/accounts", "post"),
        ("/api/v1/accounts/{id}", "put"),
        ("/api/v1/accounts/{id}", "delete"),
        ("/api/v1/accounts/balances", "get"),
        ("/api/v1/categories", "get"),
        ("/api/v1/categories", "post"),
        ("/api/v1/categories/{id}", "delete"),
        ("/api/v1/currencies", "get"),
        ("/api/v1/instruments", "get"),
        ("/api/v1/instruments", "post"),
        ("/api/v1/funds/{code}", "get"),
        ("/api/v1/merchants", "get"),
        ("/api/v1/transactions", "get"),
        ("/api/v1/transactions/batch", "post"),
        ("/api/v1/transactions/{id}", "delete"),
        ("/api/v1/transactions/{id}", "put"),
        ("/api/v1/import/knowledge", "get"),
    ];
    for (path, method) in expected {
        assert!(
            paths.get(*path).and_then(|p| p.get(*method)).is_some(),
            "OpenAPI 文档应包含端点 {method} {path}"
        );
    }
}

#[tokio::test]
async fn test_openapi_doc_batch_wrapper_and_duplicate_field() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let schemas = doc["components"]["schemas"].as_object().unwrap();

    // batch 请求体 wrapper：{ transactions, dedup }
    let batch = &schemas["TransactionBatchInput"];
    let props = batch["properties"].as_object().unwrap();
    assert!(props.contains_key("transactions"));
    assert!(
        props.contains_key("dedup"),
        "batch wrapper 应包含 dedup 字段"
    );
    let required = batch["required"].as_array().unwrap();
    assert!(
        required.iter().any(|r| r == "transactions"),
        "transactions 应必填"
    );
    assert!(
        !required.iter().any(|r| r == "dedup"),
        "dedup 应可缺省（默认 true）"
    );

    // CreateTransactionResult 应包含 duplicate 字段
    let result = &schemas["CreateTransactionResult"];
    assert!(
        result["properties"]["duplicate"].is_object(),
        "CreateTransactionResult 应包含 duplicate 字段"
    );

    // 账户响应应包含 is_hidden（黑洞账户契约）
    let account = &schemas["Account"];
    assert!(
        account["properties"]["is_hidden"].is_object(),
        "Account 应包含 is_hidden 字段"
    );
}

#[tokio::test]
async fn test_openapi_update_transaction_input_omits_idempotency_key() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let upd = &schemas["UpdateTransactionInput"];
    let props = upd["properties"].as_object().unwrap();
    assert!(props.contains_key("kind"));
    assert!(props.contains_key("amount_cents"));
    assert!(
        !props.contains_key("idempotency_key"),
        "修改请求体不应含 idempotency_key（幂等键不可编辑）"
    );
}

/// 投资四字段契约描述锁（issue #298）：`TransactionInput` / `UpdateTransactionInput`
/// 的 `instrument_id` / `quantity` / `price_cents` / `fee_cents` 必须带中文描述——
/// 契约是 AI 的唯一字段语义来源，裸字段即契约缺口（buy/sell 的悬空契约曾致投资教学缺位）。
#[tokio::test]
async fn test_openapi_investment_fields_have_descriptions() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let schemas = doc["components"]["schemas"].as_object().unwrap();

    for schema_name in ["TransactionInput", "UpdateTransactionInput"] {
        let props = &schemas[schema_name]["properties"];
        for field in ["instrument_id", "quantity", "price_cents", "fee_cents"] {
            let description = props[field]["description"].as_str().unwrap_or_default();
            assert!(
                !description.trim().is_empty(),
                "{schema_name}.{field} 应带中文描述（契约不可为裸字段）"
            );
        }
    }
}

/// kind 迁移为闭集枚举后，OpenAPI 契约锁：`Transaction.kind` 引用
/// `#/components/schemas/TransactionKind` 组件，组件为小写字符串枚举（与 wire 一致），
/// 而非 PascalCase 变体名或裸 string（issue #74 迁移锁）。
#[tokio::test]
async fn test_openapi_transaction_kind_is_lowercase_enum() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let schemas = doc["components"]["schemas"].as_object().unwrap();

    let tx = &schemas["Transaction"];
    let kind_ref = &tx["properties"]["kind"];
    assert_eq!(
        kind_ref["$ref"], "#/components/schemas/TransactionKind",
        "Transaction.kind 应为 TransactionKind 组件引用"
    );
    let kind_schema = &schemas["TransactionKind"];
    assert_eq!(
        kind_schema["type"], "string",
        "TransactionKind schema 应为 string"
    );
    let enum_values: Vec<&str> = kind_schema["enum"]
        .as_array()
        .expect("TransactionKind schema 应含 enum")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        enum_values,
        vec![
            "income", "expense", "transfer", "refund", "buy", "sell", "dividend", "split"
        ],
        "kind 枚举值应为闭集的 8 个小写字符串"
    );
}

#[tokio::test]
async fn test_openapi_doc_has_currencies_endpoint() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    let currencies = &paths["/api/v1/currencies"]["get"];
    assert!(currencies["summary"].is_string());
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    assert!(schemas.contains_key("Currency"));
    assert!(schemas.contains_key("TransactionInput"));
}

/// OpenAPI 契约文档体积预算护栏：当前 17 端点 ≈ 33KB，预算 40KB 留增长空间；
/// 端点继续增长触线时需人工决策（拆文档或提预算），避免契约文档无界膨胀挤占
/// AI 上下文（32KB 预算在基金查询端点加入时触线，issue #304 人工决策提至 40KB：
/// 17 端点下契约是 AI 教学的唯一权威文本，拆分反而破坏「一次拉取即自足」）。
#[tokio::test]
async fn test_openapi_doc_size_within_budget() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    assert!(
        bytes.len() <= 40 * 1024,
        "OpenAPI 契约文档应保持在预算内（当前 {} 字节，预算 40KB）",
        bytes.len()
    );
}

#[tokio::test]
async fn test_import_knowledge_returns_ok_as_text_plain() {
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

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/plain"),
        "应返回纯文本，实际 content-type: {content_type}"
    );

    let bytes = body_to_bytes(response.into_body()).await;
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.trim().is_empty(), "知识内容不应为空");
    assert!(
        text.contains("/api/v1/openapi.json"),
        "知识应内嵌 OpenAPI 文档地址"
    );
}

#[tokio::test]
async fn test_import_knowledge_covers_key_conventions() {
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
    let text = String::from_utf8(bytes).unwrap();

    let required_keywords = [
        "流入金额",
        "流出金额",
        "income",
        "expense",
        "transfer",
        "→",
        "无",
        "人民币",
        "CNY",
        "_cents",
        "YYYY-MM-DD",
        "dedup",
        "sha256",
        "account_id",
        "to_account_id",
        "currency_code",
        "dividend",
        // 投资交易教学关键词锁（issue #298）：锁定标的解析三步法、行字段约束、
        // 纠错与对账四要点的确定性措辞，防后续编辑静默丢失教学能力。
        "投资交易",
        "buy",
        "sell",
        "标的",
        "标的解析",
        "投资账户",
        "instrument_id",
        "quantity",
        "price_cents",
        "fee_cents",
        "GET /api/v1/instruments",
        "POST /api/v1/instruments",
        "重算",
        "部分卖出",
        // 基金申赎教学关键词锁（issue #304 / ADR-0039）：行拆解、费用归属、
        // 按代码查询优先、真实代码必带、不走名称充代码。
        "基金申赎",
        "申购",
        "赎回",
        "确认份额",
        "6 位代码",
        "GET /api/v1/funds",
        "名称充代码",
        // 个人间借贷教学关键词锁（issue #368 / ADR-0053）：落账映射方向
        // （借出=自资金账户转入 receivable、借入经 debt、还款反向转账、
        // 勿记成 expense 的示例句）、一人一账户命名约定、不带商户、利息
        // 才进收支、既有借贷经期初余额表达、AI 不代做核销（否定语义短语）。
        // 各词均属借贷节独有措辞，整节被误删或方向/否定语义被改时逐词报红。
        "个人间借贷",
        "receivable",
        "debt",
        "自资金账户转入",
        "借出·张三",
        "借入·李四",
        "反向转账",
        "部分还款即多笔",
        "张三借了我",
        "借贷行不带商户",
        "利息",
        "initial_balance_cents",
        "余额调整",
        "不自行清零余额",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
    }
}

#[tokio::test]
async fn test_openapi_doc_covers_knowledge_endpoint() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    let knowledge = &paths["/api/v1/import/knowledge"]["get"];
    assert!(knowledge["summary"].is_string());
    let description = knowledge["description"].as_str().unwrap_or_default();
    assert!(
        description.contains("导入全流程约定"),
        "导入知识端点自述应以「导入全流程约定」界定范围（issue #286）"
    );

    let responses = knowledge["responses"].as_object().unwrap();
    let ok = responses.get("200").expect("应包含 200 响应");
    let content = ok["content"].as_object().expect("200 响应应声明 content");
    assert!(
        content.contains_key("text/plain"),
        "knowledge 端点应声明 text/plain 响应"
    );
}

#[tokio::test]
async fn test_openapi_doc_covers_account_balances_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let get = &doc["paths"]["/api/v1/accounts/balances"]["get"];
    assert!(get["summary"].is_string());
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("应包含 schemas");
    let balance = schemas
        .get("AccountBalance")
        .expect("OpenAPI 应包含 AccountBalance schema");
    let props = balance["properties"].as_object().unwrap();
    assert!(props.contains_key("account"));
    assert!(props.contains_key("balance_cents"));
}

#[tokio::test]
async fn test_openapi_doc_covers_list_transactions_params_and_schema() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let get = &doc["paths"]["/api/v1/transactions"]["get"];
    assert!(get["summary"].is_string());
    let params = get["parameters"]
        .as_array()
        .expect("GET /transactions 应声明查询参数");
    let names: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
    for expected in [
        "from",
        "to",
        "account_id",
        "kind",
        "limit",
        "page",
        "page_size",
    ] {
        assert!(
            names.contains(&expected),
            "OpenAPI 应包含查询参数 {expected}"
        );
    }
    let response_200 = get["responses"]["200"]["content"]["application/json"]["schema"]
        .as_object()
        .unwrap();
    assert_eq!(
        response_200["$ref"], "#/components/schemas/TransactionListResult",
        "响应 schema 应为 TransactionListResult"
    );
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("应包含 schemas");
    let list_result = schemas
        .get("TransactionListResult")
        .expect("OpenAPI 应包含 TransactionListResult schema");
    let props = list_result["properties"].as_object().unwrap();
    assert!(props.contains_key("items"));
    assert!(props.contains_key("total"));
    let tx = schemas
        .get("Transaction")
        .expect("OpenAPI 应包含 Transaction schema");
    let props = tx["properties"].as_object().unwrap();
    for field in [
        "id",
        "kind",
        "amount_cents",
        "account_id",
        "date",
        "is_deleted",
    ] {
        assert!(props.contains_key(field), "Transaction 应包含字段 {field}");
    }
}

#[tokio::test]
async fn test_openapi_doc_covers_delete_account_and_category_endpoints() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;

    for (path, label) in [
        ("/api/v1/accounts/{id}", "账户"),
        ("/api/v1/categories/{id}", "分类"),
    ] {
        let delete = &doc["paths"][path]["delete"];
        assert!(
            delete["summary"].is_string(),
            "OpenAPI 应包含 {label} DELETE 端点"
        );
        let params = delete["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("{label} DELETE 应声明 path 参数"));
        assert!(
            params.iter().any(|p| p["name"] == "id"),
            "{label} DELETE 端点应声明 id 路径参数"
        );
        let responses = delete["responses"].as_object().unwrap();
        assert!(responses.contains_key("204"), "{label} 应声明 204 响应");
        assert!(responses.contains_key("404"), "{label} 应声明 404 响应");
    }
}
