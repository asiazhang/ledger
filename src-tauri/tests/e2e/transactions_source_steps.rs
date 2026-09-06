//! 交易来源字段契约 BDD 步骤（spec #704 / issue #706 tracer bullet：保单分支）。
//!
//! 断言口径是命令输出本身（好测试只测外部行为）：列表断言经
//! `list_transactions_internal` 现查（与列表命令同一排序：date DESC,
//! created_at DESC, id DESC），搜索断言读 `search_steps.rs` 的
//! `world.last_search` 快照。保单/商户/账户 Given 复用 `policies_steps.rs` /
//! `merchants_steps.rs` / `accounts_steps.rs` 已注册步骤。

use cucumber::then;
use rusqlite::params;

use tauri_app_lib::transaction::{
    TransactionListFilter, TransactionSourceKind, TransactionSourceStatus,
    list_transactions_internal,
};

use crate::world::LedgerWorld;

/// 按保单号查保单 id（场景内保单号唯一）。
fn policy_id_by_number(world: &LedgerWorld, number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1",
            params![number],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：保单 {number} 应已存在"))
}

/// 断言来源 = 保单来源（类型/实体 id/险种名/状态标注）：列表与搜索侧共用。
fn assert_policy_source(
    world: &LedgerWorld,
    source: &tauri_app_lib::transaction::TransactionSource,
    policy_number: &str,
    product_name: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    assert_eq!(
        source.kind,
        TransactionSourceKind::Policy,
        "来源类型应为保单"
    );
    assert_eq!(
        source.entity_id,
        policy_id_by_number(world, policy_number),
        "来源实体 id 应为挂单保单 id"
    );
    assert_eq!(source.display_name, product_name, "来源展示名应为险种名");
    assert_eq!(source.status, expected_status, "来源状态标注不匹配");
}

/// 断言第 index 条列表行来源 = 保单来源（类型/实体 id/险种名），并核对状态标注。
fn assert_list_nth_policy_source(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: &str,
    product_name: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("交易列表查询失败");
    let txn = result
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let source = txn
        .source
        .as_ref()
        .unwrap_or_else(|| panic!("第 {index} 条交易应携带来源，实际为空"));
    assert_policy_source(world, source, policy_number, product_name, expected_status);
}

#[then(expr = "交易列表第 {int} 条来源应为保单 {string} 险种 {string}")]
fn list_nth_source_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    assert_list_nth_policy_source(world, index, &policy_number, &product_name, None);
}

#[then(expr = "交易列表第 {int} 条来源应为已删除保单 {string} 险种 {string}")]
fn list_nth_source_deleted_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    assert_list_nth_policy_source(
        world,
        index,
        &policy_number,
        &product_name,
        Some(TransactionSourceStatus::Deleted),
    );
}

#[then(expr = "交易列表第 {int} 条应无来源")]
fn list_nth_no_source(world: &mut LedgerWorld, index: usize) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("交易列表查询失败");
    let txn = result
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    assert!(
        txn.source.is_none(),
        "第 {index} 条交易应无来源，实际: {:?}",
        txn.source
    );
}

#[then(expr = "搜索结果第 {int} 条来源应为保单 {string} 险种 {string}")]
fn search_nth_source_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    let snapshot = world
        .last_search
        .clone()
        .expect("搜索结果快照缺失（先执行搜索步骤）");
    let txn = snapshot
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条不存在"));
    let source = txn
        .source
        .as_ref()
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条应携带来源，实际为空"));
    assert_policy_source(world, source, &policy_number, &product_name, None);
}

#[then(expr = "搜索结果第 {int} 条应无来源")]
fn search_nth_no_source(world: &mut LedgerWorld, index: usize) {
    let snapshot = world
        .last_search
        .clone()
        .expect("搜索结果快照缺失（先执行搜索步骤）");
    let txn = snapshot
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条不存在"));
    assert!(
        txn.source.is_none(),
        "搜索结果第 {index} 条应无来源，实际: {:?}",
        txn.source
    );
}
