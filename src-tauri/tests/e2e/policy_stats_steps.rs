use cucumber::{then, when};

use ledger_policy::PolicyStats;
use ledger_policy::{delete_policy, policy_stats};

use crate::world::LedgerWorld;

#[when(expr = "软删保单号 {string}")]
fn delete_policy_by_number(world: &mut LedgerWorld, number: String) {
    let id = policy_id_by_number(world, &number);
    delete_policy(&world_conn!(world), &id, &mut || {}).expect("软删保单应成功但失败");
}

#[when(expr = "以 {string} 为今日查询保单统计")]
fn query_policy_stats(world: &mut LedgerWorld, today: String) {
    let today =
        chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").expect("今日日期应为 YYYY-MM-DD");
    world.policy.policy_stats_list =
        policy_stats(&world_conn!(world), today).expect("查询保单统计失败");
}

fn policy_id_by_number(world: &LedgerWorld, number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1 AND is_deleted=0",
            [number],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("保单 {number} 不存在"))
}

fn stats_by_number<'a>(world: &'a LedgerWorld, number: &str) -> &'a PolicyStats {
    let id = policy_id_by_number(world, number);
    world
        .policy
        .policy_stats_list
        .iter()
        .find(|s| s.policy_id == id)
        .unwrap_or_else(|| panic!("保单统计应含保单 {number} 的行"))
}

#[then(expr = "保单统计应包含 {int} 张保单")]
fn check_stats_count(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.policy.policy_stats_list.len(),
        expected,
        "保单统计行数不符"
    );
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
