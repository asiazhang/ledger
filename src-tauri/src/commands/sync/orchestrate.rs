//! 行情同步编排（issue #89）：市场/分页遍历、进度事件推送、新增/更新汇总。
//! 不直接触碰 HTTP 细节与 SQL，分别委托 `http` / `persist` 子模块。
//!
//! 中断机制（issue #104）：分页循环每页检查共享取消标志（`AtomicBool`），命中即提前返回
//! [`SyncOutcome::Cancelled`]；已落库数据保留（upsert 幂等），下次重跑自动续上。进度事件
//! 终态以 [`SyncProgress::cancelled`] 区分完成/中断。核心分页循环 [`run_sync_pages`] 与
//! 网络解耦（注入 fetch / emit 闭包），测试以 mock 驱动、不依赖真实网络。
//!
//! 锁粒度收窄（issue #147）：分页循环的落库经注入的连接访问器 [`ConnAccessor`] 进行，
//! 每页「锁外拉取 → 短暂持锁落库 → 释放 → 锁外推进度」，网络等待期间不再独占全局连接；
//! 既有标的映射随页重建，不跨页持有。生产实现为 [`GlobalConn`]（包装全局连接句柄）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};
use crate::models::SyncProgress;

use super::http::{MARKETS, MarketConfig, Pacer, StockItem, build_client, fetch_page, get_total};
use super::persist::{apply_stock_item, build_existing_instruments};

/// 全量同步的一次执行结果：区分完成 / 被中断两种终态（issue #104）。
/// 两种姿态都携带最终新增/更新计数，供进度事件与调用方（取消路径）取用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SyncOutcome {
    /// 正常完成：遍历了所有市场所有分页。
    Completed { inserted: usize, updated: usize },
    /// 分页循环中发现取消标志被置位，提前返回；已落库数据保留。
    Cancelled { inserted: usize, updated: usize },
}

/// 连接访问器接缝（issue #147）：分页循环的落库操作经它短暂获取/释放连接，
/// 网络拉取与进度推送不持有连接。生产实现为 [`GlobalConn`]；测试注入 mock
/// 以观察锁时序（拉取期间锁可用、每页落库才加锁）。
pub(super) trait ConnAccessor {
    /// 持锁执行一次落库操作，闭包返回后立即释放。
    fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R>;
}

/// 生产连接访问器：包装全局连接句柄（`DbState.conn`），每次落库短暂加锁/释放，
/// 单条后台同步任务不再独占连接（issue #147）。与其它命令的互斥仍由
/// SQLite 自身写锁 + 该互斥锁保证。
pub(super) struct GlobalConn(pub(super) Arc<Mutex<Connection>>);

impl ConnAccessor for GlobalConn {
    fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let guard = self
            .0
            .lock()
            .map_err(|e| AppError::Db(format!("数据库锁定失败: {e}")))?;
        f(&guard)
    }
}

/// 全量同步主流程：依次拉取各市场分页行情并落库，实时推送进度事件。
/// `cancel` 为跨命令共享的取消标志（issue #104），每页检查；命中即返回 [`SyncOutcome::Cancelled`]。
/// 连接经访问器 `conn` 注入（issue #147），本函数自身不持锁等待网络。
pub(super) fn do_sync<A: ConnAccessor>(
    conn: &A,
    app: &AppHandle,
    cancel: &AtomicBool,
) -> Result<SyncOutcome> {
    let client = build_client()?;
    let mut pacer = Pacer::default();

    let market_totals: Vec<(usize, &MarketConfig)> = MARKETS
        .iter()
        .map(|m| {
            get_total(&client, &mut pacer, m)
                .map(|t| (t, m))
                .map_err(|e| {
                    super::emit_error_progress(app, format!("获取{}总数失败: {e}", m.name));
                    e
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let grand_total: usize = market_totals.iter().map(|(t, _)| *t).sum();

    let mut fetch_page =
        |market: &MarketConfig, page: usize| fetch_page(&client, &mut pacer, market, page);
    let mut emit = |p: SyncProgress| {
        let _ = app.emit("sync-instruments:progress", p);
    };

    let outcome = run_sync_pages(
        conn,
        cancel,
        &market_totals,
        grand_total,
        &mut fetch_page,
        &mut emit,
    )?;
    let _ = app.emit("sync-instruments:progress", terminal_progress(&outcome));
    Ok(outcome)
}

/// 核心分页循环：每页开头检查取消标志，命中即提前返回（该页不再拉取，已落库数据保留）。
/// `fetch_page` / `emit` 由调用方注入（生产接 HTTP + AppHandle，测试注入 mock），本函数不触碰网络。
/// 连接经访问器注入（issue #147）：每页锁外拉取 → 短暂持锁落库 → 释放 → 锁外推进度，
/// 既有标的映射随页重建，不跨页持有。
pub(super) fn run_sync_pages<A, F, E>(
    conn: &A,
    cancel: &AtomicBool,
    market_totals: &[(usize, &MarketConfig)],
    grand_total: usize,
    fetch_page: &mut F,
    emit: &mut E,
) -> Result<SyncOutcome>
where
    A: ConnAccessor,
    F: FnMut(&MarketConfig, usize) -> Result<Vec<StockItem>>,
    E: FnMut(SyncProgress),
{
    let mut total_inserted = 0usize;
    let mut total_updated = 0usize;
    let mut cumulative_processed = 0usize;

    for (total, market) in market_totals.iter().copied() {
        let pages = total.div_ceil(super::http::PAGE_SIZE);

        for page in 1..=pages {
            if cancel.load(Ordering::SeqCst) {
                // 命中取消：不再拉取本页及其后，已落库数据保留（upsert 幂等），提前返回。
                return Ok(SyncOutcome::Cancelled {
                    inserted: total_inserted,
                    updated: total_updated,
                });
            }

            // 锁外拉取一页：网络等待不持有连接。
            let items = fetch_page(market, page)?;

            // 短暂持锁：重建既有标的映射并批量落库本页，闭包返回即释放。
            let (inserted, updated) = conn.with_conn(|c| persist_page(c, &items, market))?;
            total_inserted += inserted;
            total_updated += updated;
            cumulative_processed += items.len();

            // 锁外推送进度事件。
            emit(SyncProgress {
                current: cumulative_processed,
                total: grand_total,
                market: market.code.to_string(),
                done: false,
                total_inserted,
                total_updated,
                error: None,
                cancelled: false,
            });
        }
    }

    Ok(SyncOutcome::Completed {
        inserted: total_inserted,
        updated: total_updated,
    })
}

/// 单页落库：重建既有标的映射（每次一条 SELECT，代价可接受、不跨页持有）后逐条 upsert。
/// 返回 `(新增数, 更新数)`。
fn persist_page(
    conn: &Connection,
    items: &[StockItem],
    market: &MarketConfig,
) -> Result<(usize, usize)> {
    let mut existing_map = build_existing_instruments(conn)?;
    let mut inserted = 0usize;
    let mut updated = 0usize;
    for item in items {
        let (i, u) = apply_stock_item(conn, item, market.code, market.currency, &mut existing_map)?;
        inserted += i;
        updated += u;
    }
    Ok((inserted, updated))
}

/// 构造同步终态事件（`done=true`）：以 [`SyncOutcome`] 区分完成/中断，`cancelled` 字段标记。
/// 独立成函数以便测试直接验证终态的 `cancelled` 取值（完成=中断再区分），无需真实 AppHandle。
pub(super) fn terminal_progress(outcome: &SyncOutcome) -> SyncProgress {
    let (inserted, updated) = match outcome {
        SyncOutcome::Completed { inserted, updated }
        | SyncOutcome::Cancelled { inserted, updated } => (*inserted, *updated),
    };
    let cancelled = matches!(outcome, SyncOutcome::Cancelled { .. });
    SyncProgress {
        current: 0,
        total: 0,
        market: String::new(),
        done: true,
        total_inserted: inserted,
        total_updated: updated,
        error: None,
        cancelled,
    }
}
