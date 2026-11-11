//! 物品视图读写接缝（#2044）：物品清单 / 单件成本与写路径（创建 / 修改 /
//! 处置 / 软删）直调 `ledger-item` 域入口——与旧壳 list_items / create_item
//! 等命令消费的同一域函数，不经 IPC。物品写路径在域内自持事务（溯源守卫
//! / 折算 / op 产出），本壳零信号副作用——写成功后由视图广播交易失效信号。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// ---------------------------------------------------------------------------
// 物品视图读写接缝（#2044）：物品清单 / 单件成本与写路径（创建 / 修改 / 处置 /
// 软删）直调 `ledger-item` 域入口——与旧壳 list_items / create_item 等命令消费
// 费的同一域函数，不经 IPC。物品写路径在域内自持事务（溯源守卫 / 折算 / op
// 产出），本接缝零信号副作用——写成功后由视图广播 [`crate::signals`] 交易
// 失效信号（旧壳 ledger:changed 粗粒度语义的壳内对应物：交易列表来源列展示
// 物品名 / 处置态，物品变化经列表重读自然覆盖；广播接线删除即红钉在
// headless 名字改后来源列重读旅程）。

/// 关联购买交易候选页大小（旧壳 listTransactions limit 100 同值）。
const ITEM_CANDIDATE_LIMIT: usize = 100;

/// 清单物品行集（旧壳 `list_items` 命令同一执行协议）：每件附已用天数与每天
/// 成本快照（`item::cost` 接缝在域内计算，本壳只消费不另立口径；目标日在用
/// = 今天，已处置 = 处置日）。读连接是成对建连的只读侧（ADR-0117）。
pub fn item_rows(db: &DbState) -> Result<Vec<ledger_item::ItemWithDailyCost>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_item::list_items(&conn)
}

/// 按参考日重算单件物品每天成本（旧壳 `calculate_item_cost` 命令同一执行协
/// 议）：缺省 = 板用于清单同一口径（在用 → 今天 / 已处置 → 处置日），提供参
/// 考日即覆盖目标日（口径全在 `item::cost` 接缝单点）。读连接是成对建连的
/// 只读侧（ADR-0117）。
pub fn item_cost(
    db: &DbState,
    id: &str,
    reference_date: Option<&str>,
) -> Result<ledger_item::ItemDailyCost> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_item::calculate_item_cost(&conn, id, reference_date)
}

/// 创建物品（域公开写入口直调，旧壳 `create_item` 命令同一执行协议）：溯源守
/// 卫（创建必带购买交易指针）/ 带出 / 折算 / op 产出在域内单正文。写连接是
/// 成对建连的写侧。
pub fn create_item(db: &DbState, input: ledger_item::ItemInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_item::create_item(conn, input, &mut || {}))
}

/// 按 id 修改物品（域公开写入口直调，旧壳 `update_item` 命令同一执行协议）：
/// 换关带出 / 不可达状态守卫在域内。写连接是成对建连的写侧，写经统一写入口
/// 直锁形态（提交点置脏，issue #2087）。
pub fn update_item(db: &DbState, id: &str, input: ledger_item::ItemInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_item::update_item(conn, id, input, &mut || {})
    })
}

/// 处置物品（域公开写入口直调，旧壳 `dispose_item` 命令同一执行协议）：日期
/// / 残值守卫在域内；已处置再次处置 = 修正处置信息。写连接是成对建连的写侧。
pub fn dispose_item(db: &DbState, id: &str, input: ledger_item::ItemDisposeInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_item::dispose_item(conn, id, input, &mut || {})
    })
}

/// 软删物品（域公开写入口直调，旧壳 `delete_item` 命令同一执行协议）。写连
/// 接是成对建连的写侧。
pub fn delete_item(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_item::delete_item(conn, id, &mut || {}))
}

/// 关联购买交易候选（旧壳 ItemsView onMounted `listTransactions({ kinds:
/// ["expense"], limit: 100 })` 同口径）：支出交易倒序最近 100 笔，编辑弹窗
/// 换关下拉的数据源（更早的不在候选内，旧壳 MVP 取舍同口径；加载失败不阻断
/// 编辑弹窗，视图层容错）。读连接是成对建连的只读侧（ADR-0117）。
pub fn item_candidate_transactions(db: &DbState) -> Result<Vec<ledger_transaction::Transaction>> {
    let conn = conn_lock(&db.read_conn)?;
    let result = ledger_transaction::list_transactions(
        &conn,
        &ledger_transaction::TransactionListFilter {
            page: Some(1),
            page_size: Some(ITEM_CANDIDATE_LIMIT),
            kinds: Some(vec![ledger_transaction::TransactionKind::Expense]),
            ..Default::default()
        },
    )?;
    Ok(result.items)
}
