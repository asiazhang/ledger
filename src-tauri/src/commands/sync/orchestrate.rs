//! 行情同步编排（issue #89）：市场/分页遍历、进度事件推送、新增/更新汇总。
//! 不直接触碰 HTTP 细节与 SQL，分别委托 `http` / `persist` 子模块。
//!
//! 中断机制（issue #104）：分页循环每页检查共享取消标志（`AtomicBool`），命中即提前返回
//! [`SyncOutcome::Cancelled`]；已落库数据保留（upsert 幂等），下次重跑自动续上。进度事件
//! 终态以 [`SyncProgress::cancelled`] 区分完成/中断。核心分页循环 [`run_sync_pages`] 与
//! 网络解耦（注入 fetch / emit 闭包），测试以 mock 驱动、不依赖真实网络。

use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::error::Result;
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

/// 全量同步主流程：依次拉取各市场分页行情并落库，实时推送进度事件。
/// `cancel` 为跨命令共享的取消标志（issue #104），每页检查；命中即返回 [`SyncOutcome::Cancelled`]。
pub(super) fn do_sync(
    conn: &Connection,
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
pub(super) fn run_sync_pages<F, E>(
    conn: &Connection,
    cancel: &AtomicBool,
    market_totals: &[(usize, &MarketConfig)],
    grand_total: usize,
    fetch_page: &mut F,
    emit: &mut E,
) -> Result<SyncOutcome>
where
    F: FnMut(&MarketConfig, usize) -> Result<Vec<StockItem>>,
    E: FnMut(SyncProgress),
{
    let mut existing_map = build_existing_instruments(conn)?;
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

            let items = fetch_page(market, page)?;
            for item in &items {
                let (inserted, updated) =
                    apply_stock_item(conn, item, market.code, market.currency, &mut existing_map)?;
                total_inserted += inserted;
                total_updated += updated;
            }
            cumulative_processed += items.len();

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
