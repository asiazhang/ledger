//! 储蓄目标领域模型（spec #1750 / ADR-0133）：目标实体、创建入参与蓄水进度读模型。
//!
//! 金额一律整数分、目标金额正数（守卫在写路径码化拒绝）；币种不落表——目标币种
//! 即专属账户币种，随进度读模型携带（目标域不折算，ADR-0133 决策 3）；达成是
//! 「余额 ≥ 目标额」的读时派生纯展示态，不入状态枚举、不持久化。

use std::fmt;
use std::str::FromStr;

use ledger_infra::error::AppError;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

/// 目标生命周期状态（进行中 / 归档）。达成不入本闭集——它是余额派生的纯展示态
///（词汇表「达成与归档」），落表需要交易写路径反向挂目标域钩子，违反「目标域
/// 零新写入路径」。归档 / 取消归档随生命周期票接入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavingsGoalStatus {
    Active,
    Archived,
}

impl fmt::Display for SavingsGoalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SavingsGoalStatus::Active => write!(f, "active"),
            SavingsGoalStatus::Archived => write!(f, "archived"),
        }
    }
}

impl FromStr for SavingsGoalStatus {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(SavingsGoalStatus::Active),
            "archived" => Ok(SavingsGoalStatus::Archived),
            // ADR-0050 码化收口：闭集解析未知值报码化参数错误，message 逐字保留、
            // 未知值进 params（`budget.period-unknown` / `account.type-unknown` 同形）。
            _ => Err(AppError::codedp(
                "savings-goal.status-unknown",
                format!("未知目标状态: {s}"),
                &[s],
            )),
        }
    }
}

impl ToSql for SavingsGoalStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for SavingsGoalStatus {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}

/// 储蓄目标实体（读模型，对应 `goals` 表全字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsGoal {
    pub id: String,
    /// 目标名称（目标名权威，专属账户名随动只读）。
    pub name: String,
    /// 目标金额（整数分，正数）。
    pub target_amount_cents: i64,
    /// 截止日期（可空 = 无截止日；YYYY-MM-DD）。
    pub deadline: Option<String>,
    /// 生命周期状态（进行中 / 归档；达成为读时派生）。
    pub status: SavingsGoalStatus,
    /// 手填「计划月存」（可空，整数分；节奏来源闭集二值之一，词汇表「蓄水进度」）。
    pub planned_monthly_cents: Option<i64>,
    /// 专属账户绑定（1 目标 : 1 账户）。
    pub account_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

/// 目标创建入参（IPC `create_savings_goal`）：名称、目标金额与可选截止日期——
/// 专属账户由域内自动创建，账户信息不出现在入参。
#[derive(Debug, Deserialize)]
pub struct SavingsGoalInput {
    pub name: String,
    pub target_amount_cents: i64,
    pub deadline: Option<String>,
}

/// 目标编辑入参（IPC `update_savings_goal`，issue #1752）：四字段全量替换——
/// 名称（目标名权威、专属账户名随动只读）、目标金额、可选截止日期与手填
/// 「计划月存」（可空 = 清除）。账户信息不出现在入参：账户侧无独立改名入口。
#[derive(Debug, Deserialize)]
pub struct SavingsGoalUpdateInput {
    pub name: String,
    /// 目标金额（整数分，正数——与创建同校验）。
    pub target_amount_cents: i64,
    /// 截止日期（可空 = 无截止日；全量替换，键缺席同值 None）。
    pub deadline: Option<String>,
    /// 手填「计划月存」（可空 = 清除；携带时必须为正数）。
    pub planned_monthly_cents: Option<i64>,
}
/// 蓄水进度读模型（词汇表「蓄水进度」）：由目标读命令实时计算、不持久化；
/// 只输出金额与达成判定（差值带符号），不输出百分比口径。
#[derive(Debug, Clone, Serialize)]
pub struct SavingsGoalProgress {
    pub goal: SavingsGoal,
    /// 已存 = 专属账户余额（余额缓存口径，ADR-0067）。
    pub saved_cents: i64,
    /// 还差 = 目标额 − 已存（带符号差值：超额存入后为负，达成态由 `achieved` 表达）。
    pub remaining_cents: i64,
    /// 达成 = 已存 ≥ 目标额（纯展示态，读时派生）。
    pub achieved: bool,
    /// 目标币种 = 专属账户币种（单币种，目标域不折算）。
    pub currency_code: String,
}

/// 目标行读取单点（列序 = `progress` 模块 SELECT 的目标列段，随后接外联列）：
/// 实体列消费一处、join 读法共享，避免两份列序漂移。
pub(crate) fn goal_from_row(row: &rusqlite::Row) -> rusqlite::Result<SavingsGoal> {
    Ok(SavingsGoal {
        id: row.get(0)?,
        name: row.get(1)?,
        target_amount_cents: row.get(2)?,
        deadline: row.get(3)?,
        status: row.get(4)?,
        planned_monthly_cents: row.get(5)?,
        account_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        version: row.get(9)?,
        device_id: row.get(10)?,
        is_deleted: row.get::<_, i64>(11)? != 0,
    })
}
