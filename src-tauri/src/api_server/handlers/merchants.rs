use axum::Json;
use axum::extract::{Path, State};

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::{EmitterSlot, ReadConn};
use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbWriteHandle;
use ledger_infra::error::AppError;
use ledger_infra::signals::WriteOp;
use ledger_merchants::{Merchant, MerchantUpdateInput};

#[utoipa::path(
    get,
    path = "/api/v1/merchants",
    tag = "merchants",
    summary = "列出所有在用商户",
    description = "返回商户字典的全部在用行（`is_deleted=0`），按名称排序。\
                  提交交易时可带 `merchant_name`（商户名字符串）：后端按名字精确匹配在用商户，\
                  命中复用、未命中即建，AI 无需自行去重；建议先拉取本列表、按已有名字提交，\
                  避免同义名分裂商户字典。仅 `income`/`expense`/`transfer` 可携带商户（refund 自动继承原支出商户；
                  `buy`/`sell` 不能带）。",
    responses(
        (status = 200, description = "在用商户列表", body = [Merchant]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_merchants_handler(
    State(read): State<ReadConn>,
) -> Result<Json<Vec<Merchant>>, AppError> {
    read_entry("GET /api/v1/merchants", read.0, move |conn| {
        Ok(Json(ledger_merchants::list_merchants(conn, false)?))
    })
    .await
}

#[utoipa::path(
    put,
    path = "/api/v1/merchants/{id}",
    tag = "merchants",
    summary = "商户改名",
    description = "按 `id` 商户改名：`name` 可省略（省略即保持原值）。入参先 trim（与导入即建同款归一），\
                  trim 后为空返回 400（`merchant.name-required`）；改名撞在用同名返回 400 \
                  （`merchant.already-exists`，不做同义合并——改名不迁移交易引用，读回发现同义碎商户时\
                  逐笔改挂交易后再删除无法由本端点完成）。不存在或已软删的 id 返回 404。\
                  改名即时生效：交易以 `merchant_id` 引用，不回刷历史交易行（ADR-0028）。\
                  成功返回 200 与更新后的完整商户。",
    request_body = MerchantUpdateInput,
    params(
        ("id" = String, Path, description = "商户 ID")
    ),
    responses(
        (status = 200, description = "更新后的完整商户", body = Merchant),
        (status = 400, description = "参数错误（名称为空或撞在用同名）", body = ErrorResponse),
        (status = 404, description = "商户不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn update_merchant_handler(
    State(conn): State<DbWriteHandle>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<MerchantUpdateInput>,
) -> Result<Json<Merchant>, AppError> {
    let updated = write_entry(
        "PUT /api/v1/merchants/{id}",
        conn,
        emitter.as_deref(),
        WriteOp::UpdateMerchant,
        move |conn| {
            ledger_merchants::update_merchant(conn, &id, input)?;
            let updated = ledger_merchants::get_merchant(conn, &id)?;
            Ok(Outcome::Silent(updated))
        },
    )
    .await?;
    Ok(Json(updated))
}
