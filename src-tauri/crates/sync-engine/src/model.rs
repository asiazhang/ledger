//! op 信封 wire 模型：op 的身份与全序字段 + 语义命令载荷。
//!
//! 这是通道上的搬运形态（serde）：本地 op 产出与外来 op 重放共用同一模型；
//! `recorded_at` 等本机簿记事实不进 wire（各端落日志时刻是本地事实，不参与
//! 状态等值判定）。

use serde::{Deserialize, Serialize};

use super::command::DomainCommand;

/// 同步操作（op，ADR-0091 决策 2）：同步的原子单位，只追加、不改写。
///
/// `op_id` 是幂等去重键——同一 op 重复投递不产生第二次效果；`device_id` +
/// `clock` 构成跨端全序（[`super::engine::total_order`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOp {
    /// op 标识（UUID v7，产出端生成；幂等去重键）。
    pub op_id: String,
    /// 来源设备标识（DeviceId）。
    pub device_id: String,
    /// 来源端内单调逻辑时钟；跨端全序 = (clock, device_id) 升序。
    pub clock: i64,
    /// 产生时 schema 版本（SQLite `user_version`）；schema 偏斜判定依据
    /// （挂起队列启用后消费，issue #856）。
    pub schema_version: i64,
    /// 语义命令载荷（DomainCommand，只增不改）。
    pub command: DomainCommand,
}
