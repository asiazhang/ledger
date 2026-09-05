//! 报表领域模型（#421 随域归位）：月度汇总、分类占比、商户排行、年份筛选范围。
//!
//! 自全局模型目录迁入本域（#417 归属原则），域内类型经 `reports` 逐类型
//! 再导出，消费方经域路径显式 import，禁止 glob。

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
/// `transaction_count`（issue #617）：该商户在期间内、参与排行口径（支出 + 退款）
/// 的交易记录数，与金额同口径——退款笔数计入、无商户交易不进排行不计数、
/// 软删商户历史引用照常计数。区别于核心域 Merchant「关联交易条数（毛笔数）」
///（字典管理视角、全 kind、不做退款冲减），见 CONTEXT-core.md。
#[derive(Debug, Serialize)]
pub struct MerchantShare {
    pub merchant_id: String,
    pub merchant_name: String,
    pub amount_cents: i64,
    pub transaction_count: i64,
}

/// 商户消费排行载荷（issue #588）：排行行 + 本期全部商户净支出合计。
/// `total_cents` 是柱图 tooltip 占比的分母——分母永远是全量（与 `top_n`
/// 截断无关），截断只作用在 `rows` 上。
#[derive(Debug, Serialize)]
pub struct MerchantSharesReport {
    pub rows: Vec<MerchantShare>,
    pub total_cents: i64,
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
            // SUM(expense_net) 对无贡献行可为 NULL，归 0；COUNT(*) 恒非 NULL。
            amount_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            transaction_count: row.get::<_, i64>(3)?,
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
