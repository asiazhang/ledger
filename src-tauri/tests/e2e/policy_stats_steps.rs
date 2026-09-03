//! 保单视角统计与到期态推导 BDD 步骤（issue #363 / spec #358 / ADR-0051 决策 5/6）。
//!
//! 经 `policy` 域 API（`tauri_app_lib::policy`）断言外部可观察行为：
//! 挂单保费/现金流入实时合计（逐笔对账、软删流水不计入、不挂单流水不串入）、
//! 下期扣款日来自活跃协议期次（取消/暂停不显示、多段历史取活跃段）、
//! 软删保单不进统计、到期态由保障期间推导。
//! 保单/协议/期次/交易的铺垫复用 `policies_steps` / `policy_agreement_steps` /
//! `scheduled_steps` / `transactions_write_steps` / `transactions_policy_steps`
//! 已注册步骤；「固定今日」注入与订阅花费步骤同款（确定性到期口径）。
//!
//! 断言按**保单号**定位统计行（同一场景连建多张保单 `created_at` 同秒，
//! 行序 tie-break 是不可预期的 UUID，按序号定位有歧义——先例
//! `policy_agreement_steps::policy_id_by_number`）。

use cucumber::{then, when};

use tauri_app_lib::policy::PolicyStats;
use tauri_app_lib::policy::{delete_policy, policy_stats};

use crate::world::LedgerWorld;

/// 按保单号软删（两保单并存的场景需精确定位；「第 N 张」步骤依赖列表快照
/// 或最近创建指针，多保单场景有歧义）。
#[when(expr = "软删保单号 {string}")]
fn delete_policy_by_number(world: &mut LedgerWorld, number: String) {
    let id = policy_id_by_number(world, &number);
    delete_policy(&world_conn!(world), &id, &mut || {}).expect("软删保单应成功但失败");
}

/// 以注入的固定「今日」查询逐保单统计（确定性到期口径，不依赖真实时钟）。
#[when(expr = "以 {string} 为今日查询保单统计")]
fn query_policy_stats(world: &mut LedgerWorld, today: String) {
    let today =
        chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").expect("今日日期应为 YYYY-MM-DD");
    world.policy_stats_list = policy_stats(&world_conn!(world), today).expect("查询保单统计失败");
}

// ---------------------------------------------------------------------------
// 定位辅助
// ---------------------------------------------------------------------------

/// 按保单号查未删除保单 id（场景内保单号唯一）。
fn policy_id_by_number(world: &LedgerWorld, number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1 AND is_deleted=0",
            [number],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("保单 {number} 不存在"))
}

/// 按保单号取统计行（不存在即 panic——软删保单不产生统计行）。
fn stats_by_number<'a>(world: &'a LedgerWorld, number: &str) -> &'a PolicyStats {
    let id = policy_id_by_number(world, number);
    world
        .policy_stats_list
        .iter()
        .find(|s| s.policy_id == id)
        .unwrap_or_else(|| panic!("保单统计应含保单 {number} 的行"))
}

// ---------------------------------------------------------------------------
// Then 断言
// ---------------------------------------------------------------------------

#[then(expr = "保单统计应包含 {int} 张保单")]
fn check_stats_count(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(world.policy_stats_list.len(), expected, "保单统计行数不符");
}

#[then(expr = "保单 {string} 累计已缴应为 {int} 现金流入应为 {int}")]
fn check_paid_and_inflow(world: &mut LedgerWorld, number: String, paid: i64, inflow: i64) {
    let stats = stats_by_number(world, &number);
    assert_eq!(stats.total_paid_native_cents, paid, "累计已缴保费不符");
    assert_eq!(stats.total_inflow_native_cents, inflow, "累计现金流入不符");
}

#[then(expr = "保单 {string} 下期扣款日应为 {string}")]
fn check_next_charge(world: &mut LedgerWorld, number: String, date: String) {
    assert_eq!(
        stats_by_number(world, &number).next_charge_date.as_deref(),
        Some(date.as_str()),
        "下期扣款日不符"
    );
}

#[then(expr = "保单 {string} 不应显示下期扣款日")]
fn check_no_next_charge(world: &mut LedgerWorld, number: String) {
    assert_eq!(
        stats_by_number(world, &number).next_charge_date,
        None,
        "无活跃协议（或无 pending 期次）不应显示下期扣款日"
    );
}

#[then(expr = "保单 {string} 到期态应为 {string}")]
fn check_expiry(world: &mut LedgerWorld, number: String, expected: String) {
    let expected_expired = match expected.as_str() {
        "已到期" => true,
        "保障中" => false,
        other => panic!("未知到期态文案: {other}"),
    };
    assert_eq!(
        stats_by_number(world, &number).is_expired,
        expected_expired,
        "到期态不符"
    );
}
