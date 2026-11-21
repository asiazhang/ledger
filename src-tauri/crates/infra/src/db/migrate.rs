use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::schema_guard;
use crate::error::{AppError, Result};

pub const BOOT_SCHEMA_AHEAD: &str = "boot.schema-ahead";

pub fn migrations() -> &'static Migrations<'static> {
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
            M::up(include_str!(
                "../../../../migrations/V025__transaction_index_cleanup.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V026__account_credit_terms.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V027__security_transaction_origin.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V028__instrument_constant_unit_price.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V029__transaction_fx_rate_trace.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V030__transaction_merchant_covering_index.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V031__drop_note_pinyin.sql"
            )),
            M::up(include_str!("../../../../migrations/V032__goals.sql")),
            M::up(include_str!(
                "../../../../migrations/V033__security_transaction_to_instrument_index.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V034__transaction_fundings.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V035__transaction_source_order_no.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V036__sync_db_split.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V037__account_archive.sql"
            )),
            M::up(include_str!(
                "../../../../migrations/V038__transaction_kind_count_index.sql"
            )),
        ])
    })
}

pub fn guard_schema_ahead(conn: &Connection) -> Result<()> {
    if let SchemaVersion::Outside(v) = migrations().current_version(conn)? {
        tracing::warn!(
            db_version = v,
            "库 schema 版本超前于应用迁移链（账本曾由更新版本的应用打开），拒绝迁移"
        );
        return Err(AppError::coded(
            BOOT_SCHEMA_AHEAD,
            "账本由更新版本的应用创建，请升级应用后重试",
        ));
    }
    Ok(())
}

pub fn init_db(conn: &mut Connection) -> Result<()> {
    tracing::info!("开始执行数据库迁移");
    guard_schema_ahead(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(AppError::from)?;
    migrations().to_latest(conn)?;
    tracing::info!("数据库迁移完成");
    schema_guard::verify_schema(conn)?;
    conn.execute("PRAGMA foreign_keys = ON", [])
        .map_err(AppError::from)?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(AppError::from)
}
