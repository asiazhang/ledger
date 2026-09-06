//! 保单（Policy）BDD 步骤（issue #360 / spec #358 / ADR-0051）：
//! 建档读回、建档校验、编辑、软删保留。
//!
//! 经 `policy` 域 API（`tauri_app_lib::policy`）断言外部可观察行为：创建/编辑读回、
//! 校验错误信息、写后发失效信号（notify 注入，生产路径发 `ledger:changed`）、
//! 软删后不进列表且库内行引用保留不置空。
//! 保司侧 Given/When（存在保司 / 软删保司）复用 `insurers_steps.rs` 已注册步骤
//! （issue #713 换轨：保司引用保险域自有字典，不再复用商户）。

use cucumber::{then, when};

use tauri_app_lib::policy::PolicyInput;
use tauri_app_lib::policy::{
    create_policy as create_policy_domain, delete_policy as delete_policy_domain,
    list_policies as list_policies_domain, update_policy as update_policy_domain,
};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

/// 哨兵值：「无」= 止日为空（长期/终身）/ 保额缺省 / 币种缺省。
const NONE: &str = "无";

#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn build_input(
    world: &LedgerWorld,
    insurer: &str,
    policy_number: &str,
    product_name: &str,
    start_date: &str,
    end_date: &str,
    coverage: &str,
    currency: &str,
) -> PolicyInput {
    let insurer_id = world.insurer_id(insurer);
    let end = (!end_date.eq_ignore_ascii_case(NONE)).then(|| end_date.to_string());
    let amount = coverage.parse::<i64>().ok();
    // 币种哨兵「无」→ None（缺省）；保额存在且币种给出时才携带（触发成对校验路径）
    let currency_code =
        (amount.is_some() && !currency.eq_ignore_ascii_case(NONE)).then(|| currency.to_string());
    PolicyInput {
        insurer_id,
        policy_number: policy_number.into(),
        product_name: product_name.into(),
        start_date: start_date.into(),
        end_date: end,
        coverage_amount_cents: amount,
        coverage_currency_code: currency_code,
        note: None,
    }
}

/// 创建保单并要求成功；记录失效信号次数（写后发 `ledger:changed` 的 seam 断言）。
#[when(
    expr = "创建保单 保司 {string} 保单号 {string} 险种 {string} 起日 {string} 止日 {string} 保额 {string} 币种 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn create_policy(
    world: &mut LedgerWorld,
    insurer: String,
    policy_number: String,
    product_name: String,
    start_date: String,
    end_date: String,
    coverage: String,
    currency: String,
) {
    let input = build_input(
        world,
        &insurer,
        &policy_number,
        &product_name,
        &start_date,
        &end_date,
        &coverage,
        &currency,
    );
    let mut signals = 0;
    match create_policy_domain(&world_conn!(world), input, &mut || signals += 1) {
        Ok(id) => {
            world.last_policy_id = Some(id);
            world.policy_signal_count = signals;
        }
        Err(e) => panic!("创建保单应成功但失败: {e}"),
    }
}

/// 尝试创建保单并捕获错误（供「应返回错误」断言；失败不发信号）。
#[when(
    expr = "尝试创建保单 保司 {string} 保单号 {string} 险种 {string} 起日 {string} 止日 {string} 保额 {string} 币种 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn try_create_policy(
    world: &mut LedgerWorld,
    insurer: String,
    policy_number: String,
    product_name: String,
    start_date: String,
    end_date: String,
    coverage: String,
    currency: String,
) {
    let input = build_input(
        world,
        &insurer,
        &policy_number,
        &product_name,
        &start_date,
        &end_date,
        &coverage,
        &currency,
    );
    let mut signals = 0;
    match create_policy_domain(&world_conn!(world), input, &mut || signals += 1) {
        Ok(_) => panic!("创建保单应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.policy_signal_count = signals;
        }
    }
}

/// 编辑最近创建的保单（要求成功）。
#[when(
    expr = "编辑保单 保司 {string} 保单号 {string} 险种 {string} 起日 {string} 止日 {string} 保额 {string} 币种 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn update_policy(
    world: &mut LedgerWorld,
    insurer: String,
    policy_number: String,
    product_name: String,
    start_date: String,
    end_date: String,
    coverage: String,
    currency: String,
) {
    let id = world
        .last_policy_id
        .clone()
        .expect("编辑保单前应先创建保单");
    let input = build_input(
        world,
        &insurer,
        &policy_number,
        &product_name,
        &start_date,
        &end_date,
        &coverage,
        &currency,
    );
    let mut signals = 0;
    match update_policy_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => world.policy_signal_count += signals,
        Err(e) => panic!("编辑保单应成功但失败: {e}"),
    }
}

/// 尝试编辑最近创建的保单并捕获错误。
#[when(
    expr = "尝试编辑保单 保司 {string} 保单号 {string} 险种 {string} 起日 {string} 止日 {string} 保额 {string} 币种 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn try_update_policy(
    world: &mut LedgerWorld,
    insurer: String,
    policy_number: String,
    product_name: String,
    start_date: String,
    end_date: String,
    coverage: String,
    currency: String,
) {
    let id = world
        .last_policy_id
        .clone()
        .expect("编辑保单前应先创建保单");
    let input = build_input(
        world,
        &insurer,
        &policy_number,
        &product_name,
        &start_date,
        &end_date,
        &coverage,
        &currency,
    );
    let mut signals = 0;
    match update_policy_domain(&world_conn!(world), &id, input, &mut || signals += 1) {
        Ok(()) => panic!("编辑保单应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.policy_signal_count += signals;
        }
    }
}

/// 软删最近创建的保单（要求成功）。
#[when(expr = "软删第 {int} 张保单")]
fn delete_policy(world: &mut LedgerWorld, n: usize) {
    let id = world
        .policies_list
        .get(n - 1)
        .map(|p| p.id.clone())
        .or_else(|| world.last_policy_id.clone())
        .expect("软删保单前应先创建保单");
    let mut signals = 0;
    match delete_policy_domain(&world_conn!(world), &id, &mut || signals += 1) {
        Ok(()) => world.policy_signal_count += signals,
        Err(e) => panic!("软删保单应成功但失败: {e}"),
    }
}

/// 尝试软删最近创建的保单并捕获错误（已删再删场景）。
#[when(expr = "尝试软删第 {int} 张保单")]
fn try_delete_policy(world: &mut LedgerWorld, _n: usize) {
    let id = world
        .last_policy_id
        .clone()
        .expect("软删保单前应先创建保单");
    let mut signals = 0;
    match delete_policy_domain(&world_conn!(world), &id, &mut || signals += 1) {
        Ok(()) => panic!("软删保单应失败但成功"),
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.policy_signal_count += signals;
        }
    }
}

/// 刷新保单列表快照（Then 断言数据源）。
#[when(expr = "记住第 {int} 张保单的创建时间")]
fn remember_created_at(world: &mut LedgerWorld, n: usize) {
    world.policies_list = list_policies_domain(&world_conn!(world)).expect("查询保单列表失败");
    let policy = world
        .policies_list
        .get(n - 1)
        .unwrap_or_else(|| panic!("保单列表第 {n} 张不存在"));
    world.remembered_policy_created_at = Some(policy.created_at.clone());
}

// ---------------------------------------------------------------------------
// Then 断言
// ---------------------------------------------------------------------------

#[then(expr = "保单列表应包含 {int} 张保单")]
fn check_list_count(world: &mut LedgerWorld, expected: usize) {
    world.policies_list = list_policies_domain(&world_conn!(world)).expect("查询保单列表失败");
    assert_eq!(world.policies_list.len(), expected, "保单列表条数不匹配");
}

/// 取第 n 张（1 起）保单快照的辅助。
fn nth(world: &LedgerWorld, n: usize) -> &tauri_app_lib::policy::Policy {
    world
        .policies_list
        .get(n - 1)
        .unwrap_or_else(|| panic!("保单列表第 {n} 张不存在"))
}

#[then(expr = "第 {int} 张保单保司应为 {string} 保单号应为 {string} 险种应为 {string}")]
fn check_identity(
    world: &mut LedgerWorld,
    n: usize,
    insurer: String,
    number: String,
    product: String,
) {
    let policy = nth(world, n);
    // 保司以保司名断言（保险域自有字典，ADR-0082）
    let insurer_id = world.insurer_id(&insurer);
    assert_eq!(policy.insurer_id, insurer_id, "保司引用不匹配");
    assert_eq!(policy.policy_number, number, "保单号不匹配");
    assert_eq!(policy.product_name, product, "险种名称不匹配");
}

#[then(expr = "第 {int} 张保单保障期间应为 {string} 至 {string}")]
fn check_period(world: &mut LedgerWorld, n: usize, start: String, end: String) {
    let policy = nth(world, n);
    assert_eq!(policy.start_date, start, "起日不匹配");
    let expected_end = (!end.eq_ignore_ascii_case(NONE)).then(|| end.clone());
    assert_eq!(
        policy.end_date, expected_end,
        "止日不匹配（无 = 长期/终身）"
    );
}

#[then(expr = "第 {int} 张保单保额应为 {int} 币种应为 {string}")]
fn check_coverage(world: &mut LedgerWorld, n: usize, cents: i64, currency: String) {
    let policy = nth(world, n);
    assert_eq!(policy.coverage_amount_cents, Some(cents), "保额不匹配");
    assert_eq!(
        policy.coverage_currency_code.as_deref(),
        Some(currency.as_str()),
        "保额币种不匹配"
    );
}

#[then(expr = "第 {int} 张保单保额应为空")]
fn check_coverage_empty(world: &mut LedgerWorld, n: usize) {
    let policy = nth(world, n);
    assert_eq!(policy.coverage_amount_cents, None, "保额应为空");
    assert_eq!(policy.coverage_currency_code, None, "保额币种应为空");
}

#[then(expr = "第 {int} 张保单应有唯一 ID 与审计字段")]
fn check_audit(world: &mut LedgerWorld, n: usize) {
    let policy = nth(world, n).clone();
    assert!(!policy.id.is_empty(), "应有唯一 ID");
    assert!(!policy.created_at.is_empty(), "应有 created_at");
    assert!(!policy.updated_at.is_empty(), "应有 updated_at");
    assert!(!policy.device_id.is_empty(), "应有 device_id");
    assert_eq!(policy.version, 1, "新保单版本应为 1");
    assert!(!policy.is_deleted, "列表快照应为未删除");
}

#[then(expr = "第 {int} 张保单版本应为 {int} 创建时间保留")]
fn check_version_and_created_at(world: &mut LedgerWorld, n: usize, version: i64) {
    let policy = nth(world, n).clone();
    assert_eq!(policy.version, version, "版本应递增");
    assert_eq!(
        Some(&policy.created_at),
        world.remembered_policy_created_at.as_ref(),
        "编辑后 created_at 应保留"
    );
}

#[then(expr = "库内该保单行仍保留原保单号 {string} 与保司引用")]
fn check_soft_deleted_row_kept(world: &mut LedgerWorld, number: String) {
    let id = world
        .last_policy_id
        .clone()
        .expect("软删保留断言前应先创建保单");
    let (is_deleted, kept_number, kept_insurer): (i64, String, String) = world_conn!(world)
        .query_row(
            "SELECT is_deleted, policy_number, insurer_id FROM policies WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("软删后库内行应保留（不物理移除）");
    assert_eq!(is_deleted, 1, "应为软删标记");
    assert_eq!(kept_number, number, "保单号应原样保留（引用保留不置空）");
    assert!(!kept_insurer.is_empty(), "保司引用应保留不置空");
}

#[then(expr = "保单写入后应发出 {int} 次失效信号")]
fn check_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.policy_signal_count, expected,
        "失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

#[then(expr = "保单未发出失效信号")]
fn check_no_signals(world: &mut LedgerWorld) {
    assert_eq!(world.policy_signal_count, 0, "不应发出失效信号");
}

#[then(expr = "保单创建应返回错误 {string}")]
fn check_create_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

#[then(expr = "保单编辑应返回错误 {string}")]
fn check_update_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

#[then(expr = "保单删除应返回错误 {string}")]
fn check_delete_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
