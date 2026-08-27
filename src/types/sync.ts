export interface SyncProgress {
  current: number
  total: number
  market: string
  done: boolean
  total_inserted: number
  total_updated: number
  error: string | null
}

/// 同步完成后的展示结果（由 SyncProgress 的 total_inserted/total_updated 派生）。
export interface SyncResult {
  inserted: number
  updated: number
}

/// 持仓价格增量同步结果（issue #103）：只刷新当前持仓股票的最新价。
export interface SyncHoldingPricesResult {
  /// 成功同步价格的股票数
  synced: number
  /// 跳过数：非股票持仓 + 停牌/无效价 + 无法构造查询代码（市场未知）的标的
  skipped: number
  /// 结果提示文案（无持仓时为「无持仓标的可同步」），供轻量消息直接展示
  message: string
}
