//! 净资产终值缓存存储（issue #491 / ADR-0067）：只包存储，不写公式。
//!
//! 缓存行是首页净资产总览的派生终值（真实财富视角三腿合计，公式留在
//! dashboard 域的既有实时聚合函数）。正确性由**读探针指纹**保证：读取方
//! 先算当前指纹（各贡献表 `MAX(updated_at)` 组合），与缓存行不一致即视为
//! 失效，调实时聚合重算回填——无定时任务、无写路径挂钩（源表写入天然
//! 推进 `MAX(updated_at)`）。
//!
//! 指纹包含 `account_balance_cache.MAX(updated_at)`：该表毫秒精度时间戳
//! 在每次交易/账户写入时被无条件刷新（写路径整体重算接缝），为秒级精度
//! 的源表时间戳补上同秒内连续写入的区分度；实物资产估值表为 append-only
//! 冻结形态，无 `updated_at` 列，以 `MAX(created_at)` 承担同等角色。

use rusqlite::Connection;

use crate::db::now_iso;
use crate::error::Result;

/// 缓存的净资产终值（本位币口径，金额单位：分）。与 dashboard 域读模型
/// 同形而独立定义：基础设施不反向依赖聚合域。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedNetWorth {
    pub native_currency: String,
    pub net_worth_cents: i64,
    pub accounts_balance_cents: i64,
    pub holdings_market_value_cents: i64,
    pub physical_assets_value_cents: i64,
}

/// 参与净资产计算的贡献表及其指纹输入（指纹输入清单，单一真源）。
/// 元组：(表名, 时间戳列, 同秒判别器)。
///
/// 时间戳列多为秒级精度（既有冻结约定，不改格式），同秒内连续两次写入
/// `MAX(updated_at)` 不变——故叠加判别器消除同秒盲区：带 `version` 列的表用
/// `SUM(version)`（插入 version=1 或 version+1 都令判别值严格变化）；
/// append-only 估值表无 version 列、只有插入，用 `COUNT(*)`；
/// `account_balance_cache` 毫秒精度时间戳每次写入无条件刷新（ADR-0067），
/// 天然无同秒盲区，不叠加（`none`）。
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

/// 当前输入指纹：各贡献表 `MAX(时间戳)`（带 version 判别的表再拼 `SUM(version)`）
/// 的命名拼接。空表记 `-:0` 或 `-`（尚无任何行；有行后指纹必然变化）。
pub fn current_fingerprint(conn: &Connection) -> Result<String> {
    let mut sql = String::from("SELECT ");
    for (i, (table, column, discriminator)) in FINGERPRINT_SOURCES.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        // 表名/列名来自上述编译期常量，非用户输入，拼接安全。
        // 判别器：秒级时间戳表的同秒连续写入由 version 严格递增区分、append-only
        // 表由行数区分（时间戳分量不变、判别分量必变，指纹必失配）。
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
    Ok(FINGERPRINT_SOURCES
        .iter()
        .zip(parts)
        .map(|((table, _, _), value)| format!("{table}={value}"))
        .collect::<Vec<_>>()
        .join("|"))
}
/// 读取与给定指纹匹配的缓存终值；无缓存行或指纹不符返回 `None`
/// （失效判定收口在此，调用方据此决定是否重算回填）。
pub fn read_valid(conn: &Connection, fingerprint: &str) -> Result<Option<CachedNetWorth>> {
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
            Ok(Some(CachedNetWorth {
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

/// 回填/刷新缓存终值（单例行 UPSERT）。由读探针在实时重算后调用。
pub fn write(conn: &Connection, fingerprint: &str, value: &CachedNetWorth) -> Result<()> {
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
            value.native_currency,
            value.net_worth_cents,
            value.accounts_balance_cents,
            value.holdings_market_value_cents,
            value.physical_assets_value_cents,
            now_iso()
        ],
    )?;
    Ok(())
}
