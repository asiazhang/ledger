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
use super::model::{
    ConvertFields, Transaction, TransactionListFilter, TransactionListResult, TransactionSource,
    TransactionSourceKind, TransactionSourceStatus,
};
use crate::db::query::{query_all, query_one};
use crate::error::{AppError, Result};
use crate::investment;
use crate::item;
use crate::policy;

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
pub(crate) fn register_plan_source_resolver(resolver: PlanSourceResolver) {
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

pub use get_transaction_internal as get_transaction;
pub use list_transactions_internal as list_transactions;

/// 按页填充来源列（spec #704，词汇表「来源列」来源判定优先级：保单直挂 >
/// 计划反查 > 物品反查 > 标的反查），逐级只对尚无来源的行填充，不做逐行 N+1：
/// ① 保单直挂（issue #706）：保单 id 已在行内（PolicyReference），去重后一次
///    批量反查（`policy::source_display_by_ids`）。双挂场景在此天然优先——
///    订阅期次执行时把协议上的保单引用复制进流水，自动保费流水同时有
///    「订阅协议 + 保单」两条线索，来源列显示保单（更具体的档案）。
/// ② 计划反查（issue #707）：按生成交易 id 批量反查期次 → 计划——经计划来源
///    解析接缝（#1090，实现由定时计划域启动时装入），展示名 =
///    计划名（备注，可空由前端按类型名兑底），已取消计划携带状态标注。
/// ③ 物品反查（issue #708）：按溯源指针批量反查物品表
///    （`item::source_display_by_transaction_ids`），展示名 = 物品名，已处置
///    物品携带状态标注（物品列表仍在册、跳转不落空）。期次交易被建物品时
///    计划优先（计划是流水发起方，物品是购买档案）。
/// ④ 标的反查（issue #709）：按生成交易 id 批量反查证券交易记录 → 标的
///    （`investment::source_display_by_transaction_ids`；transaction_id 为主键，
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
        let refs = policy::source_display_by_ids(conn, &policy_ids)?;
        let by_id: HashMap<&str, &policy::PolicySourceDisplay> =
            refs.iter().map(|r| (r.id.as_str(), r)).collect();
        for txn in items.iter_mut() {
            let Some(pid) = txn.policy_id.as_deref() else {
                continue;
            };
            let Some(reference) = by_id.get(pid) else {
                // 引用完整性由外键（ON DELETE RESTRICT）保证；缺行属防御性跳过，
                // 不虚构展示名也不中断整页读取。
                continue;
            };
            txn.source = Some(TransactionSource {
                kind: TransactionSourceKind::Policy,
                entity_id: pid.to_string(),
                display_name: reference.product_name.clone(),
                status: reference
                    .is_deleted
                    .then_some(TransactionSourceStatus::Deleted),
            });
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

    // ③ 物品反查（仅对保单/计划均未命中的行；溯源唯一保证一交易至多一行）
    let item_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !item_txn_ids.is_empty() {
        let rows = item::source_display_by_transaction_ids(conn, &item_txn_ids)?;
        let by_txn: HashMap<&str, &item::ItemSourceDisplay> = rows
            .iter()
            .filter_map(|r| {
                r.purchase_transaction_id
                    .as_deref()
                    .map(|txn_id| (txn_id, r))
            })
            .collect();
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            let Some(row) = by_txn.get(txn.id.as_str()) else {
                continue;
            };
            txn.source = Some(TransactionSource {
                kind: TransactionSourceKind::Item,
                entity_id: row.id.clone(),
                // 展示名 = 物品名；已处置物品携带状态标注（列表仍在册，可点击由前端裁决）。
                display_name: row.name.clone(),
                status: row.is_disposed.then_some(TransactionSourceStatus::Disposed),
            });
        }
    }

    // ④ 标的反查（仅对保单/计划/物品均未命中的行；证券交易记录 transaction_id
    //    为主键，一交易至多一行）
    let instrument_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !instrument_txn_ids.is_empty() {
        let rows = investment::source_display_by_transaction_ids(conn, &instrument_txn_ids)?;
        let by_txn: HashMap<&str, &investment::InstrumentSourceDisplay> = rows
            .iter()
            .map(|r| (r.transaction_id.as_str(), r))
            .collect();
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            let Some(row) = by_txn.get(txn.id.as_str()) else {
                continue;
            };
            txn.source = Some(TransactionSource {
                kind: TransactionSourceKind::Instrument,
                entity_id: row.instrument_id.clone(),
                // 展示名 = 代码 + 名称空格连接（随走势页签标签惯例），无名称退化为
                // 裸代码；标的字典无软删，恒无状态标注（清仓标的同样可达）。
                display_name: row.display_label(),
                status: None,
            });
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
    let rows = investment::convert_fields_by_transaction_ids(conn, &convert_ids)?;
    if rows.is_empty() {
        return Ok(());
    }
    let by_txn: HashMap<&str, &ConvertFields> = rows
        .iter()
        .map(|(id, fields)| (id.as_str(), fields))
        .collect();
    for txn in items.iter_mut() {
        if let Some(fields) = by_txn.get(txn.id.as_str()) {
            txn.convert = Some((*fields).clone());
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
