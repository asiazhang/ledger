use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

use ledger_sync_protocol::command::ReplayEffect;
use ledger_sync_protocol::position as positions;

use super::command::{DomainCommand, ReplayBinding};
use super::model::SyncOp;
use super::ops;
use super::parked::{self, ParkedOp};
use super::registry::{
    AccountBinding, BudgetBinding, CategoryBinding, ExchangeRateBinding, GoalBinding,
    InstrumentBinding, InsurerBinding, ItemBinding, LedgerSettingBinding, MerchantBinding,
    PhysicalAssetBinding, PolicyBinding, PriceBinding, ScheduledBinding, TransactionBinding,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpOutcome {
    Applied,
    Skipped,
    Superseded,
    Deduped,
    Parked { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub op_id: String,
    pub outcome: OpOutcome,
}

pub fn total_order(ops: &mut [SyncOp]) {
    ops.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
}

fn order_key(op: &SyncOp) -> (i64, &str) {
    (op.clock, op.device_id.as_str())
}

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

pub fn stream_positions(conn: &rusqlite::Connection) -> Result<Vec<positions::StreamPosition>> {
    positions::list(conn)
}

pub fn apply_ops(conn: &rusqlite::Connection, incoming: &[SyncOp]) -> Result<Vec<ApplyReport>> {
    let mut ordered = incoming.to_vec();
    total_order(&mut ordered);
    replay_ordered(conn, ordered)
}

pub fn ingest_ops(conn: &rusqlite::Connection, raw_ops: &[String]) -> Result<Vec<ApplyReport>> {
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
    placed
        .into_iter()
        .map(|slot| {
            slot.ok_or_else(|| AppError::Invalid("ingest 报告槽位缺失（程序缺陷）".to_string()))
        })
        .collect()
}

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

pub fn parked_ops(conn: &rusqlite::Connection) -> Result<Vec<ParkedOp>> {
    parked::list(conn)
}

fn replay_ordered(conn: &rusqlite::Connection, ordered: Vec<SyncOp>) -> Result<Vec<ApplyReport>> {
    let local_version = ledger_infra::db::schema_version(conn)?;
    let mut reports = Vec::with_capacity(ordered.len());
    for op in &ordered {
        reports.push(replay_one(conn, op, local_version)?);
    }
    Ok(reports)
}

fn replay_one(conn: &rusqlite::Connection, op: &SyncOp, local_version: i64) -> Result<ApplyReport> {
    let report = |outcome| ApplyReport {
        op_id: op.op_id.clone(),
        outcome,
    };
    if ops::is_known(conn, &op.op_id)? {
        advance_position(conn, op)?;
        return Ok(report(OpOutcome::Skipped));
    }
    if let Some(position) = positions::position_of(conn, &op.device_id)?
        && op.clock <= position
    {
        return Ok(report(OpOutcome::Skipped));
    }
    if op.schema_version > local_version {
        let parked = parked_from_op(
            op,
            parked::CODE_SCHEMA_AHEAD,
            Vec::new(),
            "该操作来自更新版本的应用，升级本端后将自动重试".to_string(),
        )?;
        park(conn, &parked)?;
        return Ok(report(OpOutcome::Parked {
            code: parked.code,
            message: parked.message,
        }));
    }
    let (entity, entity_id) = op.command.subject();
    if let Some(entity_id) = entity_id
        && ops::has_later_subject(conn, entity, entity_id.as_ref(), op.clock, &op.device_id)?
    {
        ensure_transaction(conn, || {
            ops::insert_row(conn, op)?;
            advance_position(conn, op)
        })?;
        return Ok(report(OpOutcome::Superseded));
    }
    match ensure_transaction(conn, || {
        let effect = dispatch(conn, &op.command)?;
        ops::insert_row(conn, op)?;
        advance_position(conn, op)?;
        Ok(effect)
    }) {
        Ok(effect) => {
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

fn advance_position(conn: &rusqlite::Connection, op: &SyncOp) -> Result<()> {
    positions::advance(conn, &op.device_id, op.clock)
}

fn park(conn: &rusqlite::Connection, parked: &ParkedOp) -> Result<()> {
    let mut row = parked.clone();
    row.parked_at = ledger_infra::db::now_iso();
    parked::park(conn, &row)
}

fn park_replay_failure(conn: &rusqlite::Connection, op: &SyncOp, error: &AppError) -> Result<()> {
    let (code, params, message) = match error {
        AppError::Coded {
            code,
            message,
            params,
            ..
        } => (code.clone(), params.clone(), message.clone()),
        other => (
            parked::CODE_REPLAY_FAILED.to_string(),
            vec![other.to_string()],
            other.to_string(),
        ),
    };
    let parked = parked_from_op(op, &code, params, message)?;
    park(conn, &parked)
}

fn parked_from_op(
    op: &SyncOp,
    code: &str,
    params: Vec<String>,
    message: String,
) -> Result<ParkedOp> {
    let (entity, entity_id) = op.command.subject();
    Ok(ParkedOp {
        op_id: op.op_id.clone(),
        device_id: op.device_id.clone(),
        clock: op.clock,
        schema_version: op.schema_version,
        entity: entity.to_string(),
        entity_id: entity_id.map(|id| id.into_owned()).unwrap_or_default(),
        payload: serde_json::to_string(&op.command)
            .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?,
        code: code.to_string(),
        params,
        message,
        parked_at: String::new(),
    })
}

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
            .unwrap_or_else(|| format!("parked-{}", ledger_infra::db::deterministic_uuid(raw))),
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
        params: vec![detail.to_string()],
        message: format!("同步命令无法识别（可能产生自更高版本的应用）：{detail}"),
        parked_at: String::new(),
    }
}

fn dispatch(conn: &rusqlite::Connection, command: &DomainCommand) -> Result<ReplayEffect> {
    match command {
        DomainCommand::Transaction(cmd) => TransactionBinding::replay(conn, cmd),
        DomainCommand::Scheduled(cmd) => ScheduledBinding::replay(conn, cmd),
        DomainCommand::LedgerSetting(cmd) => LedgerSettingBinding::replay(conn, cmd),
        DomainCommand::Account(cmd) => AccountBinding::replay(conn, cmd),
        DomainCommand::Category(cmd) => CategoryBinding::replay(conn, cmd),
        DomainCommand::Merchant(cmd) => MerchantBinding::replay(conn, cmd),
        DomainCommand::Budget(cmd) => BudgetBinding::replay(conn, cmd),
        DomainCommand::Policy(cmd) => PolicyBinding::replay(conn, cmd),
        DomainCommand::Insurer(cmd) => InsurerBinding::replay(conn, cmd),
        DomainCommand::Item(cmd) => ItemBinding::replay(conn, cmd),
        DomainCommand::PhysicalAsset(cmd) => PhysicalAssetBinding::replay(conn, cmd),
        DomainCommand::Goal(cmd) => GoalBinding::replay(conn, cmd),
        DomainCommand::Instrument(cmd) => InstrumentBinding::replay(conn, cmd),
        DomainCommand::ExchangeRate(cmd) => ExchangeRateBinding::replay(conn, cmd),
        DomainCommand::Price(cmd) => PriceBinding::replay(conn, cmd),
    }
}

pub fn read_ops(conn: &rusqlite::Connection) -> Result<Vec<SyncOp>> {
    let mut list = ops::read_all(conn)?;
    total_order(&mut list);
    Ok(list)
}
