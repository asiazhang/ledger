//! HTTP 壳「端点 → 写操作身份」声明表与单点映射发射（ADR-0044 决策 3 / #335）。

use crate::signals::{WriteEvidence, WriteOp, emit_for};

use super::state::EmitterSlot;

/// HTTP 壳「端点 → 写操作身份」声明表（ADR-0044 决策 3 / #335）：本壳数据面
/// （OpenAPI 契约自描述枚举的 method + path 端点集，与 `#[utoipa::path]` 注解同源）
/// 的逐一身份声明——写端点声明其 [`WriteOp`]（与 handler 内 `emit_after_write` 的
/// 判定键同源），读端点显式 [`None`]（刻意无写身份，是决策而非遗漏）。
///
/// 交叉核对测试（`signals_cross_check`）以 [`super::openapi::ApiDoc::openapi`] 枚举的
/// 端点集与本表双向比对：「新写端点忘了声明身份」「表键漂移」「契约漏记端点」
/// 测试期即红；`Some` 身份再经 `signals_for` 的编译期穷尽 match 保证映射不缺行。
/// `GET /api/v1/openapi.json` 是契约自举端点（文档自身不入契约），不持数据身份、不入表。
///
/// **新增 / 删除 / 改动端点必须同步本表**——漏声明不是「不发信号」的合法形态。
pub const HTTP_ENDPOINT_WRITE_OPS: &[(&str, Option<WriteOp>)] = &[
    ("POST /api/v1/accounts", Some(WriteOp::CreateAccount)),
    ("PUT /api/v1/accounts/{id}", Some(WriteOp::UpdateAccount)),
    ("DELETE /api/v1/accounts/{id}", Some(WriteOp::DeleteAccount)),
    ("GET /api/v1/accounts", None),
    ("GET /api/v1/accounts/balances", None),
    ("GET /api/v1/categories", None),
    ("POST /api/v1/categories", Some(WriteOp::CreateCategory)),
    (
        "DELETE /api/v1/categories/{id}",
        Some(WriteOp::DeleteCategory),
    ),
    ("GET /api/v1/currencies", None),
    ("GET /api/v1/transactions", None),
    (
        "POST /api/v1/transactions/batch",
        Some(WriteOp::BatchCreateTransactions),
    ),
    (
        "PUT /api/v1/transactions/{id}",
        Some(WriteOp::UpdateTransaction),
    ),
    (
        "DELETE /api/v1/transactions/{id}",
        Some(WriteOp::DeleteTransaction),
    ),
    ("GET /api/v1/instruments", None),
    ("POST /api/v1/instruments", Some(WriteOp::CreateInstrument)),
    ("GET /api/v1/funds/{code}", None),
    ("GET /api/v1/merchants", None),
    ("GET /api/v1/import/knowledge", None),
];

/// HTTP 壳的单点映射发射（ADR-0044）：写事务**提交成功后**按写操作身份 + 结果证据经
/// `signals` 映射单点判定「发不发、发哪个」——壳层只转发，不持有判定知识，也不再出现
/// 按端点手写的 emit 样板。发射槽为 `None`（集成测试不经真实 Tauri 运行时，见
/// [`super::state::ApiState`]）跳过发射分支，语义不变；发射失败静默忽略，不影响写结果。
pub fn emit_after_write(emitter: &EmitterSlot, op: WriteOp, evidence: WriteEvidence) {
    if let Some(emitter) = emitter {
        emit_for(emitter.as_ref(), op, evidence);
    }
}
