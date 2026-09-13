//! 备注拼音回填报告模型（共享语义区，issue #513）。
//!
//! 职责：一键修复的失败阶段/原因/报告类型。不变量：失败阶段文案经前端资源本地化，
//! 底层消息原样透传不造码。ADR 指针：ADR-0113 决策 8。陷阱：失败时收敛位如实报告剩余积压。

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotePinyinRepairStage {
    /// 积压探测失败。
    Probe,
    /// 读取积压行失败。
    Read,
    /// 开启批事务失败。
    Begin,
    /// 写入批内行失败。
    Write,
    /// 提交批事务失败。
    Commit,
}

/// 备注拼音回填失败原因（issue #513）：失败阶段 + 底层错误消息（诊断用，
/// 阶段文案经前端文案资源本地化，消息原样透传不造码）。
#[derive(Debug, Serialize, ToSchema)]
pub struct NotePinyinRepairFailure {
    /// 失败阶段。
    pub stage: NotePinyinRepairStage,
    /// 底层错误消息。
    pub message: String,
}

/// 备注拼音一键修复报告（issue #513）：回填行数 / 是否收敛 / 失败原因。
#[derive(Debug, Serialize, ToSchema)]
pub struct NotePinyinRepairReport {
    /// 本次实际补齐的 NULL 积压行数（幂等：重复执行为 0）。
    pub backfilled: u64,
    /// 结束后积压是否清零（「有备注且拼音列 NULL」的行不存在；无备注行的
    /// NULL 列不构成积压）。
    pub converged: bool,
    /// 失败原因（None = 全程无失败）；失败时收敛位仍如实报告剩余积压。
    pub failure: Option<NotePinyinRepairFailure>,
}
