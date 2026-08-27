//! 行情同步编排（issue #89）：市场/分页遍历、进度事件推送、新增/更新汇总。
//! 不直接触碰 HTTP 细节与 SQL，分别委托 `http` / `persist` 子模块。

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::error::Result;
use crate::models::SyncProgress;

use super::http::{MARKETS, MarketConfig, Pacer, build_client, fetch_page, get_total};
use super::persist::{apply_stock_item, build_existing_instruments};

/// 全量同步主流程：依次拉取各市场分页行情并落库，实时推送进度事件。
/// 返回 `(新增数, 更新数)`。
pub(super) fn do_sync(conn: &Connection, app: &AppHandle) -> Result<(usize, usize)> {
    let mut total_inserted = 0usize;
    let mut total_updated = 0usize;

    let client = build_client()?;
    let mut pacer = Pacer::default();

    let mut existing_map = build_existing_instruments(conn)?;

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
    let mut cumulative_processed = 0usize;

    for (total, market) in market_totals {
        let pages = total.div_ceil(super::http::PAGE_SIZE);

        for page in 1..=pages {
            let items = fetch_page(&client, &mut pacer, market, page)?;
            for item in &items {
                let (inserted, updated) =
                    apply_stock_item(conn, item, market.code, market.currency, &mut existing_map)?;
                total_inserted += inserted;
                total_updated += updated;
            }
            cumulative_processed += items.len();

            let _ = app.emit(
                "sync-instruments:progress",
                SyncProgress {
                    current: cumulative_processed,
                    total: grand_total,
                    market: market.code.to_string(),
                    done: false,
                    total_inserted,
                    total_updated,
                    error: None,
                },
            );
        }
    }

    let _ = app.emit(
        "sync-instruments:progress",
        SyncProgress {
            current: 0,
            total: 0,
            market: String::new(),
            done: true,
            total_inserted,
            total_updated,
            error: None,
        },
    );

    Ok((total_inserted, total_updated))
}
