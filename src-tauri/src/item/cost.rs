//! DailyUsageCost 接缝（issue #114 / spec #113）：物品「每天使用成本」的单一权威。
//!
//! 口径（CONTEXT.md `DailyUsageCost` 条目）：
//! - **分子** = 物品总成本 − 可选残值，下限 0（残值 ≥ 成本时为 0，避免负成本）。
//! - **分母** = 购买日期 → 目标日期的**日历天数，含起止两端**（购买当日即目标日 = 1 天，
//!   不存在 0 天）；目标日早于购买日属非法输入，报错而非静默回绕。
//! - 只输出「每天」一个口径（月/年由调用方与用户自行换算，避免口径发散）。
//!
//! 纯计算模块（仿 `transaction::amount` 先例）：不触库、不做历史汇率折算
//! （MVP 在默认币种内计算）。调用方（命令层、dashboard 聚合）一律经本模块取值，
//! 不另写口径表达式。
//!
//! 消费方接线说明：本模块随 issue #114 先行落地，`commands::item`（issue #115+）
//! 接入后语义由 BDD 场景二次锁定；边界行为由本模块单元测试锁定。

use chrono::NaiveDate;

use crate::error::{AppError, Result};

/// 每天使用成本的计算结果：天数、分子与每天成本一并返回，
/// 供物品列表（需展示「已用天数」）与 dashboard 聚合共用，避免调用方重算天数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyUsageCost {
    /// 拥有天数：购买日 → 目标日的日历天数，含起止两端（同日 = 1）。
    pub days: i64,
    /// 分子（分）：总成本 − 残值，下限 0。
    pub numerator_cents: i64,
    /// 每天成本（分/天，**小数**）：`numerator_cents ÷ days`。
    ///
    /// 仅供展示（前端 `formatAmount` 折算），不参与再聚合——调用方需要合计时
    /// 应先累加各自的 `numerator_cents` 与 `days` 再相除，勿拿 f64 反推口径。
    pub per_day_cents: f64,
}

/// 「在用物品摊到今天」的默认目标日：本地时区的今天。
///
/// 注意：这里是**日历日**而非时间戳，故不适用 `db::now_iso()`（UTC 时间戳）惯例——
/// “今天”以用户本地时区的日界为准（与用户对“我用了几天”的直觉一致），
/// 不随 UTC 日界翻转。
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// 在用物品便捷入口：目标日默认今天（无自选参考日时的口径）。
pub fn calculate_to_today(
    total_cost_cents: i64,
    purchase_date: NaiveDate,
    residual_value_cents: Option<i64>,
) -> Result<DailyUsageCost> {
    calculate(
        total_cost_cents,
        purchase_date,
        today(),
        residual_value_cents,
    )
}

/// 计算一件物品的每天使用成本。
///
/// - `target_date`：参考日——在用物品传今天（见 [`calculate_to_today`]），
///   已处置物品传处置日；用户自选参考日时传自选值。
/// - `residual_value_cents`：可选残值（已处置物品可填）；
///   `None` 视同 0。残值 ≥ 成本（或总成本为负）时分子下限 0，不输出负成本。
/// - 目标日早于购买日 → [`AppError::Invalid`]。
pub fn calculate(
    total_cost_cents: i64,
    purchase_date: NaiveDate,
    target_date: NaiveDate,
    residual_value_cents: Option<i64>,
) -> Result<DailyUsageCost> {
    if target_date < purchase_date {
        return Err(AppError::Invalid(format!(
            "目标日期 {target_date} 早于购买日期 {purchase_date}，无法计算每天使用成本"
        )));
    }
    let days = (target_date - purchase_date).num_days() + 1;
    let numerator_cents = (total_cost_cents - residual_value_cents.unwrap_or(0)).max(0);
    Ok(DailyUsageCost {
        days,
        numerator_cents,
        per_day_cents: numerator_cents as f64 / days as f64,
    })
}
