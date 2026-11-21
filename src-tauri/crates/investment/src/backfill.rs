use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;

use crate::channel::{PriceChannel, derive_price_channel};
use crate::market::Market;
use crate::model::InstrumentType;
use ledger_infra::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillAttempt {
    NoData,
    Failed,
}

#[derive(Debug, Clone, Default)]
struct BackfillRunState {
    running: Option<(usize, usize)>,
    attempts: HashMap<String, BackfillAttempt>,
}

static RUN: std::sync::LazyLock<Mutex<BackfillRunState>> =
    std::sync::LazyLock::new(|| Mutex::new(BackfillRunState::default()));

fn snapshot() -> BackfillRunState {
    match RUN.lock() {
        Ok(state) => state.clone(),
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，快照按默认值降级（三态暂按补全中）");
            poisoned.into_inner().clone()
        }
    }
}

pub fn round_started(total: usize) {
    match RUN.lock() {
        Ok(mut state) => {
            state.running = Some((0, total));
            state.attempts.clear();
        }
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，轮次开始未登记（三态暂无计数）");
            drop(poisoned.into_inner());
        }
    }
}

pub fn round_progress(done: usize) {
    match RUN.lock() {
        Ok(mut state) => {
            if let Some((_, total)) = state.running {
                state.running = Some((done, total));
            }
        }
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，轮次推进未登记");
            drop(poisoned.into_inner());
        }
    }
}

pub fn round_finished() {
    match RUN.lock() {
        Ok(mut state) => state.running = None,
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，轮次收起未登记");
            drop(poisoned.into_inner());
        }
    }
}

pub fn record_attempt(instrument_id: &str, attempt: BackfillAttempt) {
    match RUN.lock() {
        Ok(mut state) => {
            state.attempts.insert(instrument_id.to_string(), attempt);
        }
        Err(poisoned) => {
            tracing::warn!(instrument = %instrument_id, "后台补全运行态互斥体损坏，尝试结局未登记");
            drop(poisoned.into_inner());
        }
    }
}

fn running_status(state: &BackfillRunState) -> TrendBackfillStatus {
    match state.running {
        Some((done, total)) => TrendBackfillStatus {
            state: TrendBackfillState::Running,
            done: Some(done),
            total: Some(total),
        },
        None => TrendBackfillStatus {
            state: TrendBackfillState::Running,
            done: None,
            total: None,
        },
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TrendBackfillStatus {
    pub state: TrendBackfillState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrendBackfillState {
    Running,
    RetryPending,
    NoData,
}

pub fn portfolio_trend_backfill_status(conn: &Connection) -> Result<Option<TrendBackfillStatus>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.symbol, i.market, i.instrument_type, i.constant_unit_price FROM instruments i \
         WHERE NOT EXISTS (SELECT 1 FROM price_history ph WHERE ph.instrument_id = i.id)",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Market>(2)?,
            row.get::<_, InstrumentType>(3)?,
            row.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut pending: Vec<String> = Vec::new();
    for row in rows {
        let (id, symbol, market, kind, constant_unit_price) = row?;
        if matches!(
            derive_price_channel(kind, market, &symbol, constant_unit_price),
            PriceChannel::Quote | PriceChannel::FundNav
        ) {
            pending.push(id);
        }
    }
    if pending.is_empty() {
        return Ok(None);
    }
    let state = snapshot();
    if pending.iter().any(|id| !state.attempts.contains_key(id)) {
        return Ok(Some(running_status(&state)));
    }
    if pending
        .iter()
        .any(|id| state.attempts.get(id) == Some(&BackfillAttempt::Failed))
    {
        return Ok(Some(TrendBackfillStatus {
            state: TrendBackfillState::RetryPending,
            done: None,
            total: None,
        }));
    }
    Ok(Some(TrendBackfillStatus {
        state: TrendBackfillState::NoData,
        done: None,
        total: None,
    }))
}

pub fn has_any_history(conn: &Connection, instrument_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM price_history WHERE instrument_id = ?1)",
        [instrument_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}
