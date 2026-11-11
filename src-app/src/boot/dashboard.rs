//! 概览视图读接缝（#2039）：四卡取数直调 `ledger-dashboard` 域入口——与
//! 旧壳 dashboard_overview / monthly_summary 命令消费同一域函数；概览卡
//! 专项读走写连接特例（域内标注），其余只读侧（ADR-0117）。

use ledger_infra::db::DbState;
use ledger_infra::error::{AppError, Result};

use super::conn_lock;

// ---------------------------------------------------------------------------
// 概览视图读接缝（#2039）：四卡取数直调域入口——与旧壳 dashboard_overview /
// monthly_summary / list_holdings / cumulative_pnl_native_total /
// financial_freedom 命令消费的同一域函数，不经 IPC。各卡各一个读闭包：
// 折算与多语句同屏口径的同一读事务收口在域内单点（dashboard 读探针与
// financial_freedom 域内 ensure_transaction，tx_scope 快照纪律），本壳不另立
// 第二份口径表达式。
// ---------------------------------------------------------------------------

/// 净资产总览（dashboard 域读探针直调，旧壳 `dashboard_overview` 命令同一
/// 执行协议）：三腿合计（账户余额含投资现金 / 持仓市值 / 实物资产）与多币
/// 种折算全在后端单点（ADR-0131 同思路：总览屏不出现第二份口径表达式）。
/// **走写连接**（旧壳同款只读甄别收口，ADR-0117 代价 3）：域读探针内含缓存
/// 自愈写（指纹失配实时重算并 UPSERT 净资产缓存），只读读连接会在只读约束
/// 上报错。
pub fn dashboard_overview(db: &DbState) -> Result<ledger_dashboard::DashboardOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_dashboard::query_dashboard_overview(&conn)
}

/// 月度收支行集（报表域读路径直调，旧壳 `monthly_summary` 命令同一执行协议）：
/// 指定年份逐月行（income_net / 毛支出 / 退款），当月行由视图按本月键拾取。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn monthly_summary_rows(
    db: &DbState,
    year: i64,
) -> Result<Vec<ledger_reports::MonthlySummary>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_reports::monthly_summary_rows(&conn, year, None, None)
}
