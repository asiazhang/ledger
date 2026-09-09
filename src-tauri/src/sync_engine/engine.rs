//! 同步引擎公开接口：跨端全序、幂等重放、wire 接入与挂起队列。
//!
//! 本域行为的唯一断言权威层（域单测）：全序确定性、重放幂等、LWW 合并、
//! OccurrenceKey 防双扣、ParkedOp 挂起与「A 端写 → B 端重放后账本状态一致」
//! 闭环判据全部对准本模块的公开函数。
//!
//! 重放协议（ADR-0091 决策 2/3/4/5/6）：外来 op 按 (逻辑时钟, DeviceId) 全序逐
//! 条处理，单条四种归宿——
//! - **Skipped**：已知 op（按 `op_id`），重复投递不产生第二次效果；
//! - **Superseded**：LWW 输者——本地日志已有同实体且全序更后的 op（序末者已
//!   生效），输者落日志可追溯、不执行（零丢失：op 本身永不丢弃）；
//! - **Parked**：不可重放（外键依赖失败、schema 版本偏斜、载荷不可解）——进
//!   挂起队列并报告码化原因，不落日志、不阻塞其余重放、不静默丢弃、不自动
//!   复活数据；重投递（升级后/依赖方补齐后）自然重试，成功即出队；
//! - **Applied**：命令经既有写入接缝执行（`transaction::replay_command` 等），
//!   与 op 落日志同处一个事务（op 为同步原子单位），折算结果随命令携带。
//!
//! 期次触发命令（OccurrenceKey，ADR-0091 决策 5）的幂等由命令自带的确定性
//! 落地身份承载：同键（plan_id + 期次标识）跨端派生同一落地 id，已落即命中
//! 去重（报告 [`OpOutcome::Deduped`]），同一期只落一次。
//!
//! 接入面二分：[`apply_ops`] 收已解析的 op（进程内工具与测试）；[`ingest_ops`]
//! 收 wire 形态（JSON 字符串，通道上的搬运形态）——解析失败即 schema 偏斜，
//! 按信封可读性挂起，#859 Transport 直接消费。

use crate::error::{AppError, Result};
use crate::transaction::{ensure_transaction, replay_command};

use super::command::DomainCommand;
use super::model::SyncOp;
use super::ops;
use super::parked::{self, ParkedOp};
use super::positions;

/// 单条 op 的重放结果：执行（含 op 落日志）、按幂等跳过、LWW 压制、期次去重
/// 或挂起。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpOutcome {
    /// 未知 op：命令已执行、op 已落日志。
    Applied,
    /// 已知 op（按 `op_id`）或位点已覆盖（流位点 ≥ 该 op 时钟：已并入快照谱系
    /// 或日志已截断，issue #857）：无第二次效果。
    Skipped,
    /// LWW 输者（ADR-0091 决策 4）：本地日志已有同实体且全序更后的 op，本 op
    /// 落日志可追溯、不执行。
    Superseded,
    /// OccurrenceKey 命中（ADR-0091 决策 5）：该期次已在本端落地（确定性落地
    /// id 已存在），op 落日志、不产生第二次效果。
    Deduped,
    /// 不可重放（ADR-0091 决策 6）：进挂起队列，码化原因随行。
    Parked { code: String, message: String },
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
    ops.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
}

/// 全序排序键单点：(端内逻辑时钟, DeviceId) 字典序（ADR-0091 决策 4）。
fn order_key(op: &SyncOp) -> (i64, &str) {
    (op.clock, op.device_id.as_str())
}

/// 位点之后的增量（issue #857）：从候选 op 中滤出本端位点未覆盖的部分。
///
/// 「仅重放位点之后的 op」的拉取侧接缝：位点之前的 op 已并入快照谱系或日志，
/// 无需重投；无位点行的流（前检查点世界）全量保留，由重放幂等去重兑底。
pub fn ops_after_positions(
    conn: &rusqlite::Connection,
    incoming: &[SyncOp],
) -> Result<Vec<SyncOp>> {
    let mut pending = Vec::with_capacity(incoming.len());
    for op in incoming {
        match positions::position_of(conn, &op.device_id)? {
            Some(position) if op.clock <= position => continue,
            _ => pending.push(op.clone()),
        }
    }
    Ok(pending)
}

/// 位点清单（按 DeviceId 序）：Checkpoint 位点组件与通道 manifest 上报位点
/// 的数据面（issue #857）。
pub fn stream_positions(conn: &rusqlite::Connection) -> Result<Vec<positions::StreamPosition>> {
    positions::list(conn)
}

/// 幂等重放：外来 op 批量应用到本机账本。
///
/// 按全序逐条处理；每条 op 原子（命令执行 + op 落日志同事务，op 即同步原子
/// 单位）；重复投递整批或部分重复均安全（已知 op 跳过）。返回按全序排列的
/// 逐条报告。不可重放的 op 挂起（见模块文档），不阻塞其余重放。
pub fn apply_ops(conn: &rusqlite::Connection, incoming: &[SyncOp]) -> Result<Vec<ApplyReport>> {
    let mut ordered = incoming.to_vec();
    total_order(&mut ordered);
    replay_ordered(conn, ordered)
}

/// wire 接入（#859 Transport 消费）：通道上的 op 原文逐条解析后重放。
///
/// 解析失败即 schema 偏斜（旧命令重放到新 schema / 信封损坏）：按信封可读性
/// 构造挂起身份（信封亦不可读则合成 id），进挂起队列、不中断整批。已解析的
/// op 之间按全序重放；报告按**接入顺序**返回（每条的归宿占其输入位置）。
pub fn ingest_ops(conn: &rusqlite::Connection, raw_ops: &[String]) -> Result<Vec<ApplyReport>> {
    // 解析相：逐条填入输入顺序的槽位——可解析为 op，不可解析现场挂起并填报告。
    let mut slots: Vec<Slot> = Vec::with_capacity(raw_ops.len());
    for raw in raw_ops {
        let slot = match serde_json::from_str::<SyncOp>(raw) {
            Ok(op) => Slot::Parsed(Box::new(op)),
            Err(parse_error) => {
                let parked = undecodable_park_draft(raw, &parse_error.to_string());
                park(conn, &parked)?;
                Slot::Parked(ApplyReport {
                    op_id: parked.op_id.clone(),
                    outcome: OpOutcome::Parked {
                        code: parked.code.clone(),
                        message: parked.message.clone(),
                    },
                })
            }
        };
        slots.push(slot);
    }
    // 重放相：可解析 op 带着输入下标参与全序排序，重放后报告按下标归位。
    let mut indexed: Vec<(usize, SyncOp)> = slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.parsed().cloned().map(|op| (i, op)))
        .collect();
    indexed.sort_by(|a, b| order_key(&a.1).cmp(&order_key(&b.1)));
    let ordered: Vec<SyncOp> = indexed.iter().map(|(_, op)| op.clone()).collect();
    let replayed = replay_ordered(conn, ordered)?;
    let mut placed: Vec<Option<ApplyReport>> = slots
        .iter()
        .map(|s| match s {
            Slot::Parked(report) => Some(report.clone()),
            Slot::Parsed(_) => None,
        })
        .collect();
    for ((index, _), report) in indexed.into_iter().zip(replayed) {
        placed[index] = Some(report);
    }
    // 槽位缺失属程序缺陷（解析槽位数与重放报告数失衡）：非码化 Invalid、fail
    // loud（对准 ADR-0060：不以 expect/unwrap 构造 panic）。
    placed
        .into_iter()
        .map(|slot| {
            slot.ok_or_else(|| AppError::Invalid("ingest 报告槽位缺失（程序缺陷）".to_string()))
        })
        .collect()
}

/// 接入槽位：解析成功待重放的 op，或解析失败已挂起的报告。
enum Slot {
    Parsed(Box<SyncOp>),
    Parked(ApplyReport),
}

impl Slot {
    fn parsed(&self) -> Option<&SyncOp> {
        match self {
            Slot::Parsed(op) => Some(op),
            Slot::Parked(_) => None,
        }
    }
}

/// 挂起队列清单（挂起通知与用户裁决界面的数据面），按全序返回。
pub fn parked_ops(conn: &rusqlite::Connection) -> Result<Vec<ParkedOp>> {
    parked::list(conn)
}

/// 全序重放本体：逐条 [`replay_one`]，报告与输入同序（已按全序排列）。
fn replay_ordered(conn: &rusqlite::Connection, ordered: Vec<SyncOp>) -> Result<Vec<ApplyReport>> {
    // 本端 schema 版本批内单次读取（schema 偏斜判定基准）。
    let local_version = crate::db::schema_version(conn)?;
    let mut reports = Vec::with_capacity(ordered.len());
    for op in &ordered {
        reports.push(replay_one(conn, op, local_version)?);
    }
    Ok(reports)
}

/// 单条 op 的重放归宿（模块文档「重放协议」的逐条实现）。
fn replay_one(conn: &rusqlite::Connection, op: &SyncOp, local_version: i64) -> Result<ApplyReport> {
    let report = |outcome| ApplyReport {
        op_id: op.op_id.clone(),
        outcome,
    };
    // 已知 op：幂等跳过（含此前已应用的、已压制的输者与已出队的挂起者）。
    if ops::is_known(conn, &op.op_id)? {
        advance_position(conn, op)?;
        return Ok(report(OpOutcome::Skipped));
    }
    // 位点门（issue #857）：流位点已越过该 op（已并入快照谱系或日志已截断）
    // ⇒ 无需重放、无第二次效果。无位点行的流（前检查点世界）不设门，重放
    // 幂等去重兑底。
    if let Some(position) = positions::position_of(conn, &op.device_id)?
        && op.clock <= position
    {
        return Ok(report(OpOutcome::Skipped));
    }
    // schema 版本偏斜（旧端收到新命令）：op 产生自更新版本，本端不可信执行，
    // 挂起并提示升级；升级后重投递自然重试。不静默丢弃。
    if op.schema_version > local_version {
        let parked = parked_from_op(
            op,
            parked::CODE_SCHEMA_AHEAD,
            "该操作来自更新版本的应用，升级本端后将自动重试".to_string(),
        )?;
        park(conn, &parked)?;
        return Ok(report(OpOutcome::Parked {
            code: parked.code,
            message: parked.message,
        }));
    }
    // LWW（ADR-0091 决策 4）：同实体存在全序更后的 op ⇒ 序末者已生效，本 op
    // 为输者——落日志可追溯、不执行。LWW 只裁决「op 都活着但打架」；op 本身
    // 永不丢弃。
    if let Some((entity, entity_id)) = op.command.subject()
        && ops::has_later_subject(conn, entity, entity_id, op.clock, &op.device_id)?
    {
        ensure_transaction(conn, || {
            ops::insert_row(conn, op)?;
            advance_position(conn, op)
        })?;
        return Ok(report(OpOutcome::Superseded));
    }
    // 执行：命令 + op 落日志 + 位点推进同事务原子；失败不落日志、水位不动，
    // 挂起后不阻塞其余重放。
    match ensure_transaction(conn, || {
        let effect = dispatch(conn, &op.command)?;
        ops::insert_row(conn, op)?;
        advance_position(conn, op)?;
        Ok(effect)
    }) {
        Ok(effect) => {
            // 成功即出队：此前挂起的同一 op（依赖方补齐后重试等）不再滞留队列。
            parked::resolve(conn, &op.op_id)?;
            Ok(report(match effect {
                ReplayEffect::Applied => OpOutcome::Applied,
                ReplayEffect::IdempotentHit => OpOutcome::Deduped,
            }))
        }
        Err(replay_error) => {
            park_replay_failure(conn, op, &replay_error)?;
            Ok(report(OpOutcome::Parked {
                code: replay_error
                    .code()
                    .unwrap_or(parked::CODE_REPLAY_FAILED)
                    .to_string(),
                message: replay_error.to_string(),
            }))
        }
    }
}

/// 位点推进（op 裁决落定后调用；挂起不推进——位点不越过未应用 op，这是
/// 「截断不丢失未应用 op」的机制根据，issue #857）。
fn advance_position(conn: &rusqlite::Connection, op: &SyncOp) -> Result<()> {
    positions::advance(conn, &op.device_id, op.clock)
}

/// 挂起入队（补齐簿记戳后经 parked 模块落库）。
fn park(conn: &rusqlite::Connection, parked: &ParkedOp) -> Result<()> {
    let mut row = parked.clone();
    row.parked_at = crate::db::now_iso();
    parked::park(conn, &row)
}

/// 重放失败的挂起构造：码化错误原样携带其码（前端按码本地化）；其余错误以
/// 通用挂起码 + 原始详情承载（外键依赖失败经各域守卫以码化错误表达）。
fn park_replay_failure(conn: &rusqlite::Connection, op: &SyncOp, error: &AppError) -> Result<()> {
    let (code, message) = match error {
        AppError::Coded { code, message, .. } => (code.clone(), message.clone()),
        other => (parked::CODE_REPLAY_FAILED.to_string(), other.to_string()),
    };
    let parked = parked_from_op(op, &code, message)?;
    park(conn, &parked)
}

/// op → 挂起行构造单点（信封字段与载荷序列化同源；簿记戳由 [`park`] 落库时补齐）。
fn parked_from_op(op: &SyncOp, code: &str, message: String) -> Result<ParkedOp> {
    Ok(ParkedOp {
        op_id: op.op_id.clone(),
        device_id: op.device_id.clone(),
        clock: op.clock,
        schema_version: op.schema_version,
        entity: op.command.entity().to_string(),
        entity_id: op
            .command
            .subject()
            .map(|(_, id)| id.to_string())
            .unwrap_or_default(),
        payload: serde_json::to_string(&op.command)
            .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?,
        code: code.to_string(),
        message,
        parked_at: String::new(),
    })
}

/// wire 解析失败的挂起行构造：按信封可读性尽力提取身份，信封亦不可读则合成
/// id——op 永不静默丢弃；合成 id 由原文确定性派生（重投递同一坏报文不堆积）。
fn undecodable_park_draft(raw: &str, detail: &str) -> ParkedOp {
    let value: Option<serde_json::Value> = serde_json::from_str(raw).ok();
    let envelope = value.as_ref();
    let envelope_str = |key: &str| {
        envelope
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let entity = envelope
        .and_then(|v| v.get("command"))
        .and_then(|c| c.get("entity"))
        .and_then(|e| e.as_str())
        .unwrap_or_default()
        .to_string();
    ParkedOp {
        op_id: envelope_str("op_id")
            .unwrap_or_else(|| format!("parked-{}", crate::db::deterministic_uuid(raw))),
        device_id: envelope_str("device_id").unwrap_or_default(),
        clock: envelope
            .and_then(|v| v.get("clock"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        schema_version: envelope
            .and_then(|v| v.get("schema_version"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        entity,
        entity_id: String::new(),
        payload: raw.to_string(),
        code: parked::CODE_UNDECODABLE.to_string(),
        message: format!("同步命令无法识别（可能产生自更高版本的应用）：{detail}"),
        parked_at: String::new(),
    }
}

/// 命令分派：按实体转发到各域的重放执行接缝（新实体随 DomainCommand 追加）。
fn dispatch(conn: &rusqlite::Connection, command: &DomainCommand) -> Result<ReplayEffect> {
    match command {
        DomainCommand::Transaction(cmd) => {
            replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Scheduled(cmd) => crate::scheduled_transactions::replay_command(conn, cmd),
        DomainCommand::LedgerSetting(cmd) => {
            crate::currencies::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Account(cmd) => {
            crate::accounts::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Category(cmd) => {
            crate::categories::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Merchant(cmd) => {
            crate::merchants::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Budget(cmd) => {
            crate::budget::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Policy(cmd) => {
            crate::policy::replay_policy_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Insurer(cmd) => {
            crate::policy::replay_insurer_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::Item(cmd) => {
            crate::item::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
        DomainCommand::PhysicalAsset(cmd) => {
            crate::physical_asset::replay_command(conn, cmd)?;
            Ok(ReplayEffect::Applied)
        }
    }
}

/// 命令执行效果：落地新效果，或幂等命中（该效果已在本端存在）。
///
/// 期次触发命令（OccurrenceKey）经确定性落地身份去重时返回 `IdempotentHit`；
/// 其余命令恒为 `Applied`。
pub(crate) enum ReplayEffect {
    Applied,
    IdempotentHit,
}

/// 读取本机全部 op（本地产出 + 已重放的外来 op），按全序返回。
///
/// 双端场景的消费形态：A 端 `read_ops` → B 端 `apply_ops`（Transport 引入前
/// 的测试与工具通道，#859 接线）。挂起中的 op 不在日志（未应用），经
/// [`parked_ops`] 读取。
pub fn read_ops(conn: &rusqlite::Connection) -> Result<Vec<SyncOp>> {
    let mut list = ops::read_all(conn)?;
    total_order(&mut list);
    Ok(list)
}
