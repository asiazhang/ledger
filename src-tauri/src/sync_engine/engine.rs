//! 同步引擎公开接口：跨端全序、幂等重放与日志读取。
//!
//! 本域行为的唯一断言权威层（域单测）：全序确定性、重放幂等、A 端写 → B 端
//! 重放后账本状态一致的闭环判据全部对准本模块的公开函数。
//!
//! 重放协议（ADR-0091 决策 2/3）：外来 op 按 (逻辑时钟, DeviceId) 全序逐条
//! 执行——已知 op（按 `op_id`）直接跳过（重复投递不产生第二次效果）；未知 op
//! 经既有写入接缝执行命令（`transaction::replay_command`），与 op 落日志同处
//! 一个事务（op 为同步原子单位），折算结果随命令携带，不依赖本地汇率表。
//!
//! 失败语义：单条 op 执行失败即整体上抛（此前已应用的 op 保持已应用，失败的
//! op **不落日志**——重投递会重试，不静默丢弃，零丢失）；「不可重放 op 进挂起
//! 队列、不阻塞其余重放」由 #856 在本接缝上承接。

use crate::error::Result;
use crate::transaction::{ensure_transaction, replay_command};

use super::command::DomainCommand;
use super::model::SyncOp;
use super::ops;

/// 单条 op 的重放结果：执行（含 op 落日志）或按幂等跳过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpOutcome {
    /// 未知 op：命令已执行、op 已落日志。
    Applied,
    /// 已知 op：按 `op_id` 幂等跳过，无第二次效果。
    Skipped,
}

/// 单条 op 的重放报告（报告顺序与全序一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub op_id: String,
    pub outcome: OpOutcome,
}

/// 跨端全序（ADR-0091 决策 4）：按 (逻辑时钟, DeviceId) 升序原地排序。
///
/// 各端对同一批 op 排出唯一一致的顺序；同钟时以 DeviceId 字典序 tiebreak，
/// 排序键全量携带于 op 信封，判定不依赖任何本机状态。
pub fn total_order(ops: &mut [SyncOp]) {
    ops.sort_by(|a, b| (a.clock, a.device_id.as_str()).cmp(&(b.clock, b.device_id.as_str())));
}

/// 幂等重放：外来 op 批量应用到本机账本。
///
/// 按全序逐条处理；每条 op 原子（命令执行 + op 落日志同事务，op 即同步原子
/// 单位）；重复投递整批或部分重复均安全（已知 op 跳过）。返回按全序排列的
/// 逐条报告。
pub fn apply_ops(conn: &rusqlite::Connection, incoming: &[SyncOp]) -> Result<Vec<ApplyReport>> {
    let mut ordered = incoming.to_vec();
    total_order(&mut ordered);
    let mut reports = Vec::with_capacity(ordered.len());
    for op in &ordered {
        if ops::is_known(conn, &op.op_id)? {
            reports.push(ApplyReport {
                op_id: op.op_id.clone(),
                outcome: OpOutcome::Skipped,
            });
            continue;
        }
        let command: &DomainCommand = &op.command;
        ensure_transaction(conn, || {
            dispatch(conn, command)?;
            ops::insert_row(conn, op)
        })?;
        reports.push(ApplyReport {
            op_id: op.op_id.clone(),
            outcome: OpOutcome::Applied,
        });
    }
    Ok(reports)
}

/// 命令分派：按实体转发到各域的重放执行接缝（新实体随 DomainCommand 追加）。
fn dispatch(conn: &rusqlite::Connection, command: &DomainCommand) -> Result<()> {
    match command {
        DomainCommand::Transaction(cmd) => replay_command(conn, cmd),
    }
}

/// 读取本机全部 op（本地产出 + 已重放的外来 op），按全序返回。
///
/// 双端场景的消费形态：A 端 `read_ops` → B 端 `apply_ops`（Transport 引入前
/// 的测试与工具通道，#859 接线）。
pub fn read_ops(conn: &rusqlite::Connection) -> Result<Vec<SyncOp>> {
    let mut list = ops::read_all(conn)?;
    total_order(&mut list);
    Ok(list)
}
