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

/** 手动同步汇率一次的落库报告（issue #1545）：ECB 增量取数 → 幂等落库后的
 * 统计——结果面展示覆盖区间（earliest/latest）与条数（points）。同步成功时
 * points > 0，earliest/latest 恒非空（零点由后端报 fx.source-no-data）。 */
export interface ExchangeRateSyncReport {
  pairs: number;
  points: number;
  earliest: string | null;
  latest: string | null;
  manual_protected: number;
}
