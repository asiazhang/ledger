//! 紧凑契约方言投影（`GET /api/v1/contract`，issue #839 / ADR-0090）。
//!
//! 方言（`ledger-contract-1`）不是手写文本，而是与标准 OpenAPI
//!（`GET /api/v1/openapi.json`，原样保留）**同一 [`ApiDoc`] 源的第二个机械
//! 投影**：输入 utoipa OpenApi 模型、输出紧凑 JSON 值，utoipa 派生、属性与
//! 装配零修改——「不改编译器，只加后端」。产物面向 AI 编程助手：剥离
//! OpenAPI 结构样板与出处引用（`issue #N` / `ADR-N`，出处保留在源注解里），
//! 端点路径省略 `/api/v1` 前缀（base 单列），字段级语义描述原文保留。
//!
//! 产物体积有字节预算护栏（≤22KB，测试 `contract_size_within_budget`），
//! 契约膨胀不允许无声挤占 AI 上下文。

use std::sync::{LazyLock, OnceLock};

use axum::Json;
use axum::response::IntoResponse;
use regex::Regex;
use utoipa::OpenApi as _;
use utoipa::openapi::path::Operation;
use utoipa::openapi::schema::{ArrayItems, Object, Ref, Schema, SchemaType, Type};
use utoipa::openapi::{OpenApi, RefOr};

use super::openapi::ApiDoc;

/// 方言版本标记：格式演进可被检测与协商（user story 14）。
const DIALECT_VERSION: &str = "ledger-contract-1";

/// 本机 base 地址：方言端点路径省略 `/api/v1` 前缀，连接所需信息单点落明。
const BASE_URL: &str = "http://127.0.0.1:9527/api/v1";

/// 一行图例：AI 无需 OpenAPI 样板知识即可解析方言（user story 2）。
const LEGEND: &str = "类型: str/i64/f64/bool/obj; T[] 数组; 后缀? 可选; 字段值[类型,说明]";

/// `GET /api/v1/contract`：返回紧凑契约方言 JSON（进程内一次性懒构建）。
///
/// 契约自举端点，与 `openapi_json_handler` 同属启动门豁免面（不含用户数据，
/// 不触 DB），锁定/启动失败期间照常可用。
pub async fn contract_handler() -> impl IntoResponse {
    static CONTRACT: OnceLock<serde_json::Value> = OnceLock::new();
    Json(CONTRACT.get_or_init(|| project(&ApiDoc::openapi())))
}

/// 投影单点：[`ApiDoc`] 标准模型 → 方言 JSON 值。纯函数，无 IO 无状态。
fn project(doc: &OpenApi) -> serde_json::Value {
    let endpoints: Vec<serde_json::Value> = doc
        .paths
        .paths
        .iter()
        .flat_map(|(path, item)| {
            [
                ("GET", item.get.as_ref()),
                ("PUT", item.put.as_ref()),
                ("POST", item.post.as_ref()),
                ("DELETE", item.delete.as_ref()),
            ]
            .into_iter()
            .filter_map(|(method, op)| op.map(|op| endpoint_value(method, path, op)))
            .collect::<Vec<_>>()
        })
        .collect();

    let mut schemas = serde_json::Map::new();
    for (name, schema) in doc.components.iter().flat_map(|c| c.schemas.iter()) {
        schemas.insert(name.clone(), schema_value(schema));
    }

    serde_json::json!({
        "v": DIALECT_VERSION,
        "base": BASE_URL,
        "legend": LEGEND,
        "endpoints": endpoints,
        "schemas": schemas,
    })
}

/// 端点投影：`m`/`p`/`s`/`d`/`body`/`res`（`p` 省略 `/api/v1` 前缀）。
fn endpoint_value(method: &str, path: &str, op: &Operation) -> serde_json::Value {
    let mut endpoint = serde_json::Map::new();
    endpoint.insert("m".into(), serde_json::json!(method));
    endpoint.insert(
        "p".into(),
        serde_json::json!(path.strip_prefix("/api/v1").unwrap_or(path)),
    );
    endpoint.insert(
        "s".into(),
        serde_json::json!(strip_provenance(op.summary.as_deref().unwrap_or_default())),
    );
    endpoint.insert(
        "d".into(),
        serde_json::json!(
            strip_provenance(op.description.as_deref().unwrap_or_default()).unwrap_or_default()
        ),
    );
    if let Some(body) = op.request_body.as_ref().and_then(|body| {
        body.content
            .get("application/json")
            .and_then(|c| c.schema.as_ref())
    }) {
        endpoint.insert("body".into(), serde_json::json!(type_expr(body, false)));
    }
    let mut res = serde_json::Map::new();
    for (status, response) in op.responses.responses.iter() {
        // Ref 响应与无 content 响应（204、text/plain）均以 `-` 占位：方言只
        // 承载 JSON 响应类型，状态码本身保留（读回自纠依据状态码 + ErrorResponse）。
        let schema = match response {
            RefOr::T(response) => response
                .content
                .get("application/json")
                .and_then(|c| c.schema.as_ref()),
            RefOr::Ref(_) => None,
        };
        res.insert(
            status.clone(),
            serde_json::json!(schema.map_or_else(|| "-".to_owned(), |s| type_expr(s, false))),
        );
    }
    endpoint.insert("res".into(), serde_json::Value::Object(res));
    serde_json::Value::Object(endpoint)
}

/// 组件投影：字符串枚举闭集 → `"a|b|c"`；对象 → 字段名（可选带 `?` 后缀）
/// → 类型表达式或 `[类型, 描述]` 元组（字段级语义描述原文保留，仅剥出处）。
fn schema_value(schema: &RefOr<Schema>) -> serde_json::Value {
    match schema {
        RefOr::T(Schema::Object(object)) if object.enum_values.is_some() => {
            serde_json::json!(type_expr(schema, false))
        }
        RefOr::T(Schema::Object(object)) => {
            let mut fields = serde_json::Map::new();
            for (name, field) in object.properties.iter() {
                let optional = !object.required.iter().any(|required| required == name);
                let mut key = name.clone();
                if optional {
                    key.push('?');
                }
                let ty = type_expr(field, optional);
                let description = description_of(field)
                    .and_then(strip_provenance)
                    .filter(|d| !d.is_empty());
                fields.insert(
                    key,
                    description.map_or(serde_json::json!(ty), |d| serde_json::json!([ty, d])),
                );
            }
            serde_json::Value::Object(fields)
        }
        // 兜底：非对象组件按类型表达式（当前契约组件全部为对象或字符串枚举）。
        other => serde_json::json!(type_expr(other, false)),
    }
}

/// 字段级描述提取（对象 / 数组 / 引用 / 组合成员均可能携带，原文保留）。
fn description_of(schema: &RefOr<Schema>) -> Option<&str> {
    match schema {
        RefOr::Ref(r) => (!r.description.is_empty()).then_some(r.description.as_str()),
        RefOr::T(Schema::Object(o)) => o.description.as_deref(),
        RefOr::T(Schema::Array(a)) => a.description.as_deref(),
        RefOr::T(Schema::OneOf(o)) => o.items.iter().find_map(description_of),
        RefOr::T(Schema::AnyOf(a)) => a.items.iter().find_map(description_of),
        RefOr::T(Schema::AllOf(a)) => a.items.iter().find_map(description_of),
        _ => None,
    }
}

/// 类型表达式：`$ref`→schema 名、数组→`T[]`、可空/可选→`?` 后缀、枚举→
/// `a|b|c` 闭集、integer/number→`i64`/`f64`、string/boolean/object→
/// `str`/`bool`/`obj`。
fn type_expr(schema: &RefOr<Schema>, optional: bool) -> String {
    let expr = match schema {
        RefOr::Ref(r) => ref_name(r),
        RefOr::T(s) => schema_expr(s),
    };
    if optional && !expr.ends_with('?') {
        format!("{expr}?")
    } else {
        expr
    }
}

fn schema_expr(schema: &Schema) -> String {
    match schema {
        Schema::Array(array) => format!("{}[]", array_items_expr(&array.items)),
        Schema::OneOf(o) => composite_expr(&o.items),
        Schema::AnyOf(a) => composite_expr(&a.items),
        Schema::AllOf(a) => composite_expr(&a.items),
        Schema::Object(object) => object_expr(object),
        // Schema 为 #[non_exhaustive]，未来新增形态兜底为 obj（当前契约不出现）。
        _ => "obj".to_owned(),
    }
}

/// 数组元素投影（`items: false` 形态当前契约不出现，兜底 obj）。
fn array_items_expr(items: &ArrayItems) -> String {
    match items {
        ArrayItems::RefOrSchema(schema) => type_expr(schema, false),
        ArrayItems::False => "obj".to_owned(),
    }
}

/// 对象/枚举投影：枚举闭集优先，其余按 schema type（type 数组含 null 即可空）。
fn object_expr(object: &Object) -> String {
    if let Some(values) = &object.enum_values {
        return values
            .iter()
            .map(|v| v.as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("|");
    }
    let (base, nullable) = match &object.schema_type {
        SchemaType::Type(ty) => (Some(ty), false),
        SchemaType::Array(types) => (
            types.iter().find(|ty| **ty != Type::Null),
            types.contains(&Type::Null),
        ),
        SchemaType::AnyValue => (None, false),
    };
    let name = base.map(type_name).unwrap_or("obj");
    if nullable {
        format!("{name}?")
    } else {
        name.to_owned()
    }
}

/// 组合类型（`oneOf(null, T)` = 可空引用）：交替项 `|` 连接，null 项折叠为 `?`。
fn composite_expr(items: &[RefOr<Schema>]) -> String {
    let mut nullable = false;
    let mut parts = Vec::new();
    for item in items {
        match item {
            RefOr::T(Schema::Object(object)) if matches!(&object.schema_type, SchemaType::Type(ty) if *ty == Type::Null) =>
            {
                nullable = true;
            }
            other => parts.push(type_expr(other, false)),
        }
    }
    let joined = parts.join("|");
    if nullable && !joined.ends_with('?') {
        format!("{joined}?")
    } else {
        joined
    }
}

fn type_name(ty: &Type) -> &'static str {
    match ty {
        Type::Object => "obj",
        Type::String => "str",
        Type::Integer => "i64",
        Type::Number => "f64",
        Type::Boolean => "bool",
        Type::Array | Type::Null => "obj",
    }
}

fn ref_name(r: &Ref) -> String {
    r.ref_location
        .rsplit('/')
        .next()
        .unwrap_or(&r.ref_location)
        .to_owned()
}

/// 机械剥离出处引用（issue #839 决策 5）：`issue #N` / `spec #N`（含紧随的
/// 「二期」）/ `ADR-N`（含紧随的「决策 N[/M]」），连带紧邻分隔符与残留的空
/// 括号/悬挂标点一并清理；出处保留在源注解里给开发者。已核对契约文本中裸
/// `#N` 仅以出处形态出现，末段兜底剥离安全。
/// 静态正则构造：模式全部为编译期内字面量，且方言投影产物由集成测试全量
/// 锁覆盖；构造失败属编程错误，fail loud（豁免贴最近语句，ADR-0060）。
fn static_regex(pattern: &'static str) -> Regex {
    #[allow(clippy::expect_used)] // 静态字面量正则，有效性由作者与投影锁保证（ADR-0060）
    Regex::new(pattern).expect("静态正则字面量有效")
}

fn strip_provenance(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    static ISSUE_SPEC_REF: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r#"\s*[/·、,，]?\s*(?:issue|spec)\s*#\d+(?:\s*二期)?"#));
    static ADR_REF: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r#"\s*[/·、,，]?\s*ADR-\d+(?:\s*决策\s*[\d/]+)?"#));
    // 已核对契约文本中裸 `#N` 仅以「issue #693/#696」同源串联形态残留，兜底剥离。
    static BARE_HASH_REF: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r"\s*[/·、]\s*#\d+|#\d+"));
    static NEWLINE_TRIM: LazyLock<Regex> = LazyLock::new(|| static_regex(r" ?\n ?"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| static_regex(r"[ \t]+"));
    // 悬挂标点清理：剥引用后残留的空括号、括号内首分隔符、结尾分隔符、
    // 标点前空白（「见 ADR-N，」→「见，」形态）。
    static EMPTY_PAREN: LazyLock<Regex> = LazyLock::new(|| static_regex(r"[（(][）)]"));
    static PUNCT_BEFORE_PAREN: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r"[，、；]\s*([）)])"));
    static PUNCT_AFTER_PAREN_OPEN: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r"([（(])[，、；:：]"));
    static SPACE_BEFORE_PUNCT: LazyLock<Regex> =
        LazyLock::new(|| static_regex(r"[ \t]+([，、；：）)])"));

    let mut text = ISSUE_SPEC_REF.replace_all(text, "").into_owned();
    text = ADR_REF.replace_all(&text, "").into_owned();
    text = BARE_HASH_REF.replace_all(&text, "").into_owned();
    text = NEWLINE_TRIM.replace_all(&text, "\n").into_owned();
    text = SPACES.replace_all(&text, " ").into_owned();
    // 悬挂标点可能连环残留（「（ / ，」多轮剥出），清理迭代至不动点（有界）。
    for _ in 0..4 {
        let before = text.clone();
        text = EMPTY_PAREN.replace_all(&text, "").into_owned();
        text = PUNCT_BEFORE_PAREN.replace_all(&text, "$1").into_owned();
        text = PUNCT_AFTER_PAREN_OPEN.replace_all(&text, "$1").into_owned();
        text = SPACE_BEFORE_PUNCT.replace_all(&text, "$1").into_owned();
        if text == before {
            break;
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
