use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, get_json, setup_app};

/// 投资五节标题（与 handler 常量结构锁同源）：基础知识端点响应的排他断言、
/// 投资端点响应的存在断言，都以节标题为锚（issue #1123 分级自足）。
const INVESTMENT_SECTION_HEADERS: [&str; 5] = [
    "## 投资交易（buy / sell）",
    "## 基金申赎（buy / sell，场外基金）",
    "## 基金转换（convert，场外基金）",
    "## 份额调整（split，场外基金与股票同构）",
    "## 现金分红（dividend，股息 / 基金分红 / 投顾组合分红同构）",
];

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

/// OpenAPI 契约文档体积预算护栏：20 端点实测 47204 字节（issue #1123 投资知识
/// 端点加入后复核），预算 48KB 留增长空间；端点继续增长触线时需人工决策（拆文档
/// 或提预算），避免契约文档无界膨胀挤占 AI 上下文（32KB 预算在基金查询端点加入时
/// 触线，issue #304 人工决策提至 40KB；40KB 在股票查询端点加入时触线，issue #693
/// 人工决策提至 48KB：18 端点下契约是 AI 教学的唯一权威文本，拆分反而破坏
/// 「一次拉取即自足」）。
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
        bytes.len() <= 48 * 1024,
        "OpenAPI 契约文档应保持在预算内（当前 {} 字节，预算 48KB）",
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
    // 分级自足（issue #1123 / ADR-0110）：基础知识 = 知识索引 + 非投资节。
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

/// 投资知识端点可达（issue #1123）：分级自足的按需分支——记账会话多数行与
/// 投资无关，只有索引触发时才拉本端点；删除路由注册本测试即红（接线型负向，
/// 断言对准端点可达与响应内容，ADR-0087）。
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

    // 基础知识关键词锁（issue #1123 分级自足拆分）：只锁基础知识端点自身应
    // 携带的约定——金额与日期口径、每行拆解、商户、个人间借贷、幂等去重、
    // 对账与纠错、知识索引；投资五节的关键词锁随节正文迁至投资端点测试
    // （见下方投资锁测试）。
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
        // 拆行口径：投资流水四类 kind 的可用性陈述仍在基础侧（跨切面归属
        // 收口见 issue #1124），「四者均可用」措辞退回旧限制口径时报红。
        "四者均可用",
        // 知识索引（issue #1123 / ADR-0110）：目录段与投资端点指针在位——
        // 删除索引条目即红（接线型，断言对准响应体内容）。
        "## 知识索引",
        "GET /api/v1/import/knowledge/investment",
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
        // 契约端点教学迁入锁（issue #839 → #931 反向迁出）的基础侧残留：
        // 读回确定性排序与多类型过滤用法（恒 unknown / 市场保留随投资节正文
        // 迁至投资端点锁测试）。
        "稳定排序", // 读回确定性排序（流程保证）
        "逗号分隔", // kinds 多类型过滤（读回用法）
        // 出资账户教学关键词锁（issue #939 / ADR-0096 决策 8）的基础侧：去重
        // 哈希纳入出资账户——新表述被误删或退回旧口径时逐词报红；直扣/直付
        // 携带与现金腿归属的锁随投资节正文在投资端点锁测试。
        "出资账户",
        "仅出资账户不同的两笔不互相去重",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
    }
}

/// 投资知识端点关键词锁（issue #1123 分级自足拆分）：投资五节的教学能力
/// 全部迁至本端点——标的解析三步法、基金申赎、基金转换、份额调整、现金
/// 分红及出资账户投资侧的确定性措辞，整节被误删或口径回退时逐词报红。
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
        // 投资交易三步法关键词锁（issue #694 / ADR-0081）：查询未命中/东财
        // 降级的流程措辞；市场推断/类型提示/回填落价等行为语义已随 #931
        // 迁回契约端点描述，锁在契约锁测试。
        "查无此码",
        "降级",
        "兜底建行",
        "恒 unknown", // fund 标的市场收口（知识侧差异教学保留）
        "市场保留",   // 股票降级建行保留解析市场（流程后果措辞）
        // 出资账户教学关键词锁（issue #939 / ADR-0096 决策 8）的投资侧：
        // 直扣/直付场景携带出资账户、余额核对含结算账户现金流（含出资账户）。
        "funding_account_id",
        "直扣/直付",
        "现金腿记出资账户",
        // 基金转换教学关键词锁（issue #981 / ADR-0099）：快照字段位置（转出端与
        // tradingTarget 转入端）、单腿一条记录、腿序幂等键、多腿转出份额两写、
        // 金额占比分摊与尾差末腿、余额不变对账口径、软删 + 重建纠错与 kind 变更
        // 禁用——整节被误删或口径退回借位落账时逐词报红。
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
        // 现金分红教学关键词锁（ADR-0109 / issue #1078）：字段位置（到账账户 /
        // 归属标的 / 金额）、「禁止落 income」、累计收益第三腿口径、对账与纠错
        // 边界、红利再投口径——整节被误删或退回普通收入承载时逐词报红。
        "现金分红（dividend",
        "到账账户",
        "归属于某标的",
        "累计收益",
        "第三腿",
        "不摊薄持仓成本",
        "红利再投",
        "kinds=dividend",
        // 份额调整教学关键词锁（ADR-0106 决策 11 / issue #1054）：字段位置、方向
        // 符号、无现金腿、守卫、成本与盈亏口径、余额不变而持仓市值随份额变化、
        // 纠错边界——整节被误删或口径退回买 / 卖借位落账时逐词报红。#1051 收编
        // 就地改 / 删后，纠错边界锁从「不可改、不可删」（过渡期限制，已失效）
        // 改为「可全字段替换修改、可软删 + 下游在用消耗守卫」。
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
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
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

    // 行为语义关键词锁（issue #931）：知识侧的契约第二副本剪除后，行为语义措辞
    // 回落契约端点描述（「手写零复述」）；本锁随内容迁至契约，防这些语义在
    // 注解层被静默删除（知识侧流程锁见上一个测试）。
    let required_keywords = [
        "类型提示",             // GET /stocks 返回（stock/etf）
        "精确市场",             // GET /stocks 返回与 POST /instruments 入参
        "权威名称",             // 东财校验回填（POST /instruments 描述）
        "北交所",               // 显式 400 边界（GET /stocks 描述）
        "美股",                 // ticker 遍历三市场（GET /stocks 描述）
        "静默复用",             // 标的 find-or-create（POST /instruments 描述）
        "回填权威名称与最新价", // 东财增强落价（POST /instruments 描述）
        "上限 100",             // 搜索封顶上限（GET /instruments 描述）
        "不影响其余行",         // 批量单行失败隔离（POST /transactions/batch 描述）
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "契约应包含行为语义关键词 {kw:?}");
    }
}

/// 两个知识端点的 OpenAPI 自述锁（issue #1123 分级自足）：基础知识自述以
/// 知识索引界定范围，投资节自述以投资五节界定范围；均声明 text/plain 200。
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
        // 类型维度唯一集合参数（spec #1025：单值 kind 参数已移除，BREAKING）
        "kinds",
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
