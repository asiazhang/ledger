//! 商户端点：在用商户列表（AI 导入契约，issue #194 / ADR-0028）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::error::AppError;
use crate::merchants::Merchant;

/// 商户列表（AI 导入契约，issue #194 / ADR-0028）：供 AI 在提交交易前拉取在用商户，
/// 按已有名字填 `merchant_name` 复用字典（避免同义名分裂商户字典）。仅返回在用行
/// （`is_deleted=0`，与 IPC `list_merchants` 缺省一致）；软删商户不可再被新交易选择。
#[utoipa::path(
    get,
    path = "/api/v1/merchants",
    tag = "merchants",
    summary = "列出所有在用商户",
    description = "返回商户字典的全部在用行（`is_deleted=0`），按名称排序。\
                  提交交易时可带 `merchant_name`（商户名字符串）：后端按名字精确匹配在用商户，\
                  命中复用、未命中即建，AI 无需自行去重；建议先拉取本列表、按已有名字提交，\
                  避免同义名分裂商户字典。仅 `income`/`expense` 可携带商户（refund 自动继承原支出商户）。",
    responses(
        (status = 200, description = "在用商户列表", body = [Merchant]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_merchants_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Merchant>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::merchants::list_merchants(&conn, false)?))
}
