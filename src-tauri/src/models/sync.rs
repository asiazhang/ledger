//! 行情同步领域模型。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    pub current: usize,
    pub total: usize,
    pub market: String,
    pub done: bool,
    pub total_inserted: usize,
    pub total_updated: usize,
    pub error: Option<String>,
}

/// 持仓价格增量同步结果（issue #103）：只刷新当前持仓股票的最新价，
/// 不增删、不改标的字典。无持仓时返回明确提示而非报错。
#[derive(Debug, Clone, Serialize)]
pub struct SyncHoldingPricesResult {
    /// 成功同步价格的股票数。
    pub synced: usize,
    /// 跳过数：非股票持仓 + 停牌/无效价 + 无法构造查询代码（市场未知）的标的。
    pub skipped: usize,
    /// 结果提示文案（无持仓时为「无持仓标的可同步」，否则「已同步 N 只，跳过 M 只」），
    /// 供前端轻量消息直接展示。
    pub message: String,
}
