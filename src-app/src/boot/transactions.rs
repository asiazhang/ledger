//! 交易读/写接缝（壳侧编排首屏与删除，立桩 #2017 / #2022 / #2037 筛选 /
//! #2023 搜索）：交易列表页（服务端 offset 分页，TxListQuery 载荷）与
//! 统一模糊搜索页（TxSearchQuery 载荷，#2073 自散参收敛）直调
//! `ledger-transaction` 域读路径；删除经壳层统一写入口。

use crate::assembly::PAGE_SIZE;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_transaction::{TransactionListResult, TransactionSearchResult};

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 交易列表页查询载荷（issue #2037 筛选读接缝）：筛选会话态快照的读路径投影。
/// 默认构造 = 无筛选但显式携带四通用 kind（主列表排除投资 kind 的读接缝单点，
/// ADR-0135——本壳立桩期未接筛选维度的缺口由此闭合）。
///
/// 本型即 total 会话复用的**筛选签名**（#2011）：全字段相等（不含页码——
/// 页码不进本型）= 同一筛选签名，翻页可复用已算得的 total。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxListQuery {
    /// 期间快照（「全部」= `None`）。
    pub range: Option<crate::period::PeriodRange>,
    /// 类型集合（显式携带，含默认态；空集合视为未携带不过滤）。
    pub kinds: Vec<ledger_transaction::TransactionKind>,
    /// 涉及账户（转出 ∪ 转入 ∪ 出资三端命中，issue #937）。
    pub involving_account_id: Option<String>,
    /// 商户（含软删商户历史交易口径，issue #191）。
    pub merchant_id: Option<String>,
    /// 分类精确过滤（issue #377）。
    pub category_id: Option<String>,
    /// 仅无分类（issue #377 三态之一）。
    pub uncategorized_only: bool,
    /// 隐藏投资相关流水（#2062 / ADR-0136 视图偏好在读路径上的载荷，不是筛
    /// 选维度）：`true` 时行集排除转出 / 转入 / 出资三端任一端为投资类型账户
    /// 的行（域读路径三端 SQL 直传，按账户不按分类）；筛选会话态不携带，
    /// 由偏好（[`crate::tx_view_preferences`]）在装载时机写入。默认 `false`
    /// = 行为不变（域契约缺省不携带，只增）。
    pub hide_investment_related: bool,
}

impl Default for TxListQuery {
    fn default() -> Self {
        Self {
            range: None,
            kinds: crate::tx_filter::MAIN_LIST_KINDS.to_vec(),
            involving_account_id: None,
            merchant_id: None,
            category_id: None,
            uncategorized_only: false,
            hide_investment_related: false,
        }
    }
}

/// 读取账本库交易列表页（读路径直调域服务，issue #2018 立桩靶标；issue #2021
/// 补日期区间维度；issue #2037 补全筛选维度与默认 kind 收窄）。
///
/// 经核心交易域读路径 [`ledger_transaction::list_transactions`] 的服务端 offset
/// 分页（ADR-0008 语义：页码从 1 起、`total` 与行集共用同一 WHERE 子句、排序
/// date DESC, created_at DESC, id DESC），每页 [`PAGE_SIZE`] 行（100 行/页，
/// 旧壳 50 行翻倍验证原生余量）；`query.range` 为期间快照（含边界，自然周期
/// 换算产物见 [`crate::period`]），None = 「全部」无过滤；其余维度与类型集合
/// 按域契约直传（维度间 AND、类型维度内取或）。来源列等读时投影接缝已由
/// [`crate::wiring`] 接线。读连接是成对建连的只读侧（ADR-0117：读路径不进
/// 写者闸门）。
///
/// `skip_total`（issue #2011）直传域读路径的全量 COUNT 跳过开关：同筛选签名
/// 会话复用 total 的翻页读置 `true`（调用方用缓存 total 回填），本函数不持有
/// 会话态——签名的持有与失效（写后广播清零）在消费方视图；缺省 `false` =
/// COUNT 恒跑、total 恒返回（既有契约）。
pub fn transaction_page(
    db: &DbState,
    page: u32,
    query: &TxListQuery,
    skip_total: bool,
) -> Result<TransactionListResult> {
    let filter = ledger_transaction::TransactionListFilter {
        page: Some(page.max(1) as usize),
        page_size: Some(PAGE_SIZE),
        from: query.range.as_ref().map(|r| r.from.clone()),
        to: query.range.as_ref().map(|r| r.to.clone()),
        involving_account_id: query.involving_account_id.clone(),
        merchant_id: query.merchant_id.clone(),
        category_id: query.category_id.clone(),
        uncategorized_only: query.uncategorized_only.then_some(true),
        kinds: (!query.kinds.is_empty()).then(|| query.kinds.clone()),
        hide_investment_related: query.hide_investment_related.then_some(true),
        skip_total,
        ..Default::default()
    };
    let conn = conn_lock(&db.read_conn)?;
    ledger_transaction::list_transactions(&conn, &filter)
}

/// 交易搜索页查询载荷（#2073 自 6 散参收敛，与 [`TxListQuery`] 同构对称——
/// 搜索读接缝的全部输入一个载体）：搜索面维度裁决见 [`transaction_search`]
/// doc。默认构造 = 空关键词无筛选（域语义 = 空结果）。
#[derive(Clone, Debug, Default)]
pub struct TxSearchQuery {
    /// 关键词（统一模糊搜索语义，ADR-0027；空 + 有筛选 = 仅筛选查询，空 +
    /// 无筛选 = 域空结果语义）。
    pub query: String,
    /// 金额区间下界（整数分，本位币口径，issue #2037；`None` = 无下界）。
    pub amount_min_cents: Option<i64>,
    /// 金额区间上界（整数分，本位币口径；`None` = 无上界）。
    pub amount_max_cents: Option<i64>,
    /// 期间快照（含边界，搜索 × 期间裁决：期间对搜索命中面生效，旧壳搜索页
    /// 同口径；`None` = 全部）。
    pub range: Option<crate::period::PeriodRange>,
}

/// 搜索账本库交易（读路径直调域服务，issue #2023）：经核心交易域统一模糊
/// 搜索 [`ledger_transaction::search_transactions`]——词条间 AND、字段间 OR 的
/// 原文命中（备注原文子串 ∨ 账户名 ∨ 商户名 ∨ 购买项名称，LIKE 折叠仅 ASCII），
/// 服务端 offset 分页（命中总数与页行集共用同一 WHERE，排序 date DESC）。与
/// 旧壳 `search_transactions` 命令消费同一域函数——同一账本库同结果（#2023
/// AC1）；issue #2037 起接金额区间（整数分，本位币口径）与期间快照（搜索 ×
/// 期间裁决：期间对搜索命中面生效，旧壳搜索页同口径）；账户/商户/分类/类型
/// 维度不在搜索面（域读路径不支持，裁决定型为规格）。页大小沿 [`PAGE_SIZE`]
/// （分页页径与列表态一致）。读连接是成对建连的只读侧（ADR-0117）。
pub fn transaction_search(
    db: &DbState,
    page: u32,
    filters: &TxSearchQuery,
) -> Result<TransactionSearchResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_transaction::search_transactions(
        &conn,
        &filters.query,
        page.max(1) as usize,
        PAGE_SIZE,
        filters.amount_min_cents,
        filters.amount_max_cents,
        filters.range.as_ref().map(|r| r.from.as_str()),
        filters.range.as_ref().map(|r| r.to.as_str()),
    )
}

/// 删除交易（域公开写入口直调，issue #2022 写路径立桩靶标；不经 IPC）：
/// 经核心交易域行为层删除编排入口 [`ledger_transaction::delete`]——软删编排
/// （持仓副作用回退 + 级联 + 主行软删）同处一个事务，余额重算由已接线的余额
/// 刷新接缝（[`crate::wiring`]）在既有事务内执行。写经壳层统一写入口直锁
/// 形态（[`crate::shell_support::write_entry_locked`]，提交点置脏，#2087）。
/// 不存在的 id 返回码化 NotFound（并发已删的呈现面归视图层）。
pub fn delete_transaction(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_transaction::delete(conn, id))
}
