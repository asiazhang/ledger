//! 币种端点：种子币种清单（导入映射用）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::currencies::Currency;
use crate::error::AppError;

#[utoipa::path(
    get,
    path = "/api/v1/currencies",
    tag = "currencies",
    summary = "列出所有币种",
    description = "返回全部种子币种清单（含 `人民币→CNY`、`港币→HKD`）。\
                  导入时可用它把源数据的中文币种名映射为 `currency_code`。",
    responses(
        (status = 200, description = "币种清单", body = [Currency]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_currencies_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Currency>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::currencies::list_currencies(&conn)?))
}
