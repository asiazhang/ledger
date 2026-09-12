//! 交易读取权威（列表过滤/排序/分页与单笔读取）。
//!
//! 来源列的计划反查段（②）自 #1090 起经接缝反转：本模块只定义注册点
//! （[`register_plan_source_resolver`]）与消费单点（[`resolve_plan_sources`]），
//! 实现由定时计划域提供（`scheduled_transactions::source::install_plan_source_hook`）、
//! 壳层启动时接线——本域对定时计划域零直接依赖（域间禁边，`scripts/check-structure.ts`）。
//! 计划反查行的公开再导出随之消亡，私有性由 compile_fail 负向用例钉死：
//!
//! ```compile_fail
//! use tauri_app_lib::scheduled_transactions::source_display_by_transaction_ids;
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;

use super::amount::TransactionKind;
use super::investment_seam::{resolve_convert_fields, resolve_instrument_sources};
use super::model::{
    ConvertFields, Transaction, TransactionListFilter, TransactionListResult, TransactionSource,
};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 计划来源解析接缝（issue #1090 / spec #1086 形态推广）
// ---------------------------------------------------------------------------

/// 计划来源解析钩子签名：按页收集的生成交易 id → 交易 id 到来源展示
/// （[`TransactionSource`]，本域自有模型）的映射。实现侧（定时计划域）负责把
/// `PlanSourceDisplay` 映射为本域来源模型（kind 三枚举 → 来源类型三枚举、
/// `cancelled` 状态 → Cancelled 标注，spec #704 口径零变化）。
type PlanSourceResolver = fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

/// 计划来源解析器的进程级单例（登记点反转的承接面）：先装者优先、重复注册零动作。
static PLAN_SOURCE_RESOLVER: OnceLock<PlanSourceResolver> = OnceLock::new();

/// 注册计划来源解析实现（幂等：进程级一次，重复注册保留首次实现）。
///
/// 调用点在壳层启动接线与测试建库单点（`test_support::open`、BDD world），
/// 与生产同形；实现由定时计划域提供
/// （`scheduled_transactions::source::install_plan_source_hook`），业务代码不直接调用。
pub fn register_plan_source_resolver(resolver: PlanSourceResolver) {
    let _ = PLAN_SOURCE_RESOLVER.set(resolver);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷，
/// 不该被读路径静默吞掉。
fn plan_source_resolver_missing_error() -> AppError {
    AppError::coded(
        "transaction.plan-source-resolver-unregistered",
        "计划来源解析器未注册：来源列的计划反查段被跳过（壳层启动接线缺失）",
    )
}

/// 计划反查段的接缝消费点（私有）：委派给注册的实现。未注册即接线缺失，
/// 码化错误上抛（列表读取显式失败，不静默丢来源列）。
fn resolve_plan_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = PLAN_SOURCE_RESOLVER
        .get()
        .ok_or_else(plan_source_resolver_missing_error)?;
    resolver(conn, transaction_ids)
}

// ---------------------------------------------------------------------------
// 来源列①保单 / ③物品反查接缝（issue #1092，与计划反查同族同形）
// ---------------------------------------------------------------------------

/// 保单直挂反查钩子：按行内保单 id 批量反查保单档案，实现侧（保单域）把自有
/// 展示字段映射为本域来源模型（kind = Policy、展示名 = 产品名、软删 → Deleted
/// 标注，spec #704 口径零变化）。
pub type PolicySourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

/// 物品反查钩子：按溯源指针（生成交易 id）批量反查在用物品，实现侧（物品域）
/// 映射为来源模型（kind = Item、展示名 = 物品名、已处置 → Disposed 标注）。
pub type ItemSourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

static POLICY_SOURCE_RESOLVER: OnceLock<PolicySourceResolver> = OnceLock::new();
static ITEM_SOURCE_RESOLVER: OnceLock<ItemSourceResolver> = OnceLock::new();

/// 注册保单直挂反查实现（幂等：进程级一次，重复注册保留首次实现）。调用点在
/// 保单域 `install_source_hook`，壳层启动接线，业务代码不直接调用。
pub fn register_policy_source_resolver(resolver: PolicySourceResolver) {
    let _ = POLICY_SOURCE_RESOLVER.set(resolver);
}

/// 注册物品反查实现（幂等同上）。调用点在物品域 `install_source_hook`。
pub fn register_item_source_resolver(resolver: ItemSourceResolver) {
    let _ = ITEM_SOURCE_RESOLVER.set(resolver);
}

/// 保单反查委派：未注册即接线缺失，码化错误上抛（列表读取显式失败）。
fn resolve_policy_sources(
    conn: &Connection,
    policy_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = POLICY_SOURCE_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.policy-source-resolver-unregistered",
            "保单来源反查器未注册：来源列的保单直挂段被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, policy_ids)
}

/// 物品反查委派：未注册即接线缺失，码化错误上抛（列表读取显式失败）。
fn resolve_item_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = ITEM_SOURCE_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.item-source-resolver-unregistered",
            "物品来源反查器未注册：来源列的物品反查段被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, transaction_ids)
}

pub use get_transaction_internal as get_transaction;
pub use list_transactions_internal as list_transactions;

/// 按页填充来源列（spec #704，词汇表「来源列」来源判定优先级：保单直挂 >
/// 计划反查 > 物品反查 > 标的反查），逐级只对尚无来源的行填充，不做逐行 N+1：
/// ① 保单直挂（issue #706）：保单 id 已在行内（PolicyReference），去重后一次
///    批量反查——经保单直挂反查接缝（#1092，实现由保单域装入）。双挂场景在此天然优先——
///    订阅期次执行时把协议上的保单引用复制进流水，自动保费流水同时有
///    「订阅协议 + 保单」两条线索，来源列显示保单（更具体的档案）。
/// ② 计划反查（issue #707）：按生成交易 id 批量反查期次 → 计划——经计划来源
///    解析接缝（#1090，实现由定时计划域启动时装入），展示名 =
///    计划名（备注，可空由前端按类型名兑底），已取消计划携带状态标注。
/// ③ 物品反查（issue #708）：按溯源指针批量反查物品表——经物品反查接缝
///    （#1092，实现由物品域装入），展示名 = 物品名，已处置
///    物品携带状态标注（物品列表仍在册、跳转不落空）。期次交易被建物品时
///    计划优先（计划是流水发起方，物品是购买档案）。
/// ④ 标的反查（issue #709）：按生成交易 id 批量反查证券交易记录 → 标的——经
///    交易×投资接缝（#1092，实现由投资域装入；transaction_id 为主键，
///    一交易至多一行），展示名 = 代码 + 名称空格连接（随走势页签标签惯例，
///    无名称退化为裸代码）。标的字典无软删（被流水引用的标的不可删），来源
///    恒命中、无状态标注——清仓标的同样可达（走势不依赖持仓）。
///
/// 零迁移：来源是读时推导，不落库；无来源交易（手动录入/AI 导入）原样为 `None`。
pub(super) fn attach_sources(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    // ① 保单直挂（保单 id 已在行内）
    let mut policy_ids: Vec<String> = Vec::new();
    for txn in items.iter() {
        if let Some(pid) = txn.policy_id.as_ref()
            && !policy_ids.contains(pid)
        {
            policy_ids.push(pid.clone());
        }
    }
    if !policy_ids.is_empty() {
        // 经保单直挂反查接缝（#1092）：注册点在本域，实现由保单域启动时装入。
        let by_id = resolve_policy_sources(conn, &policy_ids)?;
        for txn in items.iter_mut() {
            let Some(pid) = txn.policy_id.as_deref() else {
                continue;
            };
            let Some(source) = by_id.get(pid) else {
                // 引用完整性由外键（ON DELETE RESTRICT）保证；缺行属防御性跳过，
                // 不虚构展示名也不中断整页读取。
                continue;
            };
            txn.source = Some(source.clone());
        }
    }

    // ② 计划反查（仅对保单未命中的行；期次唯一索引保证一交易至多一行）——
    // 经计划来源解析接缝（#1090）：注册点在本域，实现由定时计划域启动时装入，
    // kind/状态/展示名映射口径零变化（spec #704）。
    let plan_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !plan_txn_ids.is_empty() {
        let by_txn = resolve_plan_sources(conn, &plan_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }

    // ③ 物品反查（仅对保单/计划均未命中的行；溯源唯一保证一交易至多一行）——
    // 经物品反查接缝（#1092）：注册点在本域，实现由物品域启动时装入。
    let item_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !item_txn_ids.is_empty() {
        let by_txn = resolve_item_sources(conn, &item_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }

    // ④ 标的反查（仅对保单/计划/物品均未命中的行；证券交易记录 transaction_id
    //    为主键，一交易至多一行）——经交易×投资接缝（#1092，[`super::investment_seam`]）：
    //    注册点在本域，实现由投资域启动时装入。
    let instrument_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !instrument_txn_ids.is_empty() {
        let by_txn = resolve_instrument_sources(conn, &instrument_txn_ids)?;
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            if let Some(source) = by_txn.get(txn.id.as_str()) {
                txn.source = Some(source.clone());
            }
        }
    }
    Ok(())
}

/// 按页填充转换两腿扩展（ADR-0099 / 词汇表「基金转换（Conversion）」）：仅
/// `kind = convert` 的行命中（`security_transactions` 的 convert 行以
/// `transaction_id` 为主键、一交易至多一行），逐页一次批量查询、不逐行 N+1。
///
/// 扩展查询归投资域（`security_transactions` 表的所属域，与来源列标的反查同款
/// 域访问器，ADR-0056）：行为层不自己读投资扩展表。行金额锚点是结转成本（不是确认单
/// 金额），列表金额列展示转出金额须读本扩展（`out_amount_cents`）；非转换行与未命中行
/// 保持 `None`（与来源列同款的读时投影纪律：零库列、写路径不填充）。
pub(super) fn attach_convert_fields(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    let convert_ids: Vec<String> = items
        .iter()
        .filter(|t| t.kind == TransactionKind::Convert)
        .map(|t| t.id.clone())
        .collect();
    let by_txn: HashMap<String, ConvertFields> = resolve_convert_fields(conn, &convert_ids)?;
    for txn in items.iter_mut() {
        if let Some(fields) = by_txn.get(txn.id.as_str()) {
            txn.convert = Some(fields.clone());
        }
    }
    Ok(())
}

pub fn list_transactions_internal(
    conn: &Connection,
    filter: &TransactionListFilter,
) -> Result<TransactionListResult> {
    // 过滤条件与 total/items 共用同一 WHERE 子句，保证 total 恒为"满足过滤条件的未删除交易总数"。
    let mut where_clause = String::from("WHERE is_deleted=0");
    let mut params: Vec<String> = Vec::new();
    if let Some(from) = filter.from.as_deref() {
        where_clause.push_str(" AND date >= ?");
        params.push(from.to_string());
    }
    if let Some(to) = filter.to.as_deref() {
        where_clause.push_str(" AND date <= ?");
        params.push(to.to_string());
    }
    if let Some(account_id) = filter.account_id.as_deref() {
        where_clause.push_str(" AND account_id = ?");
        params.push(account_id.to_string());
    }
    if let Some(account_id) = filter.involving_account_id.as_deref() {
        // 涉及账户三端（issue #937 / ADR-0096）：转出 ∪ 转入 ∪ 出资——按出资账户
        // 过滤命中它出资的 buy/sell；已发布两端语义不变（只增不改）。
        where_clause
            .push_str(" AND (account_id = ? OR to_account_id = ? OR funding_account_id = ?)");
        params.push(account_id.to_string());
        params.push(account_id.to_string());
        params.push(account_id.to_string());
    }
    if let Some(merchant_id) = filter.merchant_id.as_deref() {
        where_clause.push_str(" AND merchant_id = ?");
        params.push(merchant_id.to_string());
    }
    // 分类下钻两字段（issue #377）：精确匹配不含子分类；仅无分类命中 category_id IS NULL。
    // 与其他维度同规：AND 组合，互不隐含。
    if let Some(category_id) = filter.category_id.as_deref() {
        where_clause.push_str(" AND category_id = ?");
        params.push(category_id.to_string());
    }
    if filter.uncategorized_only == Some(true) {
        where_clause.push_str(" AND category_id IS NULL");
    }
    // 标的过滤（ADR-0107）：核心交易行不持标的信息，经投资域扩展表子查询命中——
    // 任一腿命中（instrument_id = 转出腿，to_instrument_id = convert 转入腿），
    // 与其它维度 AND 组合；total 与 items 共用同一 WHERE 子句，口径自动一致。
    if let Some(instrument_id) = filter.instrument_id.as_deref() {
        where_clause.push_str(
            " AND id IN (SELECT transaction_id FROM security_transactions \
             WHERE instrument_id = ? OR to_instrument_id = ?)",
        );
        params.push(instrument_id.to_string());
        params.push(instrument_id.to_string());
    }
    // 类型集合过滤（spec #1025 起为唯一类型维度，手动多选与下钻载荷共用）：kind IN (...)，
    // 维度内取或、与其余维度 AND 组合；单值亦经本参数（原单值 kind = ? 子句已随
    // 参数移除，BREAKING，见 CHANGELOG）。空集合视为未携带（不过滤），先例同
    // uncategorized_only=false。
    if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
        let placeholders = vec!["?"; kinds.len()].join(",");
        where_clause.push_str(&format!(" AND kind IN ({placeholders})"));
        kinds
            .iter()
            .for_each(|k| params.push(k.as_str().to_string()));
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM transactions {where_clause}"),
        rusqlite::params_from_iter(params.iter()),
        |r| r.get(0),
    )?;

    // 确定性排序：date DESC, created_at DESC, id DESC。
    // id 是最终 tiebreaker——`now_iso()` 为秒级精度，同一秒内写入的行 created_at 相同，
    // 不加 id 翻页会漂移（重复/遗漏）。
    let mut sql = format!(
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted,merchant_id,policy_id \
         FROM transactions {where_clause} ORDER BY date DESC, created_at DESC, id DESC"
    );
    // 分页路径优先：传 page_size 时按 offset 页码取当前页（小于 1 按 1 处理，
    // 与 InstrumentListFilter 先例一致；offset 用 saturating 运算防溢出）；
    // 否则 limit 路径取前 N 条（沿用 SQLite 原生语义：LIMIT 0 返回空、负值无上限）；
    // 两者都缺省时返回全部（total 恒返回）。
    if let Some(page_size) = filter.page_size {
        // 钳制到 SQLite 可接受的 64 位整数范围，防止极端输入（usize::MAX）产生
        // "datatype mismatch" 或 debug 构建 panic。
        let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
        let page = filter.page.unwrap_or(1).max(1);
        let offset = i64::try_from(page.saturating_sub(1).saturating_mul(page_size as usize))
            .unwrap_or(i64::MAX);
        sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
    } else if let Some(n) = filter.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    attach_sources(conn, &mut items)?;
    attach_convert_fields(conn, &mut items)?;
    Ok(TransactionListResult { items, total })
}

/// 按 `id` 读取未删除交易，供修改接口返回更新后的完整交易。不存在返回 `NotFound`。
pub fn get_transaction_internal(conn: &Connection, id: &str) -> Result<Transaction> {
    query_one::<Transaction, _>(
        conn,
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
         version,device_id,is_deleted,merchant_id,policy_id FROM transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
    })
}

#[cfg(test)]
mod resolver_tests {
    use super::*;

    #[test]
    fn 未注册错误_错误码可判读() {
        let err = plan_source_resolver_missing_error();
        assert_eq!(
            err.code(),
            Some("transaction.plan-source-resolver-unregistered")
        );
    }
}
