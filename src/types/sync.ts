/// 持仓价格增量同步结果（issue #103）：只刷新当前持仓标的的最新价。
/// 标的全量同步的控制类型（进度事件载荷 / 中断结果 / 展示结果）已随
/// ADR-0081 决策 3 退役删除（issue #698）。
export interface SyncHoldingPricesResult {
  /// 成功同步价格的股票数
  synced: number
  /// 跳过数：非股票持仓 + 停牌/无效价 + 无法构造查询代码（市场未知）的标的
  skipped: number
  /// 结果提示文案（无持仓时为「无持仓标的可同步」），供轻量消息直接展示
  message: string
}
