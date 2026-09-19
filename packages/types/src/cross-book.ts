// 跨账本投资汇总（CrossBookInvestmentSummary，issue #1196 / ADR-0114）。
// 字段命名与 Rust 侧 serde 默认（snake_case）保持一致。

/** 逐本状态闭集（与后端 serde snake_case 字面量一一对应）。 */
export type CrossBookBookStatus =
  | "included"
  | "locked"
  | "not_initialized"
  | "schema_mismatch"
  | "unreadable";

/** 逐本状态行（注册表序，含活动本）。 */
export interface CrossBookBookRow {
  /** 账本标识。 */
  id: string;
  /** 展示名。 */
  name: string;
  /** 计入状态。 */
  status: CrossBookBookStatus;
}

/** 跨账本投资汇总载荷：折算目标与全部合计在后端完成，前端不出现第二份口径。 */
export interface CrossBookInvestmentSummary {
  /** 折算目标＝主账本（当前活动账本）本位币。 */
  target_currency: string;
  /** 是否发生过当期汇率折算（界面据此标注口径）。 */
  converted: boolean;
  /** 持仓市值合计。 */
  market_value_cents: number;
  /** 持仓收益（未实现盈亏）合计。 */
  unrealized_pnl_cents: number;
  /** 累计收益合计。 */
  cumulative_pnl_cents: number;
  /** 可投资资产合计。 */
  investable_assets_cents: number;
  /** 逐本状态行（注册表序，含活动本）。 */
  books: CrossBookBookRow[];
}
