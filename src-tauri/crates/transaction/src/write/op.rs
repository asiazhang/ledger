//! op 产出接缝（写路径集中单点，issue #855 / ADR-0091）。
//!
//! 职责：本地交易写成功后把 [`TransactionCommand`] 追加进本机 OpLog。不变量：
//! 仅写入协议三入口（create / update / delete）调用一次，随编排事务提交/回滚，
//! 写失败不残留 op。ADR 指针：ADR-0091（op 记录）/ ADR-0105（写入协议）。陷阱：
//! 载荷契约住共享语义区 `crate::command`，本模块只消费、不定义。

use rusqlite::Connection;

use ledger_infra::error::Result;
use ledger_sync_protocol::op::record_local as record_op;

use crate::command::TransactionCommand;

/// op 产出接缝（交易域集中单点）：本地交易写成功后追加一条 op 进本机 OpLog。
///
/// 仅写入协议（`crate::write::protocol` 的 create / update / delete）调用；随
/// 编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: TransactionCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}
