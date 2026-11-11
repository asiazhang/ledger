//! 交易表单族读写接缝（#2034）：表单保单选项面与基础收支写路径（创建 /
//! 修改）+ 参考数据行集（保单 / 报告日期边界 / 币种字典 / 在用分类 /
//! 可见账户 / 含软删商户 / 本位币基准）直调各域入口——被交易表单与
//! 多视图共享的高扇入组（消费面 18+ 文件），不经 IPC。

use ledger_categories::Category;
use ledger_currencies::Currency;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 本位币基准（旧壳 `getBaseCurrency` 命令同一域来源）：「本位币折算」行
/// 的符号与后缀读账本本位币，读取失败由视图回退缺省 CNY（旧壳同口径软容
/// 错）。读连接是成对建连的只读侧（ADR-0117）。
pub fn base_currency_code(db: &DbState) -> Result<String> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_currencies::current_base_currency(&conn)
}

// 备份最小入口读写接缝（#2038）：手动备份与账本指纹直调 `ledger-backup` 域
// 入口——与旧壳 `create_backup` / `get_book_fingerprint` 命令消费的同一域函数，
// ---------------------------------------------------------------------------
// 交易表单族读写接缝（#2034）：表单保单选项面（未删除保单）与基础收支写路径
// （创建 / 修改）直调域入口——与旧壳 create_transaction / update_transaction
// 及保单 store 消费的同一域函数，不经 IPC。商户 / 账户 / 分类选项面消费列表
// 装载的既有参考数据行集（boot::merchant_rows 等全量投影，在用口径在表单源
// 处过滤），不另立第二份读接缝。写成功后由视图广播 [`crate::signals`] 交易
// 失效信号（列表重读；即建商户证据经列表重读的商户行集刷新自然覆盖，旧壳
// write_entry 的参考失效信号在壳内等价物）。
// ---------------------------------------------------------------------------

/// 创建交易（域公开写入口直调，旧壳 `create_transaction` 命令同一执行协议）：
/// 创建编排入口 [`ledger_transaction::create`]——守卫 / 折算 / 落库 / op 产出
/// 在域内单正文（自持事务，中途失败整体回滚），余额重算由已接线的余额刷新
/// 接缝在既有事务内执行。写经壳层统一写入口直锁形态（提交点置脏，#2087）。
/// 返回写入证据（id + 是否即建商户）。写连接是成对建连的写侧。
pub fn create_transaction(
    db: &DbState,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_transaction::TransactionWrite> {
    write_entry_locked(db, |conn| ledger_transaction::create(conn, input))
}

/// 按 id 全字段替换交易（域公开写入口直调，旧壳 `update_transaction` 命令
/// 同一执行协议；与 HTTP `PUT /api/v1/transactions/{id}` 同一行为层权威）：
/// 修改编排入口 [`ledger_transaction::update`]。返回「是否即建商户」证据。
/// 不存在的 id 报码化 NotFound。写经壳层统一写入口直锁形态（#2087）。
pub fn update_transaction(
    db: &DbState,
    id: &str,
    input: ledger_transaction::TransactionInput,
) -> Result<ledger_infra::signals::WriteEvidence> {
    write_entry_locked(db, |conn| ledger_transaction::update(conn, id, input))
}

/// 读取未删除保单行集（表单保单下拉的数据源面，#2034）：旧壳保单 store 消费
/// `list_policies` 口径（软删不可再被新选择）。读连接是成对建连的只读侧。
pub fn policy_rows(db: &DbState) -> Result<Vec<ledger_policy::Policy>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::list_policies(&conn)
}

/// 读取账本库数据期间边界（issue #2021 / ADR-0141 期间步进的钳制输入）：
/// 报表域日期极值读路径 [`ledger_reports::query_report_date_range`]——旧壳
/// report_date_range 消费的同一域函数；双 None = 空库。随每页读一并刷新
/// （单行聚合查询，成本可忽略；写路径落地后天然跟随数据外扩/收窄，
/// 无需独立失效接线）。
pub fn report_date_range(db: &DbState) -> Result<ledger_reports::DateRange> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::query_report_date_range(&conn)
}

/// 读取账本库币种字典（金额展示的参考数据面，issue #2018）：装配层的币种
/// 展示参数（符号/小数位）来自本只读投影；随每页读一并刷新（字典行数小，
/// 改名/改号后的重读成本可忽略）。
pub fn currency_rows(db: &DbState) -> Result<Vec<Currency>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_currencies::list_currencies(&conn)
}

/// 读取在用分类行集（分类路径解析的参考数据面，issue #2019）：装配层的分类
/// 路径表来自本只读投影，**仅未删除**——旧壳路径函数消费在用分类行集
/// （reference store 口径），软删分类的历史交易引用在交易列表显示 `-`；随每页
/// 读一并刷新（分类行数小，改名/改父后的重读成本可忽略）。
pub fn category_rows(db: &DbState) -> Result<Vec<Category>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_categories::list_categories(&conn, false)
}

/// 读取用户可见账户行集（账户格与借贷派生的参考数据面，#2020）：装配层的账户
/// 表来自本只读投影，**在用且未隐藏**——旧壳 accountMap 消费 `list_accounts`
/// 口径（排除软删与隐藏；黑洞/隐藏账户不在表中 → 账户格占位「-」、借贷派生
/// 按缺失归一资金侧）；随每页读一并刷新（账户行数小，改名/改类型后的重读成本
/// 可忽略）。
pub fn account_rows(db: &DbState) -> Result<Vec<ledger_accounts::Account>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_accounts::list_accounts(&conn)
}

/// 读取商户行集（商户格的参考数据面，#2020）：装配层的商户表来自本只读投影，
/// **含软删全量**——旧壳 merchantMap 消费 `includeDeleted: true` 口径（在用 +
/// 软删，历史交易显示共用，issue #191）；随每页读一并刷新（商户行数小，改名后
/// 的重读成本可忽略）。
pub fn merchant_rows(db: &DbState) -> Result<Vec<ledger_merchants::Merchant>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_merchants::list_merchants(&conn, true)
}
