export interface ExchangeRate {
  id: string;
  base_code: string;
  quote_code: string;
  rate: number;
  priced_at: string;
  source: string | null;
  updated_at: string;
  version: number;
  device_id: string;
}

export interface ExchangeRateInput {
  base_code: string;
  quote_code: string;
  rate: number;
  priced_at: string;
  source?: string | null;
}

/** 落库统计（issue #1543）：同步结果面的覆盖区间与条数。零痕迹跳过时为
 * 全零/空（判据层合法成功）；「取数了但零点」由后端报 fx.source-no-data。 */
export interface ExchangeRateSyncPersistReport {
  pairs: number;
  points: number;
  earliest: string | null;
  latest: string | null;
  manual_protected: number;
}

/** 手动同步汇率一次的报告（issue #1545，取数深度由 #1544 窗口判据分派）：
 * full_backfilled = 本次执行了全量历史回填（false = 深度已达成只走增量，或
 * 零痕迹跳过）；persist 为落库统计——结果面展示覆盖区间（earliest/latest）
 * 与条数（points）。 */
export interface ExchangeRateSyncReport {
  full_backfilled: boolean;
  persist: ExchangeRateSyncPersistReport;
}

/** 汇率同步阶段（issue #1762）：后端 `ledger:fx-sync-progress` 事件的 stage 闭集——
 * fetching = 正在读取汇率文件（含下载与解析），persisting = 正在写入。
 * 下载与解析在取数腿内一体完成、不拆分。 */
export type FxSyncStage = "fetching" | "persisting";

/** 汇率同步阶段事件载荷（issue #1762）：阶段闭集 + 可选已解析天数——读取开始时
 * 为 null，读取完成（进入写入）时一次性上报本次共解析天数。事件契约留扩展位，
 * 将来升级实时字节 / 实时天数计数不换事件名。 */
export interface FxSyncProgress {
  stage: FxSyncStage;
  days_parsed: number | null;
}
