/// 标的信息同步结果（issue #103；#827 覆盖面放开至库内全部标的 + 名称随行刷新）。
/// 标的全量同步的控制类型（进度事件载荷 / 中断结果 / 展示结果）已随
/// ADR-0081 决策 3 退役删除（issue #698）。
export interface SyncInstrumentInfoResult {
  /// 处理成功的标的数（行情分区有效价 + 基金处理成功，含基金「已是最新」）
  synced: number
  /// 跳过数：无通道行（无行情类型/市场未知/名称充代码）+ 停牌/无效价/查询无果
  /// + 首刷查无净值的基金
  skipped: number
  /// 结果提示文案（空库时为「暂无标的可同步」），供轻量消息直接展示
  message: string
}

/// 场外基金深回填的页级明细（issue #1061）：正在回填的基金代码与
/// 「已完成页 / 总页数」；只在真正翻页的首刷/深回填期间出现。
export interface InstrumentSyncFundProgress {
  code: string
  page: number
  pages: number
}

/// 标的信息同步确定进度载荷（issue #897 / ADR-0095；页级明细 issue #1061）：
/// 后端 `ledger:instrument-sync-progress` 事件的 payload——done = 已完成的有通道
/// 标的数，total = 有通道标的总数（跳过行不计入分母）；fund 缺省表示标的级推进
/// （页级明细不改 done/total 的标的级口径）。
export interface InstrumentSyncProgress {
  done: number
  total: number
  /// 基金深回填页级明细（issue #1061）；标的级推进与单页基金缺省。
  fund?: InstrumentSyncFundProgress | null
}
