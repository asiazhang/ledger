use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, get_json, setup_app};
use tauri_app_lib::api_server::INVESTMENT_SECTION_HEADERS;

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
        ("/api/v1/stocks/{code}", "get"),
        ("/api/v1/merchants", "get"),
        ("/api/v1/merchants/{id}", "put"),
        ("/api/v1/transactions", "get"),
        ("/api/v1/transactions/batch", "post"),
        ("/api/v1/transactions/{id}", "delete"),
        ("/api/v1/transactions/{id}", "put"),
        ("/api/v1/import/knowledge", "get"),
        ("/api/v1/import/knowledge/investment", "get"),
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

    let result = &schemas["CreateTransactionResult"];
    assert!(
        result["properties"]["duplicate"].is_object(),
        "CreateTransactionResult 应包含 duplicate 字段"
    );

    let account = &schemas["Account"];
    assert!(
        account["properties"]["is_hidden"].is_object(),
        "Account 应包含 is_hidden 字段"
    );
    assert!(
        account["properties"]["is_archived"].is_object(),
        "Account 应包含 is_archived 字段（只增披露）"
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
            "income", "expense", "transfer", "refund", "buy", "sell", "dividend", "split",
            "convert"
        ],
        "kind 枚举值应为闭集的 9 个小写字符串"
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
        bytes.len() <= 62 * 1024,
        "OpenAPI 契约文档应保持在预算内（当前 {} 字节，预算 62KB）",
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
        text.contains("/api/v1/contract"),
        "知识应内嵌紧凑契约方言地址（issue #839：契约自描述形态换轨，教学同步换指向）"
    );
    assert!(
        text.contains("## 知识索引"),
        "基础知识应含知识索引段（分域知识节的目录与触发单点）"
    );
    assert!(
        text.contains("GET /api/v1/import/knowledge/investment"),
        "知识索引应带投资节端点指针（AI 据此按需获取投资五节）"
    );
    for header in INVESTMENT_SECTION_HEADERS {
        assert!(
            !text.contains(header),
            "基础知识不得含投资五节正文 {header}（投资节正文只在投资端点）"
        );
    }
}

#[tokio::test]
async fn test_import_investment_knowledge_returns_ok_as_text_plain() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/knowledge/investment")
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
    assert!(!text.trim().is_empty(), "投资知识内容不应为空");
    for header in INVESTMENT_SECTION_HEADERS {
        assert!(text.contains(header), "投资知识应包含投资节标题 {header}");
    }
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
        "投资流水不落这三类",
        "投资流水的商户规则不在本页陈述",
        "由投资节承载",
        "标的解析与投资落账",
        "即须先拉取投资节再落账",
        "整场会话黏住",
        "不得套用普通收支写法",
        "现金分红五节",
        "## 知识索引",
        "GET /api/v1/import/knowledge/investment",
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
        "稳定排序",
        "逗号分隔",
        "单请求行数上限为 100",
        "必须分页读回",
        "即已读全",
        "transaction.page-size-over-cap",
        "transaction.limit-out-of-range",
        "出资账户",
        "仅出资账户不同的两笔不互相去重",
        "购买项",
        "purchases",
        "unit_price_cents",
        "只有 `expense` 可带购买项",
        "不再写进 `note` 备注",
        "价格拿不到就留空，不猜不编造",
        "金额最大的商品",
        "按件数最大的商品退让",
        "按提交顺序稳定返回",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
    }
}

#[tokio::test]
async fn test_import_investment_knowledge_covers_key_conventions() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/knowledge/investment")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let text = String::from_utf8(bytes).unwrap();

    let required_keywords = [
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
        "基金申赎",
        "申购",
        "赎回",
        "确认份额",
        "6 位代码",
        "GET /api/v1/funds",
        "名称充代码",
        "查无此码",
        "降级",
        "兜底建行",
        "恒 unknown",
        "市场保留",
        "funding_account_id",
        "直扣/直付",
        "现金腿记出资账户",
        "基金转换",
        "convert",
        "tradingTarget",
        "convertAmount",
        "无现金腿",
        "单腿一条记录",
        "逐腿直读",
        "金额占比拆分",
        "快照口径假设",
        "尾差末腿",
        "结转成本",
        "扣费后净份额",
        "kind 改成 / 改出",
        "假卖出",
        "现金分红（dividend",
        "到账账户",
        "归属于某标的",
        "累计收益",
        "第三腿",
        "不摊薄持仓成本",
        "红利再投",
        "到账账户 = 投资账户",
        "自相抵",
        "kinds=dividend",
        "份额调整（split",
        "带符号份额增量",
        "缩股",
        "Δ 不能为 0",
        "严格小于",
        "有在用持仓",
        "贡献恒为 0",
        "随份额变化",
        "可全字段替换修改、可软删",
        "码化守卫拒绝",
        "kind 清单与通用规则",
        "共五个 kind 值",
        "一律不带商户",
        "提交会被拒绝",
        "source",
        "entity_id",
        "display_name",
        "转出标的",
        "不经交易读回返回",
        "kinds=buy,sell",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
    }
}

#[tokio::test]
async fn test_knowledge_sections_are_data_source_neutral() {
    let (app, _) = setup_app();

    for uri in [
        "/api/v1/import/knowledge",
        "/api/v1/import/knowledge/investment",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = body_to_bytes(response.into_body()).await;
        let text = String::from_utf8(bytes).unwrap();
        for banned in ["东财", "东方财富", "eastmoney"] {
            assert!(
                !text.contains(banned),
                "{uri} 应数据源中立，不得出现数据源名 {banned:?}（ADR-0130）"
            );
        }
    }
}

#[tokio::test]
async fn test_contract_covers_behavior_semantics_keywords() {
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
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let text = serde_json::to_string(&value).unwrap();

    let required_keywords = [
        "类型提示",
        "精确市场",
        "权威名称",
        "北交所",
        "美股",
        "静默复用",
        "回填权威名称与最新价",
        "上限 100",
        "不影响其余行",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "契约应包含行为语义关键词 {kw:?}");
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
    for (path, desc_frag) in [
        ("/api/v1/import/knowledge", "知识索引"),
        ("/api/v1/import/knowledge/investment", "投资交易"),
    ] {
        let knowledge = &paths[path]["get"];
        assert!(knowledge["summary"].is_string(), "{path} 应有 summary");
        let description = knowledge["description"].as_str().unwrap_or_default();
        assert!(
            description.contains(desc_frag),
            "{path} 自述应以「{desc_frag}」界定范围，实际: {description}"
        );

        let responses = knowledge["responses"].as_object().unwrap();
        let ok = responses
            .get("200")
            .unwrap_or_else(|| panic!("{path} 应包含 200 响应"));
        let content = ok["content"].as_object().expect("200 响应应声明 content");
        assert!(
            content.contains_key("text/plain"),
            "{path} 应声明 text/plain 响应"
        );
    }
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
        "kinds",
        "hide_investment_related",
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
    let get_desc = get["description"].as_str().unwrap_or_default();
    assert!(
        get_desc.contains("HTTP 单请求行数上限 100"),
        "GET /transactions 契约自述应声明缺省上限，实际: {get_desc}"
    );
    assert_eq!(
        get["responses"]["400"]["description"],
        "分页参数超上限：page_size > 100（transaction.page-size-over-cap）或 limit 超范围/负值（transaction.limit-out-of-range）",
        "400 响应应声明两码化条件"
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
