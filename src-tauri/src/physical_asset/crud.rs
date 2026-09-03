//! 实物资产域行为（issue #466 T1 建档 / 列表 / 详情，issue #467 T2 编辑与更新估值）。
//!
//! 估值必填 = 建档同时写入第一条估值历史行（两表写入同事务原子）；当前估值 =
//! 最新一条历史行（估值日期最新，同日按插入序 = UUID v7 主键降序首条）；
//! 更新估值只追加新历史行不改写（旧值保留，当前估值随之变为最新一条，T2）；
//! 编辑档案只改名称与购买信息（估值不经编辑变更，T2）。在持合计 = Σ 在持资产
//! 当前估值经 Amount 接缝折本位币（当期汇率，缺汇率错误上抛）。写路径成功落库
//! 后调用 `notify`（失效信号回调注入，保单域同款；生产路径发 `ledger:changed`，
//! 失败不至此处）。

use rusqlite::{Connection, OptionalExtension};

use super::model::{
    AssetRecord, PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetList,
    PhysicalAssetStatus, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
use super::validation::{
    validate_dispose_input, validate_input, validate_update_input, validate_valuation_input,
};
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
    // 状态筛选解析单点：合法值语义与实体状态解析同源（PhysicalAssetStatus::parse），
    // 未知值码化报错（参数随 T1 命令面一次留足，处置筛选由 T3 消费）。
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

    let native_currency = default_currency_code().to_string();
    let mut assets = Vec::with_capacity(records.len());
    let mut holding_total_native_cents = 0i64;
    for record in records {
        // 折算/累计先于筛选：在持合计恒为全部在持行（回看已处置时合计不变）；
        // 折算与搬运的单一落点在 into_entity（列表/详情共用）。
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
}

/// 行记录 → 读模型实体（列表 / 详情共用的单一搬运点）：在持行经 Amount 接缝
/// 折算本位币（缺汇率错误上抛，不以零或缺项静默通过）并累计进 `holding_total`；
/// 已处置行不折算（native 为 `None`，不进在持口径）。
fn into_entity(
    conn: &Connection,
    record: AssetRecord,
    holding_total: &mut i64,
    native_currency: &str,
) -> Result<PhysicalAsset> {
    let current_valuation_native_cents = match record.status {
        PhysicalAssetStatus::Holding => {
            let native = convert_to_native(
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

/// 前置存在性检查（T2 写入口共用单点，裸 SELECT 与折算读路径解耦）：
/// 不存在（或已软删除）→ 码化 NotFound。不能用 `get_physical_asset` 判存在——
/// 详情读会做本位币折算，缺汇率时报错会被误判为「不存在」，掩盖真实条件
/// （缺汇率必须原样上抛，CONTEXT 口径「不以零或缺项静默通过」）。
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

/// 编辑档案（issue #467 T2）：只改名称与购买信息（估值不经本入口变更，
/// 只能走 [`update_physical_asset_valuation`] 追加历史行）。不存在（或已软
/// 删除）→ 码化 NotFound；成功后 bump version / updated_at 并调用 `notify`。
/// 购买价可清空（存 NULL，与币种成对落空）。
pub fn update_physical_asset(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetUpdateInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    require_asset_exists(conn, id)?;
    let normalized = validate_update_input(conn, &input)?;

    let updated = conn.execute(
        "UPDATE physical_assets SET name=?2, purchase_date=?3, purchase_price_cents=?4, \
         purchase_currency_code=?5, updated_at=?6, version=version+1, device_id=?7 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            normalized.name,
            normalized.purchase_date,
            normalized.purchase_price_cents,
            normalized.purchase_currency_code,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    notify();
    Ok(())
}

/// 更新估值（issue #467 T2）：追加一条估值历史行（只追加不改写，旧值保留），
/// 当前估值 = 最新一条（估值日期最新，同日按插入序）由读口径自然生效，列表 /
/// 详情无需额外写入。不存在（或已软删除）→ 码化 NotFound；成功后调用
/// `notify`。估值行无更新语义，不触碰资产行的 version（LWW 口径不变，
/// 与 V015 迁移注释「估值历史仅留审计」一致）。
pub fn update_physical_asset_valuation(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetValuationInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    // 前置存在性检查（含软删过滤）：估值历史依附资产存续，不允许孤儿行。
    require_asset_exists(conn, id)?;
    let normalized = validate_valuation_input(conn, &input)?;

    conn.execute(
        "INSERT INTO physical_asset_valuations \
         (id,asset_id,valuation_date,amount_cents,currency_code,device_id,created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            new_uuid(),
            id,
            normalized.valuation_date,
            normalized.amount_cents,
            normalized.currency_code,
            device_id(),
            now_iso(),
        ],
    )?;
    notify();
    Ok(())
}

/// 处置（issue #468 T3）：状态标记转 `disposed` 并记录处置日期（必填）与
/// 可选处置价 + 币种（纯记录，不进任何金额口径）。已处置不进默认列表 /
/// 在持合计（读口径自然生效，无需额外写入）。对已处置资产再次处置 = 修正
/// 处置信息（更新日期与价格，版本递增，先例物品域）。不存在（或已软删除）
/// → 码化 NotFound；成功后 bump version / updated_at 并调用 `notify`。
pub fn dispose_physical_asset(
    conn: &Connection,
    id: &str,
    input: PhysicalAssetDisposeInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    // 处置日期与购买日期的先后守卫需要既有行的购买日期：单列裸读（与折算
    // 读路径解耦，先例 require_asset_exists 的注释理由）。
    let purchase_date: Option<String> = conn
        .query_row(
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
        })?;
    let normalized = validate_dispose_input(conn, purchase_date.as_deref(), &input)?;

    let updated = conn.execute(
        "UPDATE physical_assets SET status='disposed', disposal_date=?2, \
         disposal_price_cents=?3, disposal_currency_code=?4, updated_at=?5, \
         version=version+1, device_id=?6 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            normalized.disposal_date,
            normalized.disposal_price_cents,
            normalized.disposal_currency_code,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    // 处置成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(())
}

/// 软删除（issue #468 T3）：置 `is_deleted=1`，不物理移除——数据与估值历史
/// 保留（误删有后悔药），列表（默认在持 / 已处置筛选）与在持合计经读口径
/// `WHERE is_deleted=0` 自动过滤。不存在（含已删除）→ 码化 NotFound；
/// 成功后 bump version / updated_at 并调用 `notify`。
pub fn delete_physical_asset(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    require_asset_exists(conn, id)?;
    let deleted = conn.execute(
        "UPDATE physical_assets SET is_deleted=1, updated_at=?2, \
         version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    debug_assert_eq!(
        deleted, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    // 删除成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(())
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

    // 详情与列表同一读口径（单一搬运点）；详情无合计语义，累计值丢弃。
    let native_currency = default_currency_code().to_string();
    let mut unused_total = 0i64;
    into_entity(conn, record, &mut unused_total, &native_currency)
}
