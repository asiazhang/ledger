/// 标的信息同步结果（issue #103；#827 覆盖面放开至库内全部标的 + 名称随行刷新）。
/// 标的全量同步的控制类型（进度事件载荷 / 中断结果 / 展示结果）已随
/// ADR-0081 决策 3 退役删除（issue #698）。
export interface SyncInstrumentInfoResult {
  /// 处理成功的标的数（行情分区有效价 + 基金处理成功，含基金「已是最新」）
  synced: number;
  /// 跳过数：无通道行（无行情类型/市场未知/名称充代码）+ 停牌/无效价/查询无果
  /// + 首刷查无净值的基金
  skipped: number;
  /// 结果提示文案（空库时为「暂无标的可同步」），供轻量消息直接展示
  message: string;
  /// 降级事实位（issue #1376 / ADR-0121 决策 4）：本次同步回退到逐标的通道
  /// （批量取数面失败或跨同步停用期）时为真，界面据此明示「已降级、本次较慢」；
  /// 正常（批量面命中）路径为假。缺口（批量面未覆盖的逐条回退）不是降级。
  bulk_degraded: boolean;
}

/// 标的信息同步确定进度载荷（issue #897 / ADR-0095）：后端
/// `ledger:instrument-sync-progress` 事件的 payload——done = 已完成的有通道
/// 标的数，total = 有通道标的总数（跳过行不计入分母）。原基金页级明细字段
///（`fund`，issue #1061）已随逐只净值通道换源退役（issue #1571），事件恒为
/// 两字段形状。历史回补事件（`ledger:history-backfill-progress`）同形状。
export interface InstrumentSyncProgress {
  done: number;
  total: number;
}
