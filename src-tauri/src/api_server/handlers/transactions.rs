//! 交易端点：列表（过滤 + 分页）/ 批量创建（默认去重）/ 全字段替换 / 软删除。
//!
//! 写端点经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 事务、置脏、信号内化单点，「即建商户」证据随闭包返回必达；读端点经
//! `run_db`（形状乙）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rusqlite::Connection;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::EmitterSlot;
use crate::db::run_db;
use crate::error::AppError;
use crate::signals::WriteOp;
use crate::transaction::amount::TransactionKind;
use crate::transaction::{
    CreateTransactionResult, Transaction, TransactionBatchInput, TransactionListFilter,
    TransactionListResult, UpdateTransactionInput,
};
use crate::write_entry::{Outcome, write_entry};

#[utoipa::path(
    get,
    path = "/api/v1/transactions",
    tag = "transactions",
    summary = "列出交易（可按日期/账户/类型过滤 + 服务端分页）",
    description = "读回/列表唯一入口：返回 `{items, total}`，过滤参数（from/to、account_id、\
                  involving_account_id、merchant_id、category_id、kind/kinds、\
                  uncategorized_only、limit、page/page_size）全部可选；默认按日期倒序稳定排序。\
                  读回核对与参数语义见导入知识「对账完成判定」节；行携带 `source` 来源字段（读时反查推导），\
                  无来源交易为`null`。",
    params(
        ("from" = Option<String>, Query, description = "起始日期（含），YYYY-MM-DD"),
        ("to" = Option<String>, Query, description = "结束日期（含），YYYY-MM-DD"),
        ("account_id" = Option<String>, Query, description = "按转出账户过滤"),
        ("involving_account_id" = Option<String>, Query, description = "涉及账户过滤（account_id 或 to_account_id 命中即算，含转入的转账）"),
        ("merchant_id" = Option<String>, Query, description = "按商户过滤（含软删商户的历史交易）"),
        ("category_id" = Option<String>, Query, description = "按分类精确过滤（不含子分类，含软删分类的历史交易）"),
        ("uncategorized_only" = Option<bool>, Query, description = "true 时仅返回无分类交易；与 category_id 同携按 AND 组合"),
        ("kind" = Option<TransactionKind>, Query, description = "income / expense / transfer / buy / sell / refund（闭集枚举，非法值 4xx）"),
        ("kinds" = Option<Vec<TransactionKind>>, Query, description = "类型集合过滤（issue #581）：逗号分隔单参数如 expense,refund，命中 kind IN (...)；与其余维度 AND 组合，非法值 4xx"),
        ("limit" = Option<i64>, Query, description = "取前 N 条，缺省返回全部；传 page_size 时分页路径生效"),
        ("page" = Option<usize>, Query, description = "页码，从 1 开始，默认 1"),
        ("page_size" = Option<usize>, Query, description = "每页条数，缺省返回全部（total 恒返回）")
    ),
    responses(
        (status = 200, description = "交易分页结果 {items, total}", body = TransactionListResult),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn list_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Query(query): Query<TransactionListFilter>,
) -> Result<Json<TransactionListResult>, AppError> {
    run_db("GET /api/v1/transactions", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = crate::transaction::list_transactions_internal(&conn, &query)?;
        Ok(Json(result))
    })
    .await
}

#[utoipa::path(
    post,
    path = "/api/v1/transactions/batch",
    tag = "transactions",
    summary = "批量创建交易（默认去重）",
    description = "批量创建交易（`dedup` 默认 true）：幂等键优先、内容哈希兜底去重；\
                  单行校验失败返回 `success: false` 附中文 `error`、不影响其余行；非法 `kind` \
                  属请求体格式错误、整批 4xx。去重与幂等键规则见导入知识「幂等与去重」节。",
    request_body = TransactionBatchInput,
    responses(
        (status = 200, description = "逐条创建结果（含 duplicate 标记）", body = [CreateTransactionResult]),
        (status = 400, description = "请求体格式错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn batch_create_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Json(body): Json<TransactionBatchInput>,
) -> Result<Json<Vec<CreateTransactionResult>>, AppError> {
    // 批次事务由 run 自持（issue #245），提交点置脏/到期检查单点；整批回滚不置脏
    // 由写入口闭包失败语义保证。批内任一行即建商户才发参考失效信号
    //（修复 HTTP 导入即建商户后前端商户字典陈旧，issue #331）。
    let results = write_entry(
        "POST /api/v1/transactions/batch",
        conn,
        emitter.as_deref(),
        WriteOp::BatchCreateTransactions,
        move |conn| {
            crate::transaction::TransactionBatch::run(conn, body.transactions, body.dedup)
                .map(|outcome| Outcome::Evidenced(outcome.results, outcome.evidence))
        },
    )
    .await?;
    Ok(Json(results))
}

#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}",
    tag = "transactions",
    summary = "按 id 全字段替换交易（编辑）",
    description = "按 id 全字段替换交易（幂等键不可编辑，校验与创建一致）；\
                  已有部分卖出的买入拒绝修改；不存在的 id 返回 404。纠错纪律见导入知识「对账纠错」节。",
    request_body = UpdateTransactionInput,
    params(
        ("id" = String, Path, description = "交易 ID")
    ),
    responses(
        (status = 200, description = "更新后的完整交易", body = Transaction),
        (status = 400, description = "参数错误（如转账缺目标账户、部分卖出的买入、买卖标的不存在）", body = ErrorResponse),
        (status = 404, description = "交易不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn update_transaction_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTransactionInput>,
) -> Result<Json<Transaction>, AppError> {
    // 修改与读回同一写闭包，提交点置脏/检查单点；仅即建商户发参考失效信号
    //（证据随闭包返回必达，ADR-0073）。
    let updated = write_entry(
        "PUT /api/v1/transactions/{id}",
        conn,
        emitter.as_deref(),
        WriteOp::UpdateTransaction,
        move |conn| {
            let evidence =
                crate::transaction::update_transaction_internal(conn, &id, input.into())?;
            let updated = crate::transaction::get_transaction_internal(conn, &id)?;
            Ok(Outcome::Evidenced(updated, evidence))
        },
    )
    .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/transactions/{id}",
    tag = "transactions",
    summary = "删除交易（软删除）",
    description = "按 `id` 软删除交易（`is_deleted=1`）。buy 交易同步清理关联持仓\
                  （`security_lots` / `security_transactions`）；若该买入已有部分卖出则返回 400。\
                  删除后该交易不再占用去重位，重跑批量导入会重新写入（`duplicate: false`）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "交易 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 400, description = "该买入交易已有部分卖出，无法删除", body = ErrorResponse),
        (status = 404, description = "交易不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn delete_transaction_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 零信号写端点同样经统一写入口并传发射槽（ADR-0073 决策 3）：映射单点
    // 当前判定零信号、发射循环自然零次，未来补信号时天然生效。
    write_entry(
        "DELETE /api/v1/transactions/{id}",
        conn,
        emitter.as_deref(),
        WriteOp::DeleteTransaction,
        move |conn| crate::transaction::delete_transaction_internal(conn, &id).map(Outcome::Silent),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
