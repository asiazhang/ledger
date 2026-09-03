//! 账户端点：列表（含黑洞账户）/ 幂等创建 / 编辑 / 软删除 / 实时余额。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::EmitterSlot;
use crate::api_server::write_ops::emit_after_write;
use crate::error::AppError;
use crate::models::{Account, AccountBalance, AccountInput, AccountUpdateInput};
use crate::signals::{WriteEvidence, WriteOp};

#[utoipa::path(
    get,
    path = "/api/v1/accounts",
    tag = "accounts",
    summary = "列出所有账户",
    description = "返回账户完整列表，**包含预置黑洞账户**（`is_hidden=true`）。\
                  AI 可把 `资金账户=无` 的交易映射到对应币种的黑洞账户。",
    responses(
        (status = 200, description = "账户列表（含黑洞账户）", body = [Account]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_accounts_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Account>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let accounts = crate::accounts::list_accounts_for_api(&conn)?;
    Ok(Json(accounts))
}

#[utoipa::path(
    post,
    path = "/api/v1/accounts",
    tag = "accounts",
    summary = "创建账户（按自然键幂等）",
    description = "按 `name` + `type` + `currency_code` 幂等创建账户：已存在同名同类型同币种且未删除的账户时，\
                  直接返回已有账户的 `id`，不重复插入、不报错。",
    request_body = AccountInput,
    responses(
        (status = 201, description = "创建成功，返回账户 ID", body = String),
        (status = 400, description = "参数错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn create_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Json(input): Json<AccountInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = crate::db::write(&conn, |conn| {
        crate::accounts::create_account_idempotent(conn, input)
    })?;
    emit_after_write(&emitter, WriteOp::CreateAccount, WriteEvidence::None);
    Ok((StatusCode::CREATED, Json(id)))
}

#[utoipa::path(
    put,
    path = "/api/v1/accounts/{id}",
    tag = "accounts",
    summary = "编辑账户（改名/改币种）",
    description = "按 `id` 编辑账户，可选字段未传保持原值。`type` 不可改（参与余额符号归属）；\
                  `currency_code` 仅无交易账户可改（有交易时后端拒绝，避免历史折算口径错乱）。\
                  余额校准不走本端点：创建一笔与黑洞账户的转账即可（余额 = 期初 + Σ 流水）。\
                  不存在的 id 返回 404。成功返回 200 与更新后的完整账户。",
    request_body = AccountUpdateInput,
    params(
        ("id" = String, Path, description = "账户 ID")
    ),
    responses(
        (status = 200, description = "更新后的完整账户", body = Account),
        (status = 400, description = "参数错误（如名称为空、有交易改币种、未知币种）", body = ErrorResponse),
        (status = 404, description = "账户不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn update_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<AccountUpdateInput>,
) -> Result<Json<Account>, AppError> {
    // 连接层统一写入口（ADR-0032）：修改与读回同一写闭包，提交点置脏/检查单点。
    let updated = crate::db::write(&conn, |conn| {
        crate::accounts::update_account(conn, &id, input)?;
        crate::accounts::get_account(conn, &id)
    })?;
    emit_after_write(&emitter, WriteOp::UpdateAccount, WriteEvidence::None);
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/accounts/{id}",
    tag = "accounts",
    summary = "删除账户（软删除）",
    description = "按 `id` 软删除账户（`is_deleted=1`）。**不校验引用**（与 UI 行为一致：删除有交易的账户后历史交易仍保留）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "账户 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 404, description = "账户不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn delete_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| crate::accounts::delete_account(conn, &id))?;
    emit_after_write(&emitter, WriteOp::DeleteAccount, WriteEvidence::None);
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/accounts/balances",
    tag = "accounts",
    summary = "列出全部未删除账户的实时余额（含黑洞账户）",
    description = "返回 `{account, balance_cents}[]`，与 AI 侧账户列表一致**包含 `is_hidden` 黑洞账户**。\
                  余额口径 = 初始余额 + 收入 − 支出 + 转入 − 转出 + 退款，实时计算不持久化。\
                  软删除账户不在列表中。转账分别计入转出与转入账户。",
    responses(
        (status = 200, description = "账户余额列表（含黑洞账户）", body = [AccountBalance]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_account_balances_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<AccountBalance>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let balances = crate::accounts::list_account_balances_for_api(&conn)?;
    Ok(Json(balances))
}
