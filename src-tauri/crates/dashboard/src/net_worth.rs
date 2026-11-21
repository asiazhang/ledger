use rusqlite::Connection;

use super::model::DashboardOverview;
use ledger_infra::db::now_iso;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount;

const NET_WORTH_FORMULA_TAG: &str = "f2";

const FINGERPRINT_SOURCES: &[(&str, &str, &str)] = &[
    ("accounts", "updated_at", "version"),
    ("transactions", "updated_at", "version"),
    ("security_lots", "updated_at", "version"),
    ("market_prices", "updated_at", "version"),
    ("exchange_rates", "updated_at", "version"),
    ("physical_assets", "updated_at", "version"),
    ("physical_asset_valuations", "created_at", "count"),
    ("account_balance_cache", "updated_at", "none"),
];

pub fn current_fingerprint(conn: &Connection) -> Result<String> {
    let mut sql = String::from("SELECT ");
    for (i, (table, column, discriminator)) in FINGERPRINT_SOURCES.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        let version_part = match *discriminator {
            "version" => " || ':' || COALESCE(SUM(version), 0)",
            "count" => " || ':' || COUNT(*)",
            _ => "",
        };
        sql.push_str(&format!(
            "(SELECT COALESCE(MAX({column}), '-'){version_part} FROM {table})"
        ));
    }
    let parts: Vec<String> = conn.query_row(&sql, [], |r| {
        (0..FINGERPRINT_SOURCES.len())
            .map(|i| r.get::<_, String>(i))
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let fingerprint = FINGERPRINT_SOURCES
        .iter()
        .zip(parts)
        .map(|((table, _, _), value)| format!("{table}={value}"))
        .collect::<Vec<_>>()
        .join("|");
    Ok(format!("{NET_WORTH_FORMULA_TAG}|{fingerprint}"))
}

pub fn read_valid(conn: &Connection, fingerprint: &str) -> Result<Option<DashboardOverview>> {
    let row = conn
        .query_row(
            "SELECT fingerprint, native_currency, net_worth_cents, accounts_balance_cents, \
             holdings_market_value_cents, physical_assets_value_cents \
             FROM net_worth_cache WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    match row {
        Some((stored, native_currency, net_worth_cents, accounts, holdings, physical))
            if stored == fingerprint =>
        {
            Ok(Some(DashboardOverview {
                native_currency,
                net_worth_cents,
                accounts_balance_cents: accounts,
                holdings_market_value_cents: holdings,
                physical_assets_value_cents: physical,
            }))
        }
        _ => Ok(None),
    }
}

pub fn write(conn: &Connection, fingerprint: &str, overview: &DashboardOverview) -> Result<()> {
    conn.execute(
        "INSERT INTO net_worth_cache \
         (id, fingerprint, native_currency, net_worth_cents, accounts_balance_cents, \
          holdings_market_value_cents, physical_assets_value_cents, updated_at) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(id) DO UPDATE SET \
             fingerprint = excluded.fingerprint, \
             native_currency = excluded.native_currency, \
             net_worth_cents = excluded.net_worth_cents, \
             accounts_balance_cents = excluded.accounts_balance_cents, \
             holdings_market_value_cents = excluded.holdings_market_value_cents, \
             physical_assets_value_cents = excluded.physical_assets_value_cents, \
             updated_at = excluded.updated_at",
        rusqlite::params![
            fingerprint,
            overview.native_currency,
            overview.net_worth_cents,
            overview.accounts_balance_cents,
            overview.holdings_market_value_cents,
            overview.physical_assets_value_cents,
            now_iso()
        ],
    )?;
    Ok(())
}

pub fn query_dashboard_overview(conn: &Connection) -> Result<DashboardOverview> {
    let (overview, fingerprint, cache_hit) = ensure_transaction(conn, || {
        let fingerprint = current_fingerprint(conn)?;
        if let Some(cached) = read_valid(conn, &fingerprint)?
            && cached.native_currency == amount::default_currency_code(conn)?
        {
            return Ok((cached, fingerprint, true));
        }
        let overview = super::compute_dashboard_overview(conn)?;
        Ok((overview, fingerprint, false))
    })?;
    if !cache_hit {
        write(conn, &fingerprint, &overview)?;
    }
    Ok(overview)
}
