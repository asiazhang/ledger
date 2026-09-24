//! 蓄水进度的双向推算（词汇表「预计达成时间（ETA）」/「所需月存」）：节奏来源
//! 闭集二值——指向目标账户的**在用**定时转账计划按周期折算月存（订阅花费先例
//! `monthly_coefficient`，issue #161 同款系数与舍入）优先，无在用计划用手填
//! 「计划月存」；两者皆无即节奏为零，推算字段全部缺席、不虚构时点（设置引导
//! 归界面，issue #1753）。
//!
//! **关联计划查找的读口径单点**（issue #1753 定案）：「某目标的关联计划」= 定时
//! 转账计划（`kind='scheduled_transfer'`）转入账户为目标专属账户
//!（`scheduled_transfer_plans.to_account_id`）、状态在用（`status='active'`，
//! 暂停 / 取消 / 完成不再计入）且未软删。全库只此一处定义该查找——后续消费
//! 关联计划的读面（如计划跳转）同源复用，不另立第二份口径。
//!
//! 推算为纯目标参数算术（已存、目标额、截止日、节奏、今天），不做利率与复利
//! 假设；输出只到金额与月数（含预计年月），无百分比口径（词汇表「蓄水进度」）。

use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::model::SavingsGoalPaceSource;

/// 关联计划折算行：在用定时转账计划的计费参数（读口径见模块头）。
struct LinkedPlanPace {
    amount_cents: i64,
    currency_code: String,
    recurrence_type: String,
    recurrence_interval: i64,
}

impl FromRow for LinkedPlanPace {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(LinkedPlanPace {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
            recurrence_type: row.get(2)?,
            recurrence_interval: row.get(3)?,
        })
    }
}

/// 关联计划折算月存（节奏来源闭集二值之「计划」）：指向目标账户的在用定时转账
/// 计划逐计划折算——先折算本位币（Amount 接缝当期折算，订阅花费同款；计划币种
/// 即专属账户币种——定时转账创建入口拒绝跨币种，正常数据零汇率读取）、再乘折算
/// 系数（`ledger_scheduled::monthly_coefficient`：月 ×1 / 年 ÷12 / 周 ×52÷12 /
/// 日 ×30，间隔均摊）、四舍五入到分、最后求和。无在用计划返回 `None`——与
/// 「手填节奏」可区分，不与零金额计划混淆（计划金额建表 CHECK 正数，正常数据
/// 折算必为正）。
pub fn linked_plan_pace_monthly(conn: &Connection, account_id: &str) -> Result<Option<i64>> {
    let plans: Vec<LinkedPlanPace> = query_all(
        conn,
        "SELECT st.amount_cents,st.currency_code,st.recurrence_type,st.recurrence_interval \
         FROM scheduled_transactions st \
         JOIN scheduled_transfer_plans p ON p.scheduled_transaction_id = st.id \
         WHERE st.kind='scheduled_transfer' AND st.status='active' AND st.is_deleted=0 \
           AND p.to_account_id=?1",
        [account_id],
    )?;
    if plans.is_empty() {
        return Ok(None);
    }
    let mut monthly = 0i64;
    for p in plans {
        // 未知周期类型为脏数据，报错上抛，不静默跳过（订阅花费同款纪律）
        let recurrence_type = p.recurrence_type.parse()?;
        let native_cents =
            amount::convert_to_native_current(conn, p.amount_cents, &p.currency_code)?;
        let coefficient =
            ledger_scheduled::monthly_coefficient(recurrence_type, p.recurrence_interval);
        monthly += (native_cents as f64 * coefficient).round() as i64;
    }
    Ok(Some(monthly))
}

/// 双向推算读模型（词汇表 ETA / 所需月存——同一组输入的正反两面，不分两套口径）：
/// 全部为读时派生、不持久化；节奏为零时节奏字段缺席、其余推算字段不虚构。
#[derive(Debug, Clone, Serialize)]
pub struct SavingsGoalProjection {
    /// 当前节奏（月存，整数分）：`Some` = 有节奏（来源见 [`Self::pace_source`]），
    /// `None` = 节奏为零（无在用计划且未手填）。
    pub pace_monthly_cents: Option<i64>,
    /// 节奏来源闭集：计划折算 / 手填；节奏为零时 `None`。
    pub pace_source: Option<SavingsGoalPaceSource>,
    /// 无截止正推——还差 N 个月（向上取整）；未达成且有节奏才有值。
    pub eta_months: Option<i64>,
    /// 无截止正推——预计达成年月（`YYYY-MM`，节奏月数加到今天所在月）。
    pub eta_month: Option<String>,
    /// 有截止反推——每月需存（剩余 ÷ 剩余整月数，向上取整）；有截止、未达成且
    /// 截止日未过才有值（已过截止日不给虚构的月存）。
    pub required_monthly_cents: Option<i64>,
    /// 落后 / 超前差值 = 当前节奏 − 所需月存（正 = 超前、负 = 落后）；两侧皆有值
    /// 才有意义，任一缺席（节奏为零 / 无截止 / 已达成 / 截止日已过）即缺席。
    pub pace_delta_cents: Option<i64>,
}

/// 整数上取整除法（两除数均正）：有符号整数的 `div_ceil` 仍在 `int_roundings` 门后
///（rustc 1.98 实测 E0658），手写等价式。
fn div_ceil(a: i64, b: i64) -> i64 {
    (a + b - 1) / b
}

/// 月差（今天 → 某月）：同日历月为 0、次月为 1，跨年同样按月序号差。
fn month_diff(from: NaiveDate, to: NaiveDate) -> i64 {
    let month_index = |d: NaiveDate| i64::from(d.year()) * 12 + i64::from(d.month());
    month_index(to) - month_index(from)
}

/// 双向推算算术（纯参数，无连接）：`pace` 与来源由调用方（节奏闭集二值解析）
/// 注入，本函数只算时点与差值。达成（余额 ≥ 目标额，`remaining_cents <= 0`）
/// 不再推算——达成态由状态列表达，时点已无意义。
///
/// 月数取整口径：两者皆向上取整（「还差 N 个月」= 按当前节奏至少还要 N 个整月；
/// 「每月需存」= 剩余金额摊到剩余整月每期至少要存的整数分）。预计年月 = 今天
/// 所在月 + 月数（达成落在第 N 个月后的那个日历月）。
pub(crate) fn project(
    deadline: Option<&str>,
    remaining_cents: i64,
    pace: Option<i64>,
    pace_source: Option<SavingsGoalPaceSource>,
    today: NaiveDate,
) -> SavingsGoalProjection {
    let mut projection = SavingsGoalProjection {
        pace_monthly_cents: pace,
        pace_source,
        eta_months: None,
        eta_month: None,
        required_monthly_cents: None,
        pace_delta_cents: None,
    };
    if remaining_cents <= 0 {
        return projection; // 达成 / 超额：不推算时点与差值（纯展示态见词汇表）
    }
    // 无截止正推：还差 N 个月 + 预计年月（节奏为零不虚构，保持缺席）
    if deadline.is_none()
        && let Some(p) = pace
        && p > 0
    {
        let months = div_ceil(remaining_cents, p);
        projection.eta_months = Some(months);
        projection.eta_month = u32::try_from(months)
            .ok()
            .and_then(|m| today.checked_add_months(Months::new(m)))
            .map(|d| format!("{:04}-{:02}", d.year(), d.month()));
        // 年月溢出（脏数据级月数）：月数照报、预计年月缺席——不静默降级为空串
        //（域纪律：不虚构时点，宁可缺席不外发残值）。
        return projection;
    }
    // 有截止反推：每月需存 + 落后 / 超前差值
    if let Some(deadline) = deadline
        && let Ok(d) = chrono::NaiveDate::parse_from_str(deadline, "%Y-%m-%d")
        && d >= today
    {
        // 剩余整月数：截止日未过、且落在今天所在月内按 1 个整月计（截止当月
        // 仍须把剩余存完）；截止日已过（d < today）不反推，不虚构月存。
        let months_left = month_diff(today, d).max(1);
        let required = div_ceil(remaining_cents, months_left);
        projection.required_monthly_cents = Some(required);
        if let Some(p) = pace {
            projection.pace_delta_cents = Some(p - required);
        }
    }
    projection
}
