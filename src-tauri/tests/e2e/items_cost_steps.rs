//! 物品（Item）BDD 步骤 · 成本计算主题（issue #121 自选参考日重算、
//! issue #122 dashboard 汇总卡聚合）：DailyUsageCost 三元组（分子 ÷ 天数）
//! 与在用物品每天成本合计（本位币，缺汇率上抛）。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::commands::item::{calculate_item_cost_internal, item_daily_total_internal};
use tauri_app_lib::error::AppError;
use tauri_app_lib::item::cost;
use tauri_app_lib::models::ItemDailyCost;

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 自选参考日重算（issue #121）：计算接口接受可选参考日，缺省沿用列表口径
// ---------------------------------------------------------------------------

/// 计算最近创建的物品（`world.last_item_id`）每天使用成本的共用入口。
fn calc_item_cost(
    world: &mut LedgerWorld,
    id: &str,
    reference_date: Option<String>,
) -> Result<ItemDailyCost, AppError> {
    calculate_item_cost_internal(&world_conn!(world), id, reference_date.as_deref())
}

/// 缺省参考日（不传）：在用 → 今天；已处置 → 处置日（口径与列表一致）。
#[when(expr = "按最近创建的物品计算每天成本 不带参考日")]
fn calc_item_cost_default(world: &mut LedgerWorld) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.last_item_cost = Some(calc_item_cost(world, &id, None).expect("计算每天成本应成功"));
}

/// 自选参考日 = 今天前 N 天（相对日期，保证天数可静态断言）。
#[when(expr = "按最近创建的物品计算每天成本 今天前 {int} 天为参考日")]
fn calc_item_cost_days_ago(world: &mut LedgerWorld, days_ago: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 自选参考日 = 今天后 N 天（预览「用满 N 天」的摊薄）。
#[when(expr = "按最近创建的物品计算每天成本 今天后 {int} 天为参考日")]
fn calc_item_cost_days_later(world: &mut LedgerWorld, days_later: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() + chrono::Duration::days(days_later))
        .format("%Y-%m-%d")
        .to_string();
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 自选固定参考日（YYYY-MM-DD）。
#[when(expr = "按最近创建的物品计算每天成本 参考日 {string}")]
fn calc_item_cost_fixed_ref(world: &mut LedgerWorld, date: String) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

/// 尝试按指定参考日计算并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试按最近创建的物品计算每天成本 参考日 {string}")]
fn try_calc_item_cost(world: &mut LedgerWorld, date: String) {
    // 不存在场景传固定假 id，真实走到 query_one 落空的 NotFound 路径（同其它步骤惯例）
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item-id".into());
    world.last_error = match calc_item_cost(world, &id, Some(date)) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 尝试计算不存在的物品 id（固定假 id 走 NotFound 报错路径）。
#[when(expr = "尝试按不存在的物品计算每天成本")]
fn try_calc_item_cost_missing(world: &mut LedgerWorld) {
    world.last_error = match calc_item_cost(world, "no-such-item-id", None) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 断言重算结果三元组：分子 ÷ 天数 = 每天成本（与详情视图展示口径一致）。
#[then(expr = "计算结果已用天数应为 {int} 分子应为 {int} 每天成本应为 {float}")]
fn check_calc_item_cost(world: &mut LedgerWorld, days: i64, numerator: i64, per_day: f64) {
    let result = world
        .last_item_cost
        .as_ref()
        .unwrap_or_else(|| panic!("没有计算结果（先调「按最近创建的物品计算每天成本」）"));
    assert_eq!(result.used_days, days, "重算天数不匹配");
    assert_eq!(result.numerator_cents, numerator, "重算分子不匹配");
    assert!(
        (result.per_day_cents - per_day).abs() < 1e-6,
        "重算每天成本不匹配: 期望 {per_day}, 实际 {}",
        result.per_day_cents
    );
}

#[then(expr = "计算每天成本应返回错误 {string}")]
fn check_calc_item_cost_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

// ---------------------------------------------------------------------------
// dashboard 汇总卡聚合（issue #122）：全部在用物品每天成本合计（本位币）
// ---------------------------------------------------------------------------

/// 查询全部在用物品每天成本合计（错误路径记入 last_error，供「应返回错误」断言）。
#[when(expr = "查询在用物品每天成本合计")]
fn query_item_daily_total(world: &mut LedgerWorld) {
    match item_daily_total_internal(&world_conn!(world)) {
        Ok(total) => {
            world.last_item_daily_total = Some(total);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_item_daily_total = None;
        }
    }
}

/// 断言合计三元组：每天成本合计（本位币分/天）+ 默认币种代码 + 计入件数。
#[then(expr = "在用物品每天成本合计应为 {float} 本位币应为 {string} 件数应为 {int}")]
fn check_item_daily_total(world: &mut LedgerWorld, per_day: f64, currency: String, count: usize) {
    let total = world
        .last_item_daily_total
        .as_ref()
        .expect("未查询到合计（先调「查询在用物品每天成本合计」）");
    assert!(
        (total.per_day_cents - per_day).abs() < 1e-6,
        "每天成本合计不匹配: 期望 {per_day}, 实际 {}",
        total.per_day_cents
    );
    assert_eq!(total.native_currency, currency, "合计币种应为默认币种");
    assert_eq!(total.item_count, count as u64, "计入合计的件数不匹配");
}

/// 移除汇率行（测试脚手架，与 scheduled_steps 的「存在汇率」对偶）：
/// 构造「物品落库时有汇率、聚合时缺汇率」的环境，断言错误上抛而非以零计入。
#[when(expr = "移除汇率 {string} 兑 {string}")]
fn remove_exchange_rate(world: &mut LedgerWorld, base: String, quote: String) {
    world_conn!(world)
        .execute(
            "DELETE FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            params![base, quote],
        )
        .unwrap();
}
