//! 迁移链与 schema 初始化（自 `db/mod.rs` 按职责拆出，issue #1127，纯移动）：
//! 迁移集合定义、`init_db` 尾部单点（schema 守卫接线，ADR-0100）与 schema
//! 版本读取（`user_version`，迁移自动追踪的库级事实）。

use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use super::schema_guard;
use crate::error::{AppError, Result};

/// 迁移集合。新增 schema 变更或种子数据时，在 `src-tauri/migrations/` 下新建
/// `V00X__名称.sql`，并在 `migrations()` 的 `vec!` 里追加
/// `M::up(include_str!("../../../../migrations/V00X__名称.sql"))`。
/// 版本由 SQLite 的 `user_version` 字段自动追踪，无需手动维护版本表。
pub(crate) fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations<'static>> = OnceLock::new();
    MIGRATIONS.get_or_init(|| {
        Migrations::new(vec![
            M::up(include_str!("../../../../migrations/V001__initial.sql")),
            M::up(include_str!("../../../../migrations/V002__investment.sql")),
            M::up(include_str!(
                "../../../../migrations/V003__scheduled_transactions.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V004__seed_defaults.sql"
            )),
            // 注：原 V005__search_index.sql（搜索索引）已从序列整体移除（见 ADR-0027），
            // 序列号不回填，后续迁移文件名保持不变；新库不再产生搜索索引对象。
            M::up(include_str!(
                "../../../../migrations/V006__transaction_amount_index.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V007__transaction_idempotency_key.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V008__app_settings.sql"
            )),
            M::up(include_str!("../../../../migrations/V009__items.sql")),
            M::up(include_str!(
                "../../../../migrations/V010__price_history.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V011__instruments_source.sql"
            )),
            M::up(include_str!("../../../../migrations/V012__policies.sql")),
            M::up(include_str!(
                "../../../../migrations/V013__transaction_policy_id.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V014__subscription_plan_policy_id.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V015__physical_assets.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V016__transaction_structural_indexes.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V017__balance_net_worth_cache.sql"
            )),
            M::up(include_str!("../../../../migrations/V018__note_pinyin.sql")),
            M::up(include_str!(
                "../../../../migrations/V019__insurer_dictionary.sql"
            )),
            M::up(include_str!("../../../../migrations/V020__sync_oplog.sql")),
            M::up(include_str!(
                "../../../../migrations/V021__sync_merge_semantics.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V022__sync_checkpoint.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V023__transaction_funding_account.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V024__security_lot_adjustments.sql"
            )),
        ])
    })
}

/// 初始化数据库 schema 与默认种子数据（全部由迁移驱动）。
pub fn init_db(conn: &mut Connection) -> Result<()> {
    tracing::info!("开始执行数据库迁移");
    migrations().to_latest(conn)?;
    tracing::info!("数据库迁移完成");
    // Schema 漂移守卫（issue #971/#992 / ADR-0100）：to_latest 只比 user_version，
    // 「版本已达最新但 schema 内容漂移」的库在此确定性拦截。守卫接线是尾部
    // 单点：全部生产建连路径统一收口本函数，一处接线零遗漏。接线负向判据
    //（#963 惯例 / ADR-0087）：删除本调用，e2e「缺列漂移的明文库启动进入
    // 失败状态」场景变红。
    schema_guard::verify_schema(conn)?;
    Ok(())
}

/// 当前 schema 版本（SQLite `user_version`，迁移自动追踪）。
///
/// 同步 op 产生时随身携带（issue #855 / ADR-0091：产生时 schema 版本），
/// schema 偏斜判定（#856）依据；作为通用库级事实收口在基础设施。
pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(AppError::from)
}
