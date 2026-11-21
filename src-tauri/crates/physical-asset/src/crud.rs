use rusqlite::{Connection, OptionalExtension};

use super::command::{PhysicalAssetCommand, ValuationCommandRow, record_local};
use super::model::{
    AssetRecord, PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetList,
    PhysicalAssetStatus, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
use super::validation::{
    validate_dispose_input, validate_input, validate_update_input, validate_valuation_input,
};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount::{convert_to_native_current, default_currency_code};

const ASSET_WITH_VALUATION_COLUMNS: &str = "\
     a.id,a.name,a.purchase_date,a.purchase_price_cents,a.purchase_currency_code,\
     a.status,a.disposal_date,a.disposal_price_cents,a.disposal_currency_code,\
     a.created_at,a.updated_at,a.version,a.device_id,a.is_deleted,\
     v.amount_cents AS current_valuation_cents,\
     v.currency_code AS current_valuation_currency_code,\
     v.valuation_date AS current_valuation_date";

const ASSET_WITH_VALUATION_FROM: &str = "\
     FROM physical_assets a \
     JOIN physical_asset_valuations v ON v.asset_id = a.id \
       AND v.id = (SELECT v2.id FROM physical_asset_valuations v2 \
                   WHERE v2.asset_id = a.id \
                   ORDER BY v2.valuation_date DESC, v2.id DESC LIMIT 1)";

pub fn create_physical_asset(
    conn: &Connection,
    input: PhysicalAssetInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let normalized = validate_input(conn, &input)?;

    let id = ensure_transaction(conn, || {
        let id = new_uuid();
        let first_valuation = ValuationCommandRow {
            id: new_uuid(),
            valuation_date: normalized.initial_valuation_date.clone(),
            amount_cents: normalized.initial_valuation_cents,
            currency_code: normalized.initial_valuation_currency_code.clone(),
        };
        write_create(
            conn,
            &NewAssetRows {
                id: &id,
                name: &normalized.name,
                purchase_date: normalized.purchase_date.as_deref(),
                purchase_price_cents: normalized.purchase_price_cents,
                purchase_currency_code: normalized.purchase_currency_code.as_deref(),
                first_valuation: &first_valuation,
            },
        )?;
        record_local(
            conn,
            PhysicalAssetCommand::Create {
                id: id.clone(),
                name: normalized.name.clone(),
                purchase_date: normalized.purchase_date.clone(),
                purchase_price_cents: normalized.purchase_price_cents,
                purchase_currency_code: normalized.purchase_currency_code.clone(),
                first_valuation,
            },
        )?;
        Ok(id)
    })?;
    notify();
    Ok(id)
}

struct NewAssetRows<'a> {
    id: &'a str,
    name: &'a str,
    purchase_date: Option<&'a str>,
    purchase_price_cents: Option<i64>,
    purchase_currency_code: Option<&'a str>,
    first_valuation: &'a ValuationCommandRow,
}

fn write_create(conn: &Connection, asset: &NewAssetRows<'_>) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO physical_assets \
         (id,name,purchase_date,purchase_price_cents,purchase_currency_code,\
         status,disposal_date,disposal_price_cents,disposal_currency_code,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,'holding',NULL,NULL,NULL,?6,?6,1,?7,0)",
        rusqlite::params![
            asset.id,
            asset.name,
            asset.purchase_date,
            asset.purchase_price_cents,
            asset.purchase_currency_code,
            now,
            device_id(conn)?,
        ],
    )?;
    conn.execute(
        "INSERT INTO physical_asset_valuations \
         (id,asset_id,valuation_date,amount_cents,currency_code,device_id,created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            asset.first_valuation.id,
            asset.id,
            asset.first_valuation.valuation_date,
            asset.first_valuation.amount_cents,
            asset.first_valuation.currency_code,
            device_id(conn)?,
            now_iso(),
        ],
    )?;
    Ok(())
}

pub fn list_physical_assets(conn: &Connection, status: Option<&str>) -> Result<PhysicalAssetList> {
    ensure_transaction(conn, || {
        let filter = match status {
            None => PhysicalAssetStatus::Holding,
            Some(raw) => PhysicalAssetStatus::parse(raw)
                .map_err(|e| AppError::codedp("physical-asset.status-invalid", e, &[raw]))?,
        };

        let records: Vec<AssetRecord> = query_all(
            conn,
            &format!(
                "SELECT {ASSET_WITH_VALUATION_COLUMNS} {ASSET_WITH_VALUATION_FROM} \
                 WHERE a.is_deleted=0 ORDER BY a.created_at, a.id"
            ),
            [],
        )?;

        let native_currency = default_currency_code(conn)?;
        let mut assets = Vec::with_capacity(records.len());
        let mut holding_total_native_cents = 0i64;
        for record in records {
            let asset = into_entity(
                conn,
                record,
                &mut holding_total_native_cents,
                &native_currency,
            )?;
            if asset.status == filter {
                assets.push(asset);
            }
        }

        Ok(PhysicalAssetList {
            assets,
            holding_total_native_cents,
            native_currency,
        })
    })
}

fn into_entity(
    conn: &Connection,
    record: AssetRecord,
    holding_total: &mut i64,
    native_currency: &str,
) -> Result<PhysicalAsset> {
    let current_valuation_native_cents = match record.status {
        PhysicalAssetStatus::Holding => {
            let native = convert_to_native_current(
                conn,
                record.current_valuation_cents,
                &record.current_valuation_currency_code,
            )?;
            *holding_total += native;
            Some(native)
        }
        PhysicalAssetStatus::Disposed => None,
    };
    Ok(PhysicalAsset {
        id: record.id,
        name: record.name,
        purchase_date: record.purchase_date,
        purchase_price_cents: record.purchase_price_cents,
        purchase_currency_code: record.purchase_currency_code,
        status: record.status,
        disposal_date: record.disposal_date,
        disposal_price_cents: record.disposal_price_cents,
        disposal_currency_code: record.disposal_currency_code,
        created_at: record.created_at,
        updated_at: record.updated_at,
        version: record.version,
        device_id: record.device_id,
        is_deleted: record.is_deleted,
        current_valuation_cents: record.current_valuation_cents,
        current_valuation_currency_code: record.current_valuation_currency_code,
        current_valuation_date: record.current_valuation_date,
        current_valuation_native_cents,
        native_currency: native_currency.to_string(),
    })
}

fn require_asset_exists(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM physical_assets WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "physical-asset.not-found",
            format!("实物资产不存在: {id}"),
            &[id],
        ));
    }
    Ok(())
}

fn require_asset_purchase_date(conn: &Connection, id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT purchase_date FROM physical_assets WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "physical-asset.not-found",
            format!("实物资产不存在: {id}"),
            &[id],
        )
    })
}

pub fn update_physical_asset(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetUpdateInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        require_asset_exists(conn, id)?;
        let normalized = validate_update_input(conn, &input)?;
        write_update(
            conn,
            id,
            &normalized.name,
            normalized.purchase_date.as_deref(),
            normalized.purchase_price_cents,
            normalized.purchase_currency_code.as_deref(),
        )?;
        record_local(
            conn,
            PhysicalAssetCommand::Update {
                id: id.to_string(),
                name: normalized.name.clone(),
                purchase_date: normalized.purchase_date.clone(),
                purchase_price_cents: normalized.purchase_price_cents,
                purchase_currency_code: normalized.purchase_currency_code.clone(),
            },
        )
    })?;
    notify();
    Ok(())
}

fn write_update(
    conn: &Connection,
    id: &str,
    name: &str,
    purchase_date: Option<&str>,
    purchase_price_cents: Option<i64>,
    purchase_currency_code: Option<&str>,
) -> Result<()> {
    let updated = conn.execute(
        "UPDATE physical_assets SET name=?2, purchase_date=?3, purchase_price_cents=?4, \
         purchase_currency_code=?5, updated_at=?6, version=version+1, device_id=?7 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            name,
            purchase_date,
            purchase_price_cents,
            purchase_currency_code,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(())
}

pub fn update_physical_asset_valuation(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetValuationInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        require_asset_exists(conn, id)?;
        let normalized = validate_valuation_input(conn, &input)?;
        let valuation = ValuationCommandRow {
            id: new_uuid(),
            valuation_date: normalized.valuation_date.clone(),
            amount_cents: normalized.amount_cents,
            currency_code: normalized.currency_code.clone(),
        };
        write_valuation(conn, id, &valuation)?;
        record_local(
            conn,
            PhysicalAssetCommand::UpdateValuation {
                asset_id: id.to_string(),
                valuation,
            },
        )
    })?;
    notify();
    Ok(())
}

fn write_valuation(
    conn: &Connection,
    asset_id: &str,
    valuation: &ValuationCommandRow,
) -> Result<()> {
    conn.execute(
        "INSERT INTO physical_asset_valuations \
         (id,asset_id,valuation_date,amount_cents,currency_code,device_id,created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            valuation.id,
            asset_id,
            valuation.valuation_date,
            valuation.amount_cents,
            valuation.currency_code,
            device_id(conn)?,
            now_iso(),
        ],
    )?;
    Ok(())
}

pub fn dispose_physical_asset(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetDisposeInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        let purchase_date = require_asset_purchase_date(conn, id)?;
        let normalized = validate_dispose_input(conn, purchase_date.as_deref(), &input)?;
        write_dispose(
            conn,
            id,
            &normalized.disposal_date,
            normalized.disposal_price_cents,
            normalized.disposal_currency_code.as_deref(),
        )?;
        record_local(
            conn,
            PhysicalAssetCommand::Dispose {
                id: id.to_string(),
                disposal_date: normalized.disposal_date.clone(),
                disposal_price_cents: normalized.disposal_price_cents,
                disposal_currency_code: normalized.disposal_currency_code.clone(),
            },
        )
    })?;
    notify();
    Ok(())
}

fn write_dispose(
    conn: &Connection,
    id: &str,
    disposal_date: &str,
    disposal_price_cents: Option<i64>,
    disposal_currency_code: Option<&str>,
) -> Result<()> {
    let updated = conn.execute(
        "UPDATE physical_assets SET status='disposed', disposal_date=?2, \
         disposal_price_cents=?3, disposal_currency_code=?4, updated_at=?5, \
         version=version+1, device_id=?6 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            disposal_date,
            disposal_price_cents,
            disposal_currency_code,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(())
}

pub fn delete_physical_asset(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    ensure_transaction(conn, || {
        require_asset_exists(conn, id)?;
        write_delete(conn, id)?;
        record_local(conn, PhysicalAssetCommand::Delete { id: id.to_string() })
    })?;
    notify();
    Ok(())
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let deleted = conn.execute(
        "UPDATE physical_assets SET is_deleted=1, updated_at=?2, \
         version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    debug_assert_eq!(
        deleted, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(())
}

pub(crate) fn replay_create(
    conn: &Connection,
    id: &str,
    name: &str,
    purchase_date: Option<&str>,
    purchase_price_cents: Option<i64>,
    purchase_currency_code: Option<&str>,
    first_valuation: &ValuationCommandRow,
) -> Result<()> {
    let normalized = validate_input(
        conn,
        &PhysicalAssetInput {
            name: name.to_string(),
            purchase_date: purchase_date.map(String::from),
            purchase_price_cents,
            purchase_currency_code: purchase_currency_code.map(String::from),
            initial_valuation_cents: Some(first_valuation.amount_cents),
            initial_valuation_currency_code: Some(first_valuation.currency_code.clone()),
            initial_valuation_date: Some(first_valuation.valuation_date.clone()),
        },
    )?;
    write_create(
        conn,
        &NewAssetRows {
            id,
            name: &normalized.name,
            purchase_date: normalized.purchase_date.as_deref(),
            purchase_price_cents: normalized.purchase_price_cents,
            purchase_currency_code: normalized.purchase_currency_code.as_deref(),
            first_valuation,
        },
    )
}

pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    purchase_date: Option<&str>,
    purchase_price_cents: Option<i64>,
    purchase_currency_code: Option<&str>,
) -> Result<()> {
    require_asset_exists(conn, id)?;
    let normalized = validate_update_input(
        conn,
        &PhysicalAssetUpdateInput {
            name: name.to_string(),
            purchase_date: purchase_date.map(String::from),
            purchase_price_cents,
            purchase_currency_code: purchase_currency_code.map(String::from),
        },
    )?;
    write_update(
        conn,
        id,
        &normalized.name,
        normalized.purchase_date.as_deref(),
        normalized.purchase_price_cents,
        normalized.purchase_currency_code.as_deref(),
    )
}

pub(crate) fn replay_valuation(
    conn: &Connection,
    asset_id: &str,
    valuation: &ValuationCommandRow,
) -> Result<()> {
    require_asset_exists(conn, asset_id)?;
    validate_valuation_input(
        conn,
        &PhysicalAssetValuationInput {
            amount_cents: Some(valuation.amount_cents),
            currency_code: Some(valuation.currency_code.clone()),
            valuation_date: Some(valuation.valuation_date.clone()),
        },
    )?;
    write_valuation(conn, asset_id, valuation)
}

pub(crate) fn replay_dispose(
    conn: &Connection,
    id: &str,
    disposal_date: &str,
    disposal_price_cents: Option<i64>,
    disposal_currency_code: Option<&str>,
) -> Result<()> {
    let purchase_date = require_asset_purchase_date(conn, id)?;
    let normalized = validate_dispose_input(
        conn,
        purchase_date.as_deref(),
        &PhysicalAssetDisposeInput {
            disposal_date: Some(disposal_date.to_string()),
            disposal_price_cents,
            disposal_currency_code: disposal_currency_code.map(String::from),
        },
    )?;
    write_dispose(
        conn,
        id,
        &normalized.disposal_date,
        normalized.disposal_price_cents,
        normalized.disposal_currency_code.as_deref(),
    )
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    require_asset_exists(conn, id)?;
    write_delete(conn, id)
}

pub fn get_physical_asset(conn: &Connection, id: &str) -> Result<PhysicalAsset> {
    ensure_transaction(conn, || {
        let record = query_one::<AssetRecord, _>(
            conn,
            &format!(
                "SELECT {ASSET_WITH_VALUATION_COLUMNS} {ASSET_WITH_VALUATION_FROM} \
                 WHERE a.is_deleted=0 AND a.id=?1"
            ),
            [id],
        )?
        .ok_or_else(|| {
            AppError::codedp_not_found(
                "physical-asset.not-found",
                format!("实物资产不存在: {id}"),
                &[id],
            )
        })?;

        let native_currency = default_currency_code(conn)?;
        let mut unused_total = 0i64;
        into_entity(conn, record, &mut unused_total, &native_currency)
    })
}
