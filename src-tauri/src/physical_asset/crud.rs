//! 实物资产域行为（issue #466 T1）：建档、列表（含在持合计）与详情。
//!
//! 估值必填 = 建档同时写入第一条估值历史行（两表写入同事务原子）；当前估值 =
//! 最新一条历史行（估值日期最新，同日按插入序 = UUID v7 主键降序首条）；
//! 在持合计 = Σ 在持资产当前估值经 Amount 接缝折本位币（当期汇率，缺汇率
//! 错误上抛）。写路径成功落库后调用 `notify`（失效信号回调注入，保单域同款；
//! 生产路径发 `ledger:changed`，失败不至此处）。

use rusqlite::Connection;

use super::model::{
    AssetRecord, PhysicalAsset, PhysicalAssetInput, PhysicalAssetList, PhysicalAssetStatus,
};
use super::validation::validate_input;
use crate::db::query::{query_all, query_one};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::transaction::amount::{convert_to_native, default_currency_code};

/// 资产全列 + 当前估值三件套（JOIN 每资产最新一条估值历史行）。
/// 「最新」= 估值日期最新，同日按插入序（UUID v7 主键时间有序，降序首条）。
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

/// 「保证处于事务中」（嵌套感知，ADR-0033 决策 #2）：连接 autocommit 则自持
/// BEGIN/COMMIT/ROLLBACK，已在事务中则加入外层。建档要原子写资产行 + 首条
/// 估值行两表，缺失会造成「资产无当前估值」的半行（列表 JOIN 丢行、净资产
/// 缺腿）。域内私有助手，形状与交易域 `transaction::behavior::ensure_transaction`
/// 同款（该函数为交易域私有，不跨域复用实现）。
fn in_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f();
    }
    conn.execute("BEGIN", [])?;
    match f() {
        Ok(v) => match conn.execute("COMMIT", []) {
            Ok(_) => Ok(v),
            // COMMIT 失败：尽力回滚清理残留，再上抛提交错误（同交易域先例）。
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}

/// 建档：校验 → 落库（资产行 + 首条估值行，同事务）→ 成功后调用 `notify`。
/// 校验语义见 `validation::validate_input`（缺名称 / 缺估值显式报错、不落库）。
pub fn create_physical_asset(
    conn: &Connection,
    input: PhysicalAssetInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let normalized = validate_input(conn, &input)?;

    let id = in_transaction(conn, || {
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO physical_assets \
             (id,name,purchase_date,purchase_price_cents,purchase_currency_code,\
             status,disposal_date,disposal_price_cents,disposal_currency_code,\
             created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,'holding',NULL,NULL,NULL,?6,?6,1,?7,0)",
            rusqlite::params![
                id,
                normalized.name,
                normalized.purchase_date,
                normalized.purchase_price_cents,
                normalized.purchase_currency_code,
                now,
                device_id(),
            ],
        )?;
        conn.execute(
            "INSERT INTO physical_asset_valuations \
             (id,asset_id,valuation_date,amount_cents,currency_code,device_id,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                new_uuid(),
                id,
                normalized.initial_valuation_date,
                normalized.initial_valuation_cents,
                normalized.initial_valuation_currency_code,
                device_id(),
                now_iso(),
            ],
        )?;
        Ok(id)
    })?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 列表：未删除资产（按状态筛选，缺省 = 在持）+ **在持**估值合计。
/// 一次拉全部未删行（JOIN 最新估值行），在持行经 Amount 接缝折算本位币
/// （缺汇率错误上抛，不以零或缺项静默通过）；合计口径恒为在持资产，
/// 与筛选无关（回看已处置时「家底合计」不变）。排序按创建先后，列表稳定。
pub fn list_physical_assets(conn: &Connection, status: Option<&str>) -> Result<PhysicalAssetList> {
    let filter = match status {
        None | Some("holding") => PhysicalAssetStatus::Holding,
        Some("disposed") => PhysicalAssetStatus::Disposed,
        Some(other) => {
            return Err(AppError::codedp(
                "physical-asset.status-invalid",
                format!("未知资产状态筛选: {other}（合法值: holding/disposed）"),
                &[other],
            ));
        }
    };

    let records: Vec<AssetRecord> = query_all(
        conn,
        &format!(
            "SELECT {ASSET_WITH_VALUATION_COLUMNS} {ASSET_WITH_VALUATION_FROM} \
             WHERE a.is_deleted=0 ORDER BY a.created_at, a.id"
        ),
        [],
    )?;

    let native_currency = default_currency_code().to_string();
    let mut assets = Vec::with_capacity(records.len());
    let mut holding_total_native_cents = 0i64;
    for record in records {
        // 折算只发生在需要进合计的在持行；已处置行不折算（native 为 None）。
        let current_valuation_native_cents = match record.status {
            PhysicalAssetStatus::Holding => {
                let native = convert_to_native(
                    conn,
                    record.current_valuation_cents,
                    &record.current_valuation_currency_code,
                )?;
                holding_total_native_cents += native;
                Some(native)
            }
            PhysicalAssetStatus::Disposed => None,
        };
        if record.status != filter {
            continue;
        }
        assets.push(PhysicalAsset {
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
            native_currency: native_currency.clone(),
        });
    }

    Ok(PhysicalAssetList {
        assets,
        holding_total_native_cents,
        native_currency,
    })
}

/// 按 `id` 读单个未删除资产（详情）：不存在（或已软删除）→ 码化 NotFound。
pub fn get_physical_asset(conn: &Connection, id: &str) -> Result<PhysicalAsset> {
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

    // 详情与列表同一读口径：在持行折算本位币（缺汇率错误上抛）。
    let native_currency = default_currency_code().to_string();
    let current_valuation_native_cents = match record.status {
        PhysicalAssetStatus::Holding => Some(convert_to_native(
            conn,
            record.current_valuation_cents,
            &record.current_valuation_currency_code,
        )?),
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
        native_currency,
    })
}
