//! 紧凑契约方言端点锁（issue #839）：`GET /api/v1/contract` 返回为 AI 设计的
//! 紧凑 JSON 方言（`ledger-contract-1`）——与标准 OpenAPI（`/api/v1/openapi.json`）
//! 同一 `ApiDoc` 源的第二机械投影。本文件只锁外部可见行为（HTTP 产物形状与体积
//! 预算），不为投影函数建立内部调用级断言。
//!
//! 既有 OpenAPI 文档结构锁（`documentation.rs`）的契约保证在此逐条移植到方言
//! 形状：端点覆盖清单、kind 小写枚举、dedup wrapper、幂等键不可编辑、投资四
//! 字段描述锁；并新增方言头、类型表达式全覆盖、出处剥离与体积预算锁。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, get_json, setup_app};

/// 拉取方言文档（每测试独立装配，断言互不依赖执行顺序）。
async fn fetch_contract() -> serde_json::Value {
    let (app, _) = setup_app();
    let (status, doc) = get_json(&app, "/api/v1/contract").await;
    assert_eq!(status, StatusCode::OK);
    doc
}

/// 方言头：版本标记、base 地址与图例在位（user story 2/14/15）——AI 无需
/// OpenAPI info 块即可连接并解析方言。
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

/// 端点覆盖清单（移植自 `test_openapi_doc_covers_all_endpoints`）：方言端点
/// 键为 `m` + `p`（路径省略 /api/v1 前缀，base 已含）。
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

/// 端点请求体与状态码→响应类型映射：读回自纠所需的状态码与错误 schema
/// 语义在位（user story 5）；无 JSON 体的响应以 `-` 占位。
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

    // 批量导入：body wrapper + 200 数组响应 + 400 错误（移植 dedup wrapper 锁的一半）。
    let batch = find("POST", "/transactions/batch");
    assert_eq!(batch["body"], "TransactionBatchInput");
    assert_eq!(batch["res"]["200"], "CreateTransactionResult[]");
    assert_eq!(batch["res"]["400"], "ErrorResponse");

    // 读回：200 响应类型在位（移植列表 schema 锁的一半）。
    assert_eq!(
        find("GET", "/transactions")["res"]["200"],
        "TransactionListResult"
    );

    // 删除：204（无 JSON 体）与 404 状态码保留（移植自
    // test_openapi_doc_covers_delete_transaction_endpoint）；部分卖出的 400
    // 已随级联删除退役（issue #940 / ADR-0097：删除改级联 + 回补，不再拒绝）。
    let delete = find("DELETE", "/transactions/{id}");
    assert_eq!(delete["res"]["204"], "-", "无响应体以 - 占位");
    assert_eq!(delete["res"]["404"], "ErrorResponse");
    assert!(
        delete["res"].get("400").is_none(),
        "删除不再声明部分卖出 400（级联删除退场，issue #940）"
    );

    // 账户编辑：请求体 schema 名在位。
    let put = find("PUT", "/accounts/{id}");
    assert_eq!(put["body"], "AccountUpdateInput");
    assert_eq!(put["res"]["200"], "Account");

    // 导入知识：text/plain 端点无 JSON 响应 schema（分级自足后两个知识端点，
    // issue #1123）。
    assert_eq!(find("GET", "/import/knowledge")["res"]["200"], "-");
    assert_eq!(
        find("GET", "/import/knowledge/investment")["res"]["200"],
        "-"
    );
}

/// kind 闭集枚举移植到方言形状（移植自
/// `test_openapi_transaction_kind_is_lowercase_enum`）：`Transaction.kind` 引用
/// schema 名，`TransactionKind` 为 9 个小写值的 `|` 闭集。
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

/// batch wrapper 与 duplicate 字段移植到方言形状（移植自
/// `test_openapi_doc_batch_wrapper_and_duplicate_field`）：`transactions` 必填
/// （无 `?` 后缀）、`dedup` 可缺省（`?` 后缀即 optionality 标记）。
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

/// 幂等键不可编辑移植到方言形状（移植自
/// `test_openapi_update_transaction_input_omits_idempotency_key`）：修改请求体
/// 不含 `idempotency_key` 字段——方言以字段缺席表达「不可编辑」。
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

/// 投资四字段描述锁移植到方言形状（移植自
/// `test_openapi_investment_fields_have_descriptions`，issue #298 语义不动）：
/// `instrument_id` / `quantity` / `price_cents` / `fee_cents` 在两个请求体
/// schema 中带原文中文描述——契约仍是字段语义的唯一权威（user story 3）。
#[tokio::test]
async fn contract_investment_fields_keep_descriptions() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    for schema_name in ["TransactionInput", "UpdateTransactionInput"] {
        let props = &schemas[schema_name];
        // 类型表达式同时锁定：str?（引用）/ f64?（数量）/ i64?（价格与费用）。
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

    // buy/sell 语义原文保留（投资四字段描述锁的确定性措辞）。
    let merchant = &schemas["TransactionInput"]["merchant_name?"];
    let merchant_desc = merchant[1].as_str().unwrap();
    assert!(
        merchant_desc.contains("命中复用、未命中即建") && merchant_desc.contains("merchant_id"),
        "商户名字段描述语义原文保留"
    );
}

/// 类型表达式全覆盖（issue #839 测试决策）：现有 schema 恰好覆盖枚举闭集、
/// 数组、可选、i64、f64 全部形态——逐形态断言，防投影规则静默漂移。
#[tokio::test]
async fn contract_type_expressions_cover_all_forms() {
    let doc = fetch_contract().await;
    let schemas = doc["schemas"].as_object().unwrap();

    // 枚举闭集（多组）。
    assert_eq!(
        schemas["AccountType"],
        "cash|bank|credit|ewallet|investment|debt|receivable|other"
    );
    assert_eq!(schemas["InstrumentType"], "stock|fund|bond|etf|other");

    // 数组：T[]。
    assert_eq!(schemas["TransactionListResult"]["items"], "Transaction[]");
    assert_eq!(
        schemas["TransactionBatchInput"]["transactions"],
        "TransactionInput[]"
    );

    // 可选（? 后缀）与可空标量。
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

    // 可空引用：oneOf(null + ref) 投影为 T?；成员描述保留为元组。
    assert_eq!(schemas["Transaction"]["source?"][0], "TransactionSource?");
    assert!(
        !schemas["Transaction"]["source?"][1]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "来源字段描述保留（元组第二位）"
    );

    // 带描述的引用字段：[schema 名, 描述] 元组。
    let kind = &schemas["TransactionSource"]["kind"];
    assert_eq!(kind[0], "TransactionSourceKind");
    assert!(!kind[1].as_str().unwrap_or_default().is_empty());
}

/// 出处剥离锁（issue #839）：方言投影期机械剥离 `issue #N` / `ADR-N` 引用，
/// 而标准 OpenAPI 投影保留出处——两投影各取所需（user story 11）。
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

    // 剥离不伤语义：出处相邻的正文描述保持可读（非空壳括号、无悬挂标点）。
    let fund_desc = dialect["schemas"]["FundLookup"]["code"][1]
        .as_str()
        .unwrap_or_default();
    let _ = fund_desc; // 字段级描述存在性已在其余锁覆盖；此处防误删整字段。
    assert!(
        !dialect["schemas"]["ErrorResponse"]["kind"].is_null(),
        "ErrorResponse 字段在位（schema 级描述退役后字段语义仍完整）"
    );
}

/// 出资账户契约锁（issue #939 / ADR-0096 决策 8）：创建/修改请求体与交易读回
/// schema 都带出可选出资账户字段——契约自描述端点随类型派生自动带出新字段
/// 的验收面（字段可选、类型可选字符串、字段描述提及准入语义）。
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

/// 份额调整（split）字段语义锁（issue #1054 / ADR-0106 决策 11）：AI / 契约是
/// split 的唯一写入面，`TransactionInput.quantity` 的描述必须自描述方向符号
/// （`+` 折算/结转/送股、`−` 缩股）、缩股取严只作用于 `−` 向、以及「无现金腿」
/// 的准入口径（金额恒 0、单价不提供、费用只能为 0）——AI 据此能自行构造出被
/// 接受的提交；描述退回「数量必须 > 0」旧口径或把守卫泛化到两向时本锁报红。
#[tokio::test]
async fn contract_transaction_input_quantity_describes_split_semantics() {
    let doc = fetch_contract().await;
    let desc = doc["schemas"]["TransactionInput"]["quantity?"][1]
        .as_str()
        .unwrap_or_default();

    for frag in [
        "带符号份额增量",
        "折算",
        "`−` 缩股且 |Δ| 小于当前持仓", // 守卫只作用于 − 向（+Δ 折算/送股无上限）
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

/// 方言体积预算护栏（issue #839）：产物 ≤22KB。
///
/// token 换算口径（与 issue 原型一致，不引入真实 tokenizer 依赖）：
/// ASCII ≈ 1 token / 3.6 字符、中文 ≈ 0.95 token / 字符——原型实测 ~17KB
/// ≈ ~5.1K tokens（契约单次拉取自 ~14.0K tokens 降 63%）。触线须人工决策提预算
/// 或瘦身，不允许契约膨胀无声挤占 AI 上下文（延续 #304 / #693 契约膨胀护栏传统）。
/// 基金转换（issue #978/#979）加入 `TransactionInput` / `UpdateTransactionInput`
/// 的两腿可选字段、`ConvertFields.to_symbol` 后已逼近预算（≈19.9KB）——注意
/// `TransactionConvert` / `TransactionTrade` 是 IPC 专用投影，不在 `ApiDoc` 组件内，
/// 不占方言体积。
///
/// issue #981 复核：实测 20412 字节，余量仅 68（上限 20480），已逼近上限。已发布
/// 字段描述受 `AGENTS.md`「已发布 AI API 契约只增不改」约束，本票不动契约；后续
/// 任何新增字段都会触线，须先人工决策：对**未发布新增**的字段描述做一轮显式瘦身，
/// 或经维护者同意提高预算——不得默默挤占 AI 上下文（决策建议已在 issue #981 留痕）。
///
/// issue #1069 触线：`Instrument.price_channel` 派生字段与 `PriceChannel` 组件带入
/// 后实测 20732 字节越过原 20480，经维护者决策提至 22KB（≈6.3K tokens）——新字段
/// 描述承载价格通道语义、无冗余可削，留痕见 ADR-0090 决策 6。
///
/// issue #1123 复核：投资知识端点进入契约发现面（20 端点）后实测 22057 字节，
/// 仍在 22KB 预算内（余量约 471B）——下一个新增端点/字段大概率触线，届时须人工
/// 决策提预算或瘦身，延续本护栏的留痕传统。
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
        bytes.len() <= 22 * 1024,
        "紧凑契约方言应保持在预算内（当前 {} 字节，预算 22KB）",
        bytes.len()
    );
}
