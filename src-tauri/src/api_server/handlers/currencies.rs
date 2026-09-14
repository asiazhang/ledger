//! 币种端点：种子币种清单（导入映射用）。

use axum::Json;
use axum::extract::State;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ReadConn;
use crate::shell_support::read_entry::read_entry;
use ledger_currencies::Currency;
use ledger_infra::error::AppError;

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
    State(read): State<ReadConn>,
) -> Result<Json<Vec<Currency>>, AppError> {
    read_entry("GET /api/v1/currencies", read.0, move |conn| {
        Ok(Json(ledger_currencies::list_currencies(conn)?))
    })
    .await
}
