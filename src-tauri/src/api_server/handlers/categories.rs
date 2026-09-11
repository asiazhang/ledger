//! 分类端点：列表 / 幂等创建 / 软删除。
//!
//! 写端点经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）；
//! 读端点经 `run_db`（形状乙）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::EmitterSlot;
use crate::categories::{Category, CategoryInput};
use crate::error::AppError;
use crate::read_entry::read_entry;
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

#[utoipa::path(
    get,
    path = "/api/v1/categories",
    tag = "categories",
    summary = "列出所有分类",
    description = "返回全部分类（含种子数据），`kind` 为 `income` / `expense`，支持两级分类体系（`parent_id`）。",
    responses(
        (status = 200, description = "分类列表", body = [Category]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_categories_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<crate::categories::Category>>, AppError> {
    read_entry("GET /api/v1/categories", conn, move |conn| {
        Ok(Json(crate::categories::list_categories(conn, false)?))
    })
    .await
}

#[utoipa::path(
    post,
    path = "/api/v1/categories",
    tag = "categories",
    summary = "创建分类（按自然键幂等）",
    description = "按 `name` + `kind` + `parent_id` 幂等创建分类：已存在同名同类型同父分类且未删除的分类时，\
                  直接返回已有分类的 `id`，不重复插入、不报错。`parent_id` 为 `null` 表示顶级分类。",
    request_body = CategoryInput,
    responses(
        (status = 201, description = "创建成功，返回分类 ID", body = String),
        (status = 400, description = "参数错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn create_category_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    body: String,
) -> Result<(StatusCode, Json<String>), AppError> {
    // 请求体反序列化失败保留裸 Invalid（ADR-0050 决策 2 允许的「程序性/内部
    // 错误」，#1072 明确不转）：message 是 JSON 解析器的技术错误原文（英文、
    // 位置相关），没有可逐字保留的中文模板，与同族的 sync_engine 序列化失败
    // 同待遇；HTTP 400 与 `kind` 形状不变。
    let input: CategoryInput =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    write_entry(
        "POST /api/v1/categories",
        conn,
        emitter.as_deref(),
        WriteOp::CreateCategory,
        move |conn| crate::categories::create_category_idempotent(conn, input).map(Outcome::Silent),
    )
    .await
    .map(|id| (StatusCode::CREATED, Json(id)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/categories/{id}",
    tag = "categories",
    summary = "删除分类（软删除）",
    description = "按 `id` 软删除分类（`is_deleted=1`）。**不校验引用**（与 UI 行为一致）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "分类 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 404, description = "分类不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn delete_category_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    write_entry(
        "DELETE /api/v1/categories/{id}",
        conn,
        emitter.as_deref(),
        WriteOp::DeleteCategory,
        move |conn| crate::categories::delete_category(conn, &id).map(Outcome::Silent),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
