//! 商户字典管理读写接缝（#2052）：按名即建、创建 / 改名 / 软删与引用
//! 计数直调 `ledger-merchants` 域入口——商户域写语义零改动，唯一性
//! 守卫住域内单正文；按名即建同时是计划表单「输入即建」消费面。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 按名即建商户（域公开按名 find-or-create 直调，旧壳 `createMerchant`
/// 命令的重名车间同义——`create_merchant_by_name` 的精确匹配在用名复用 /
/// 未命中即建语义在域内单点，重名陈旧态竞态不再前端逐层兜底）。计划表单
///「输入即建」的自由文本提交路径消费面（旧壳 resolveMerchantRef 的 NSH
/// 等价路径 收口在装配单点）。写连接是成对建连的写侧。
pub fn create_merchant_by_name(db: &DbState, name: &str) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_merchants::create_merchant_by_name(conn, name)
    })
}

// ---------------------------------------------------------------------------
// 商户字典管理读写接缝（#2052）：旧壳 MerchantManager 消费的 createMerchant /
// updateMerchant / deleteMerchant / listMerchantTransactionCounts 命令的同一域
// 函数直调，不经 IPC。商户域写语义零改动（Out of Scope：只迁 UI），唯一性
// 守卫 / 软删 / 历史引用保留全在域内单正文；读接缝复用 [`merchant_rows`]
// 全量投影不另立（清单在用 / 已删分区在装配层划分）。写成功后由视图广播
// [`crate::signals`] 交易失效信号（交易列表来源列展示商户名 / 表单商户源 /
// 筛选商户下拉——旧壳 ledger:changed 参考失效信号的壳内等价物；引用计数
// 读失败静默保留旧值是旧壳 listMerchantTransactionCounts 消费面同口径）。
// ---------------------------------------------------------------------------

/// 创建商户（域公开写入口直调，旧壳 `createMerchant` 命令同一执行协议）：
/// 在用重名被域守卫码化拒绝（`merchant.already-exists`），零落库。返回新商户
/// id。写连接是成对建连的写侧。
pub fn create_merchant(db: &DbState, input: ledger_merchants::MerchantInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_merchants::create_merchant(conn, input))
}

/// 改名商户（域公开写入口直调，旧壳 `updateMerchant` 命令同一执行协议）：
/// trim 归一 / 空名与在用撞名由域守卫码化拒绝，改名即时生效（交易以
/// merchant_id 引用，不回刷历史交易行——ADR-0028）。写连接是成对建连的写侧。
pub fn update_merchant(
    db: &DbState,
    id: &str,
    input: ledger_merchants::MerchantUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_merchants::update_merchant(conn, id, input)
    })
}

/// 软删商户（域公开写入口直调，旧壳 `deleteMerchant` 命令同一执行协议）：
/// 历史交易引用照常显示（软删商户不可再被新交易选择）；不存在的 id 报码化
/// NotFound。写连接是成对建连的写侧。
pub fn delete_merchant(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_merchants::delete_merchant(conn, id))
}

/// 读取商户关联交易计数（旧壳 `listMerchantTransactionCounts` 命令消费的同一
/// 域函数）：每商户被未删流水引用的毛笔数（含软删行、LEFT JOIN 全字典、缺失
/// 补 0 在域内），实时推导不落库。读连接是成对建连的只读侧。
pub fn merchant_transaction_counts(
    db: &DbState,
) -> Result<Vec<ledger_merchants::MerchantTransactionCount>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_merchants::transaction_counts(&conn)
}

// 储蓄目标视图读写接缝（#2056）：蓄水进度读直调域读路径（旧壳
// savingsGoalProgress 命令消费的同一域函数，#2041 账户视图绑集读同点消费），目标写路径
// （创建 / 编辑 / 归档 / 取消归档 / 删除）直调域公开写入口——与旧壳
// create/update/archive/unarchive/delete_savings_goal 命令同一执行协议，不经
// IPC。生命周期守卫（余额非零拒绝删除、归档不删数据）住域单正文，本壳不自
// 建守卫语义（spec #2056：生命周期守卫语义以域为唯一真源）。删除级联软删
