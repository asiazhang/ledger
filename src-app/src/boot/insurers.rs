//! 保司管理读写接缝（#2051）：保司字典行集读与写路径（按名即建 / 创建 /
//! 改名 / 软删）直调 `ledger-policy` 域入口——字典完整性守卫住域写入口
//! 单正文；保单表单「输入即建」路径同点消费。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 保司按名 find-or-create（域公开直调，旧壳 `createInsurer` 命令的重名兜底
/// 同义——`create_insurer_by_name` 的「输入名精确命中在用保司复用 / 未命中
/// 即建」语义在域内单点，重名陈旧态竞态不再前端逐层兜底；本壳保存时数据
/// 现读，无 store 缓存竞态面）。保单表单「输入即建」路径消费面。写连接是
/// 成对建连的写侧。
pub fn create_insurer_by_name(db: &DbState, name: &str) -> Result<String> {
    write_entry_locked(db, |conn| ledger_policy::create_insurer_by_name(conn, name))
}

// ---------------------------------------------------------------------------
// 保司管理读写接缝（#2051）：保司字典清单读与写路径（创建 / 改名 / 软删）
// 直调 `ledger-policy` 域入口——与旧壳 list_insurers / create_insurer /
// update_insurer / delete_insurer 命令消费的同一域函数，不经 IPC。字典完整
// 性守卫（在用行唯一 / 空名拒绝 / NotFound）住域写入口单正文，本壳不自建；
// 旧壳写入后经 ledger:changed 参考失效重拉的失效语义在壳内对应物 = 视图
// 写成功回调自拉重读（保司不挂交易，交易失效信号不涉）；读连接是只读侧。
// ---------------------------------------------------------------------------

/// 保司字典全量行集（旧壳 `list_insurers` 命令同一执行协议）：含已删全量
///（在用 + 软删，name/created_at 升序）；在用/已删双分区拆分与「显示已删」
/// 只读分位归装配层纯面。读连接是成对建连的只读侧（ADR-0117）。
pub fn insurer_rows(db: &DbState, include_deleted: bool) -> Result<Vec<ledger_policy::Insurer>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::list_insurers(&conn, include_deleted)
}

/// 创建保司（域公开写入口直调，旧壳 `create_insurer` 命令同一执行协议）：
/// trim 归一、空名拒绝与在用行同名码化拒绝（`insurer.already-exists`）在
/// 域内单正文（自持事务）。返回新保司 id。写连接是成对建连的写侧。
pub fn create_insurer(db: &DbState, input: ledger_policy::InsurerInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_policy::create_insurer(conn, input))
}

/// 改名保司（域公开写入口直调，旧壳 `update_insurer` 命令同一执行协议）：
/// 字段省略保持原值、空名拒绝与撞在用同名码化拒绝在域内单正文；不存在
///（或已软删）id → 码化 NotFound。写连接是成对建连的写侧。
pub fn update_insurer(
    db: &DbState,
    id: &str,
    input: ledger_policy::InsurerUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| ledger_policy::update_insurer(conn, id, input))
}

/// 软删除保司（域公开写入口直调，旧壳 `delete_insurer` 命令同一执行协议）：
/// 软删不占名字、存量保单引用照常显示（软删不可再被新选择由消费方校验）。
/// 不存在的 id → 码化 NotFound。写连接是成对建连的写侧。
pub fn delete_insurer(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_policy::delete_insurer(conn, id))
}
