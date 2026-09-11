//! 商户端点：在用商户列表 + 改名（AI 导入契约，issue #194 / ADR-0028 / #884）。
//!
//! 写端点经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 事务、置脏、信号内化单点（与 IPC `update_merchant` 共享 `WriteOp::UpdateMerchant`，
//! 参考数据写入静态映射发参考失效信号）；读端点经 `run_db`（形状乙）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path, State};
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::EmitterSlot;
use crate::error::AppError;
use crate::merchants::{Merchant, MerchantUpdateInput};
use crate::read_entry::read_entry;
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

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
                  避免同义名分裂商户字典。仅 `income`/`expense`/`transfer` 可携带商户（refund 自动继承原支出商户；
                  `buy`/`sell` 不能带）。",
    responses(
        (status = 200, description = "在用商户列表", body = [Merchant]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_merchants_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Merchant>>, AppError> {
    read_entry("GET /api/v1/merchants", conn, move |conn| {
        Ok(Json(crate::merchants::list_merchants(conn, false)?))
    })
    .await
}

/// 商户改名（issue #884）：与 IPC `update_merchant` 共用域层 `update_merchant`
///（入参先 trim，trim 后为空 400；撞在用同名 400，不做同义合并——改名不迁移交易
/// 引用，ADR-0028）与 `WriteOp::UpdateMerchant`（参考失效信号单一映射）。
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
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<MerchantUpdateInput>,
) -> Result<Json<Merchant>, AppError> {
    // 修改与读回同一写闭包，提交点置脏/检查单点。
    let updated = write_entry(
        "PUT /api/v1/merchants/{id}",
        conn,
        emitter.as_deref(),
        WriteOp::UpdateMerchant,
        move |conn| {
            crate::merchants::update_merchant(conn, &id, input)?;
            let updated = crate::merchants::get_merchant(conn, &id)?;
            Ok(Outcome::Silent(updated))
        },
    )
    .await?;
    Ok(Json(updated))
}
