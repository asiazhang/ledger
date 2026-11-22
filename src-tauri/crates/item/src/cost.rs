use chrono::NaiveDate;

use ledger_infra::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyUsageCost {
    pub days: i64,
    pub numerator_cents: i64,
    pub per_day_cents: f64,
}

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

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

pub fn calculate(
    total_cost_cents: i64,
    purchase_date: NaiveDate,
    target_date: NaiveDate,
    residual_value_cents: Option<i64>,
) -> Result<DailyUsageCost> {
    if target_date < purchase_date {
        return Err(AppError::codedp(
            "item.target-before-purchase",
            format!("目标日期 {target_date} 早于购买日期 {purchase_date}，无法计算每天使用成本"),
            &[&target_date.to_string(), &purchase_date.to_string()],
        ));
    }
    let days = (target_date - purchase_date).num_days() + 1;
    let numerator_cents = (total_cost_cents - residual_value_cents.unwrap_or(0)).max(0);
    Ok(DailyUsageCost {
        days,
        numerator_cents,
        per_day_cents: numerator_cents as f64 / days as f64,
    })
}
