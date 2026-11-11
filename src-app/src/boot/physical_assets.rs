//! 实物资产视图读写接缝（#2049）：实物资产清单（含在持估值合计同源快照）
//! 与写路径（创建 / 修改 / 改估值 / 处置 / 软删）直调
//! `ledger-physical-asset` 域入口。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// ---------------------------------------------------------------------------
// 实物资产视图读写接缝（#2049）：实物资产清单（含在持估值合计同源快照）
// 直调 `ledger-physical-asset` 域入口——与旧壳 physicalAsset 命令族
//（listPhysicalAssets / createPhysicalAsset / updatePhysicalAsset /
// updatePhysicalAssetValuation / disposePhysicalAsset / deletePhysicalAsset）
// 消费的同一域函数，不经 IPC。域内单正文：建档估值必填（首条历史行）、
// 日期守卫（估值/处置拒绝未来、处置不早于购买）、金额与币种成对、软删保
// 留库行与估值历史、折算与在持合计（Amount 接缝当期汇率，缺汇率错误上
// 抛）——本壳不自建语义。失效信号回调注入在域写入口参数位保留（重读职责
// 归调用方，本壳传空闭包）：写成功后经信号族统一广播、视图订阅重读。写连
// 接是成对建连的写侧，读连接是只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 实物资产清单与在持估值合计（旧壳 `listPhysicalAssets` 命令同源）+ 状态
/// 筛选（在持 / 已处置，合法值解析归域）。折算与合计全在域读路径单点产
/// 出，本壳零二次聚合（V017 / ADR-0067 资产合计同纪律：算术不进壳）。
pub fn physical_asset_rows(
    db: &DbState,
    status_filter: Option<&str>,
) -> Result<ledger_physical_asset::PhysicalAssetList> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_physical_asset::list_physical_assets(&conn, status_filter)
}

/// 创建实物资产（域公开写入口直调，旧壳 `createPhysicalAsset` 命令同一执
/// 行协议）：校验归一化（名称 trim 非空、估值必填且 > 0、币种必填且存在、
/// 估值日期拒绝未来、购买价与币种成对）在域内，码化错误原样上抛。
pub fn create_physical_asset(
    db: &DbState,
    input: ledger_physical_asset::PhysicalAssetInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::create_physical_asset(conn, input, &mut || {})
    })
}

/// 编辑实物资产档案（域公开写入口直调，旧壳 `updatePhysicalAsset` 命令同
/// 一执行协议）：全量替换名称与购买信息；估值不在入参内（结构性排除，
/// ADR-0064 只追加口径）。写连接是成对建连的写侧。
pub fn update_physical_asset(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::update_physical_asset(conn, id, input, &mut || {})
    })
}

/// 追加估值历史行（域公开写入口直调，旧壳 `updatePhysicalAssetValuation`
/// 命令同一执行协议）：只追加不改写，当前值由读口径裁决为最新一条。
pub fn update_physical_asset_valuation(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetValuationInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::update_physical_asset_valuation(conn, id, input, &mut || {})
    })
}

/// 处置实物资产（域公开写入口直调，旧壳 `disposePhysicalAsset` 命令同一
/// 执行协议）：处置日期必填、处置价与币种成对可选纯记录（不进任何金额口
/// 径）；处置 = 状态标记非删除，档案保留可经已处置筛选回看。
pub fn dispose_physical_asset(
    db: &DbState,
    id: &str,
    input: ledger_physical_asset::PhysicalAssetDisposeInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::dispose_physical_asset(conn, id, input, &mut || {})
    })
}

/// 软删除实物资产（域公开写入口直调，旧壳 `deletePhysicalAsset` 命令同一
/// 执行协议）：`is_deleted=1` 保留库行与估值历史，不进清单与合计。
pub fn delete_physical_asset(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_physical_asset::delete_physical_asset(conn, id, &mut || {})
    })
}
