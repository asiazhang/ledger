//! 报表领域模型：月度汇总、分类占比、商户排行、年份筛选范围。

use serde::Serialize;

use crate::db::query::FromRow;

#[derive(Debug, Serialize)]
pub struct MonthlySummary {
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub refund_cents: i64,
}

/// 商户消费排行行（issue #192）：`expense_net`（毛支出 − 退款）按商户聚合、
/// 本位币口径；商户名取自字典行现名（改名/软删后历史引用照常统计显示）。
#[derive(Debug, Serialize)]
pub struct MerchantShare {
    pub merchant_id: String,
    pub merchant_name: String,
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct CategoryShare {
    pub category_id: String,
    pub category_name: String,
    pub amount_cents: i64,
}

/// 报表日期极值范围（issue #266 / #389）：数据驱动的极值日期对 `{min_date, max_date}`
/// （YYYY-MM-DD，空库双 null）。
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DateRange {
    pub min_date: Option<String>,
    pub max_date: Option<String>,
}

impl FromRow for MonthlySummary {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MonthlySummary {
            month: row.get::<_, String>(0)?,
            income_cents: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            expense_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            refund_cents: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    }
}

impl FromRow for MerchantShare {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MerchantShare {
            merchant_id: row.get(0)?,
            merchant_name: row.get(1)?,
            amount_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }
}

impl FromRow for CategoryShare {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CategoryShare {
            category_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            category_name: row.get(1)?,
            amount_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }
}
