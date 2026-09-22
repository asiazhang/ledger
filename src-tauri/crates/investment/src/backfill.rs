//! 价格历史后台补全的运行态快照与走势空态三态判定（ADR-0122 决策 5 / issue #1377）。
//!
//! 走势面板的空态不再指向同步按钮，改按补全状态三态化——**补全中**（带计数）/
//! **补全失败待重试** / **无数据**。三态的判定消费两类事实：
//!
//! - **派生事实**（持久，本域自查）：标的有价格写入通道（行情 / 净值）且磁盘上
//!   没有任何历史序列——这正是价格历史后台补全队列的收集判据（ADR-0122 决策 7
//!   「续做靠派生事实」），走势空态与队列同源，不另立第二份口径；
//! - **运行态快照**（进程内，非持久，由行情同步域发布）：后台补全当前是否在跑
//!   （带 `done`/`total` 计数）与本轮已尝试仍无历史序列的标的及其结局（无可采
//!   数据 / 失败待重试）。运行态不落库——进程重启即回「补全中」，与「队列由
//!   派生事实重建」同一哲学（ADR-0122 决策 7）。
//!
//! 依赖方向（ADR-0112 决策 5 的合法直呼半边）：发布方是行情同步域（上层，生产
//! `ledger-market-sync` 直接调 [`round_started`] 等发布函数——上层依赖下层直呼，
//! 不引入端口/事件反转）；消费方是本域走势查询（[`super::trend`]），在投影为空
//! 时按本模块判定补字段。发布与消费的运行时接线随行情同步域的后台任务自洽，
//! 壳层零接线；未发布（无后台任务运行）时快照为默认值，判定退化为「补全中」
//! ——对「有通道无历史」的标的这是诚实缺省（后台任务会采它）。

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;

use crate::channel::{PriceChannel, derive_price_channel};
use crate::market::Market;
use crate::model::InstrumentType;
use ledger_infra::error::Result;

/// 后台补全一轮中单只标的的尝试结局（仅对**尝试后仍无历史序列**的标的记录；
/// 采到数据的标的的历史已落库，空态判定不再消费它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillAttempt {
    /// 尝试成功但数据源没有可采的历史序列（查无此码 / 新基金未公布首期净值 /
    /// 退市股无日 K）——「无数据」三态的判据。
    NoData,
    /// 尝试失败（网络错误）或本轮窗口不完整（部分页被拦截 / 页数触顶）——
    /// 「补全失败待重试」三态的判据；下一窗口按派生事实自然重试。
    Failed,
}

/// 后台补全的运行态快照：一轮进行中的标的级计数 + 本进程已尝试仍无历史序列的
/// 标的结局表。进程内状态，不落库（ADR-0122 决策 7）。
#[derive(Debug, Clone, Default)]
struct BackfillRunState {
    /// 进行中的一轮：`(done, total)`；`None` = 无在途轮次（队列空或轮次已收）。
    running: Option<(usize, usize)>,
    /// 本进程各轮尝试后仍无历史序列的标的 → 结局；新一轮开始时整体清空
    /// （队列按派生事实重收集，旧结局不再可信）。
    attempts: HashMap<String, BackfillAttempt>,
}

static RUN: std::sync::LazyLock<Mutex<BackfillRunState>> =
    std::sync::LazyLock::new(|| Mutex::new(BackfillRunState::default()));

/// 快照读取（毒化互斥体按默认值降级：三态退化为「补全中」，不 panic）。
/// 毒化策略全模块统一：**读侧恢复 + 告警，写侧告警 + 跳过**——运行态是
/// 可重建的进程内判据（轮次与结局都会随下一轮重置），写跳过不产生不可自愈
/// 的错态；告警保证毒化（持锁恐慌）不静默。
fn snapshot() -> BackfillRunState {
    match RUN.lock() {
        Ok(state) => state.clone(),
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，快照按默认值降级（三态暂按补全中）");
            poisoned.into_inner().clone()
        }
    }
}

/// 一轮补全开始（行情同步域的排空轮在队列收集完成后调用）：登记轮次计数分母、
/// 清空上一轮遗留的尝试结局表。`total = 0` 的空轮不调用（队列空零动作）。
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

/// 一轮补全的标的级推进（每处理完一只调用一次，成败同计格，与进度事件同口径）。
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

/// 一轮补全结束：在途计数收起；尝试结局表保留到下一轮开始——轮间窗口的空态
/// 判定消费的正是它（「无数据 / 待重试」在两轮之间依然可答）。
pub fn round_finished() {
    match RUN.lock() {
        Ok(mut state) => state.running = None,
        Err(poisoned) => {
            tracing::warn!("后台补全运行态互斥体损坏，轮次收起未登记");
            drop(poisoned.into_inner());
        }
    }
}

/// 记录一只标的的尝试结局：调用方（行情同步域）在单只补全单元返回后调用，
/// 且仅当该标的**仍无历史序列**时（有历史序列者无需记录，空态判定不再消费）。
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

/// 「补全中」三态：在途轮次带计数，轮间窗口无计数（缺省字段不序列化）。
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

/// 走势空态的补全状态（ADR-0122 决策 5 三态）：`running` = 补全中（在途轮次带
/// `done`/`total` 计数）；`retry_pending` = 补全失败待重试；`no_data` = 无数据
/// （该标的确实没有可采的历史序列）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TrendBackfillStatus {
    pub state: TrendBackfillState,
    /// 在途轮次已完成的标的数（仅 `running` 且有在途轮次时携带）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<usize>,
    /// 在途轮次的队列总长（仅 `running` 且有在途轮次时携带）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

/// 三态枚举（serde 蛇形序列化：`running` / `retry_pending` / `no_data`，与前端
/// 类型闭集一致）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrendBackfillState {
    Running,
    RetryPending,
    NoData,
}

/// 单标的走势空态的补全状态判定（读投影只增字段，issue #1377）：仅对**有价格
/// 写入通道（行情 / 净值）且磁盘上没有任何历史序列**的标的有值——有历史者与
/// 无通道者返回 `None`（投影不带该字段，前端按既有空态文案渲染）。判定消费
/// 派生事实（通道 + 历史）与运行态快照，与后台补全队列同源。
pub fn instrument_trend_backfill_status(
    conn: &Connection,
    instrument_id: &str,
) -> Result<Option<TrendBackfillStatus>> {
    let collectable = is_collectable_instrument(conn, instrument_id)?;
    if !collectable {
        return Ok(None);
    }
    if has_any_history(conn, instrument_id)? {
        return Ok(None);
    }
    let state = snapshot();
    if let Some(attempt) = state.attempts.get(instrument_id) {
        return Ok(Some(match attempt {
            BackfillAttempt::Failed => TrendBackfillStatus {
                state: TrendBackfillState::RetryPending,
                done: None,
                total: None,
            },
            BackfillAttempt::NoData => TrendBackfillStatus {
                state: TrendBackfillState::NoData,
                done: None,
                total: None,
            },
        }));
    }
    Ok(Some(running_status(&state)))
}

/// 组合走势空态的补全状态判定（读投影只增字段，issue #1377）：对全部「有价格
/// 写入通道且没有任何历史序列」的标的聚合——任一尚未尝试（或新一轮已开跑）=
/// 补全中（带在途计数）；否则任一待重试 = 待重试；全部无可采 = 无数据。没有
/// 待补全标的（历史齐全而曲线仍空：区间裁剪 / 持仓与价格周错开等）返回 `None`。
pub fn portfolio_trend_backfill_status(conn: &Connection) -> Result<Option<TrendBackfillStatus>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.symbol, i.market, i.instrument_type, i.constant_unit_price FROM instruments i \
         WHERE NOT EXISTS (SELECT 1 FROM price_history ph WHERE ph.instrument_id = i.id)",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            // DB 读边界 parse 一次（市场闭集类型，issue #1673）。
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

/// 标的是否有价格写入通道（行情 / 净值）——与后台补全队列的通道分区同源
/// ([`derive_price_channel`] 判定单点，issue #1060）。恒定价格通道不在其列：
/// 它的走势由读侧按常量合成（ADR-0126 决策 6），无空态可言。
fn is_collectable_instrument(conn: &Connection, instrument_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT symbol, market, instrument_type, constant_unit_price FROM instruments WHERE id = ?1",
        [instrument_id],
        |row| {
            let symbol: String = row.get(0)?;
            let market: Market = row.get(1)?;
            let kind: InstrumentType = row.get(2)?;
            let constant_unit_price: Option<i64> = row.get(3)?;
            Ok(matches!(
                derive_price_channel(kind, market, &symbol, constant_unit_price),
                PriceChannel::Quote | PriceChannel::FundNav
            ))
        },
    )
    .map_err(Into::into)
}

/// 磁盘上是否有任何历史序列（全局判据，与区间裁剪无关）——首刷判据
///（ADR-0038 决策 6 / issue #1059）的读半边。单一判定点（本域唯一住址）：
/// 行情同步域的回填编排、现价刷新落周点与水位读均消费本函数，不另留第二份
/// EXISTS 口径（issue #1377）。
pub fn has_any_history(conn: &Connection, instrument_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM price_history WHERE instrument_id = ?1)",
        [instrument_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}
