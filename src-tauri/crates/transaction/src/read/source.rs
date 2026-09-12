//! 来源列与转换两腿的读时投影（读路径，spec #704 / ADR-0099）。
//!
//! 职责：列表/搜索命中页的来源列反查与转换扩展填充（`attach_sources` /
//! `attach_convert_fields`），逐级只对尚无来源的行填充、不做逐行 N+1。不变量：
//! 来源与转换是读时推导，零库列、写路径不填充；未注册接缝即码化错误。ADR 指针：
//! ADR-0112 决策 5 / ADR-0113 决策 8。陷阱：注册点在跨域接缝区
//! `crate::seams::source` 与 `crate::seams::investment`，本模块只消费。

use std::collections::HashMap;

use rusqlite::Connection;

use ledger_infra::error::Result;

use crate::amount::TransactionKind;
use crate::model::{ConvertFields, Transaction};
use crate::seams::investment::{resolve_convert_fields, resolve_instrument_sources};
use crate::seams::source::{resolve_item_sources, resolve_plan_sources, resolve_policy_sources};

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
        // 经保单直挂反查接缝（#1092）：注册点在接缝区，实现由保单域启动时装入。
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
    // 经计划来源解析接缝（#1090）：注册点在接缝区，实现由定时计划域启动时装入，
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
    // 经物品反查接缝（#1092）：注册点在接缝区，实现由物品域启动时装入。
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
    //    为主键，一交易至多一行）——经交易×投资接缝（#1092，`crate::seams::investment`）：
    //    注册点在接缝区，实现由投资域启动时装入。
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
