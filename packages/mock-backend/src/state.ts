import type {
  Account,
  Budget,
  Category,
  Currency,
  ExchangeRate,
  Holding,
  Instrument,
  Insurer,
  MarketPrice,
  Merchant,
  PhysicalAsset,
  PortfolioTrendPoint,
  SavingsGoal,
  Transaction,
} from "@ledger/types";
import {
  fixtureAccounts,
  fixtureAssetValuations,
  fixtureBudgets,
  fixtureCategories,
  fixtureCurrencies,
  fixtureExchangeRates,
  fixtureHoldings,
  fixtureInstruments,
  fixtureInsurers,
  fixtureMarketPrices,
  fixtureMerchants,
  fixturePhysicalAssets,
  fixturePortfolioTrendPoints,
  fixturePriceHistory,
  fixtureSavingsGoals,
  fixtureSecurityTxns,
  fixtureSettings,
  fixtureTransactions,
  type MockPriceHistory,
} from "./fixtures";
import { emitMockEvent } from "./tauri/event";

/**
 * 替身后端的共享内存状态与跨域助手（issue #1965，自 ledger.ts 下沉）：
 * 内存形态（五字典 + 标的字典 + 汇率/现价/价格历史 + 预算 + 交易 + 证券扩展行 + 设置 + 实物资产/储蓄目标 + id 计数器）、
 * 重播种、递增 id、触碰同步字段、失效信号出口、参数窄化与存在性校验助手——
 * 供各域实现文件复用，域模块不再自持状态。写入改内存、读投影内存、刷新
 * 重播种（ADR-0143 决策 2/6）。
 */

/** 账本级设置的内存形态（issue #1965 首批：本位币基准 + 日志等级；后续域随需扩展） */
export interface MockSettings {
  base_currency: string;
  log_level: string;
}

/** 证券交易扩展行（issue #1970，镜像真后端 `security_transactions` 表形态）：
 * 投资 kind 交易的投资域载荷——`transaction_id` 主键一对一、action 闭集、
 * convert 两腿、split 带符号增量、dividend 无数量单价。核心 `Transaction` 行
 * 不含投资字段，投资明细/编辑回填投影经本扩展行 JOIN 装配。 */
export interface MockSecurityTxn {
  transaction_id: string;
  instrument_id: string;
  action: "buy" | "sell" | "convert" | "split" | "dividend";
  quantity: number | null;
  price_cents: number | null;
  fee_cents: number;
  to_instrument_id: string | null;
  to_quantity: number | null;
  out_amount_cents: number | null;
  in_amount_cents: number | null;
}

/** 实物资产内存行：当前估值三件套与 native 口径不落行——读时由最新一条估值
 *  历史行投影（与真后端「JOIN 每资产最新一条」同语义，当前估值 = 最新一条）。 */
export type MockPhysicalAsset = Omit<
  PhysicalAsset,
  | "current_valuation_cents"
  | "current_valuation_currency_code"
  | "current_valuation_date"
  | "current_valuation_native_cents"
  | "native_currency"
>;

/** 估值历史行（内存形态，issue #1975）：只追加不改写；无同步字段——不外发，
 *  当前估值的唯一事实源。 */
export interface MockAssetValuation {
  id: string;
  asset_id: string;
  /** 估值日期（YYYY-MM-DD）；最新判定 = 日期最大，同日按插入序（数组序）。 */
  valuation_date: string;
  amount_cents: number;
  currency_code: string;
}

/** 夹具种子快照形态（五字典 + 标的字典 + 汇率/现价/价格历史 + 预算 + 交易 +
 * 证券扩展行 + 持仓 + 组合走势周点 + 设置 + 实物资产/储蓄目标 + id 计数器） */
export interface MockLedger {
  currencies: Currency[];
  accounts: Account[];
  categories: Category[];
  merchants: Merchant[];
  insurers: Insurer[];
  instruments: Instrument[];
  exchangeRates: ExchangeRate[];
  marketPrices: MarketPrice[];
  priceHistory: MockPriceHistory[];
  budgets: Budget[];
  transactions: Transaction[];
  securityTransactions: MockSecurityTxn[];
  holdings: Holding[];
  portfolioTrendPoints: PortfolioTrendPoint[];
  settings: MockSettings;
  physical_assets: MockPhysicalAsset[];
  physical_asset_valuations: MockAssetValuation[];
  savings_goals: SavingsGoal[];
  counters: Map<string, number>;
}

/** 设备 id 恒为 mock（替身内全部写入行的 device_id）。 */
export const DEVICE_ID = "mock";

/** 替身「今日」：缺省日期（估值日等）与推算窗口（储蓄目标 ETA / 所需月存）的
 *  确定性锚（budget.ts 同款锚 2026-01-15，issue #1967 先例）——替身不随真实
 *  时钟走，刷新重播种后期望值逐字节可复现。 */
export const MOCK_TODAY = "2026-01-15";

let current: MockLedger = seed();

/** 重新播种：状态整体回到夹具种子（安装替身与页面刷新同语义，不持久）。 */
export function reseed(): void {
  current = seed();
}

/** 当前状态访问器：reseed 整体替换快照，域模块经函数取用、不持引用。 */
export function db(): MockLedger {
  return current;
}

function seed(): MockLedger {
  return {
    currencies: structuredClone(fixtureCurrencies),
    accounts: structuredClone(fixtureAccounts),
    categories: structuredClone(fixtureCategories),
    merchants: structuredClone(fixtureMerchants),
    insurers: structuredClone(fixtureInsurers),
    instruments: structuredClone(fixtureInstruments),
    exchangeRates: structuredClone(fixtureExchangeRates),
    marketPrices: structuredClone(fixtureMarketPrices),
    priceHistory: structuredClone(fixturePriceHistory),
    budgets: structuredClone(fixtureBudgets),
    transactions: structuredClone(fixtureTransactions),
    securityTransactions: structuredClone(fixtureSecurityTxns),
    holdings: structuredClone(fixtureHoldings),
    portfolioTrendPoints: structuredClone(fixturePortfolioTrendPoints),
    settings: { ...fixtureSettings },
    physical_assets: structuredClone(fixturePhysicalAssets),
    physical_asset_valuations: structuredClone(fixtureAssetValuations),
    savings_goals: structuredClone(fixtureSavingsGoals),
    counters: new Map(),
  };
}

/** 会话内递增 id（前缀按实体区分）；刷新重播种后从种子值重新起算。 */
export function nextId(prefix: string): string {
  const n = (current.counters.get(prefix) ?? 0) + 1;
  current.counters.set(prefix, n);
  return `${prefix}-${n}`;
}

/** 当前时刻 ISO 时间戳（写入行 created_at / updated_at 的统一来源）。 */
export function nowIso(): string {
  return new Date().toISOString();
}

/** 触碰同步字段（写入侧统一口径：updated_at 前进、version 自增）。 */
export function touch(row: { updated_at: string; version: number }): void {
  row.updated_at = nowIso();
  row.version += 1;
}

/** 参考/交易写入的统一失效信号出口（无 payload，ADR-0012）。 */
export function emitLedgerChanged(): void {
  emitMockEvent("ledger:changed");
}

/** 价格类写入的失效信号出口（无 payload，ADR-0031：实际写入即发、零写入不广播）。 */
export function emitPricesChanged(): void {
  emitMockEvent("ledger:prices-changed");
}

// —— 参数窄化助手：invoke args 是 Record<string, unknown>，命令实现按契约取用 ——

export function argsOf(args: Record<string, unknown> | undefined): Record<string, unknown> {
  return args ?? {};
}

export function str(args: Record<string, unknown>, key: string): string {
  const v = args[key];
  if (typeof v !== "string" || v === "") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

export function bool(args: Record<string, unknown>, key: string): boolean {
  return args[key] === true;
}

/** 必需布尔参数窄化：缺失或非布尔报错（与 str() 同语义的校验型窄化，开关推送类命令用）。 */
export function reqBool(args: Record<string, unknown>, key: string): boolean {
  const v = args[key];
  if (typeof v !== "boolean") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

/** 按声明字段集捡拾对象参数（input / filter / items 等嵌套对象直通泛型）。 */
export function obj<T>(args: Record<string, unknown>, key: string): T {
  return args[key] as T;
}

// —— 共享小件 ——

export function findActive<T extends { id: string; is_deleted: boolean }>(
  rows: T[],
  id: string,
  label: string,
): T {
  const row = rows.find((r) => r.id === id && !r.is_deleted);
  if (!row) throw new Error(`替身后端：${label}不存在或已删除 ${id}`);
  return row;
}

export function softDelete<
  T extends { id: string; is_deleted: boolean; updated_at: string; version: number },
>(rows: T[], id: string, label: string): void {
  const row = findActive(rows, id, label);
  row.is_deleted = true;
  touch(row);
}
