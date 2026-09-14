//! IPC 命令壳 · 跨账本投资汇总（CrossBookInvestmentSummary，ADR-0114 / issue #1196）。
//!
//! 只做编排接线：注册表枚举 → 活动本 schema 版本 → 非活动本逐本探测与只读取数
//! → 活动本读 + 折算合并，编排语义权威在 [`crate::cross_book_summary`]（壳层编
//! 排模块，接缝归属见 ADR-0114 与其修订记录）；投资口径读函数归投资域
//! （`ledger_investment`），折算归核心交易域金额接缝（`convert_to_native`）。
//!
//! 全程只读：注册表读、逐本只读建连、活动本走读连接；不触碰任何写入口。
//! 阻塞 IO（注册表文件、逐本建连）经 [`ledger_infra::db::run_db`] 投放阻塞线程
//! 池；活动本读经壳层统一读入口 [`read_entry`]（ADR-0104），span 归因分步标注。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::commands::data_location::default_data_dir;
use crate::cross_book_summary::{
    CrossBookInvestmentSummary, collect_other_books, merge_readings, read_book_investment,
};
use crate::shell_support::read_entry::read_entry;
use ledger_infra::db::{DbState, book_registry, run_db};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount;

const REGISTRY_UNAVAILABLE_MESSAGE: &str = "账本注册表尚未就绪，请重启应用后再试";

/// 跨账本投资汇总：各本投资域口径（持仓市值/持仓收益/累计收益/可投资资产）的
/// 只读合计，折算目标＝当前活动账本本位币（ADR-0114 决策 3/5/6）。
#[tauri::command]
pub async fn cross_book_investment_summary<R: Runtime>(
    app: AppHandle<R>,
    db: State<'_, DbState>,
) -> Result<CrossBookInvestmentSummary> {
    // ① 注册表（引导配置，打开任何库之前必须可读）：损坏/未配置 → 汇总不可用，
    //    码化拒绝（前端入口在注册表健康时才展示，此处为深链兜底）。
    let default_dir = default_data_dir(&app)?;
    let registry = run_db("cross_book_summary:registry", move || {
        match book_registry::read_registry(&default_dir) {
            book_registry::RegistryRead::Resolved(registry) => Ok(registry),
            _ => Err(AppError::coded(
                "book.registry-unavailable",
                REGISTRY_UNAVAILABLE_MESSAGE,
            )),
        }
    })
    .await?;
    let books = registry.books;
    // 活动指针：校验通过的注册表必达（String 非可缺省，data_location 同款防御面无需要）。
    let active_id = registry.active_id;

    // ② 活动本 schema 版本（活动本建连必经迁移，其版本即当前应用 schema 版本）。
    let active_schema_version = read_entry(
        "cross_book_summary:schema_version",
        db.read_conn.clone(),
        ledger_infra::db::schema_version,
    )
    .await?;

    // ③ 非活动本逐本探测与只读取数（密文=未解锁、空=未初始化、版本不一致即排除，
    //    失败逐本降级，不阻塞其他本出数）。
    let (rows, mut readings) = run_db("cross_book_summary:books", move || {
        let (rows, readings) = collect_other_books(&books, &active_id, active_schema_version);
        Ok((rows, readings))
    })
    .await?;

    // ④ 活动本读数 + 当期汇率折算合并（汇率取活动本汇率表，ADR-0114 决策 3）。
    read_entry("cross_book_summary", db.read_conn.clone(), move |conn| {
        readings.push(read_book_investment(conn)?);
        let target_currency = amount::default_currency_code(conn)?;
        let totals = merge_readings(&readings, &target_currency, &mut |cents, currency| {
            amount::convert_to_native(conn, cents, currency)
        })?;
        Ok(CrossBookInvestmentSummary {
            target_currency,
            converted: totals.converted,
            market_value_cents: totals.market_value_cents,
            unrealized_pnl_cents: totals.unrealized_pnl_cents,
            cumulative_pnl_cents: totals.cumulative_pnl_cents,
            investable_assets_cents: totals.investable_assets_cents,
            books: rows,
        })
    })
    .await
}
