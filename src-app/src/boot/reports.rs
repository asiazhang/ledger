//! 报表视图读接缝（#2048）：期间口径三卡取数直调 `ledger-reports` 域入口
//! ——与旧壳 monthlySummaryForPeriod / categoryShares / merchantShares
//! 命令消费同一域函数，本壳只消费不另立口径。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;

// ---------------------------------------------------------------------------
// 报表视图读接缝（#2048）：期间口径的三张报表直调报表域入口——与旧壳
// monthly_summary / category_shares / merchant_shares 命令的 from/to 期间
// 形态消费的同一域函数，不经 IPC。期间快照 = 报表页会话态（ADR-0057 四枚
// 日期芯片，双端必有界）。读连接是成对建连的只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 月度汇总（期间内按月分布口径，issue #411 定型）：期间快照进，域聚合
/// 分组按月不变、无流水的月份不成行。遗留 `year` 参数不参与（期间口径下）。
pub fn monthly_summary_for_period(
    db: &DbState,
    range: &crate::period::PeriodRange,
) -> Result<Vec<ledger_reports::MonthlySummary>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::monthly_summary_rows(&conn, 0, Some(&range.from), Some(&range.to))
}

/// 分类份额（净额，issue #376 母集 + #411 期间化）：支出聚合 `expense_net`
/// 口径，期间快照进；缺省月份/年份遗留参数全部不参与（期间口径）。
pub fn category_shares_rows(
    db: &DbState,
    range: &crate::period::PeriodRange,
) -> Result<Vec<ledger_reports::CategoryShare>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::category_shares_rows(
        &conn,
        "expense",
        None,
        None,
        Some(&range.from),
        Some(&range.to),
    )
}

/// 商户消费排行（净额，issue #192 + #588 topN 截断 + #411 期间化）：期间快照
/// 进，载荷 = 排行行 + 全量合计（占比分母，与截断无关）。
pub fn merchant_shares_report(
    db: &DbState,
    range: &crate::period::PeriodRange,
    top_n: Option<i64>,
) -> Result<ledger_reports::MerchantSharesReport> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::merchant_shares_report(&conn, 0, Some(&range.from), Some(&range.to), top_n)
}
