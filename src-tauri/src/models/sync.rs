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
    /// 终态是否被中断（issue #104）：`done=true` 且 `cancelled=true` 表示同步被中断，
    /// `done=true` 且 `cancelled=false` 表示同步正常完成。`done=false` 时忽略此字段。
    pub cancelled: bool,
}

/// 全量同步中断命令的结果（issue #104）：`cancelled=true` 表示确实中断了一个正在进行的同步，
/// `false` 表示调用时无同步在跑（无副作用）。`message` 为可直接展示的中文提示。
#[derive(Debug, Clone, Serialize)]
pub struct CancelSyncResult {
    /// 是否确实中断了一个正在进行的全量同步。
    pub cancelled: bool,
    /// 结果提示文案（无同步时为「当前没有正在进行的同步」，否则「已请求中断同步」）。
    pub message: String,
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
