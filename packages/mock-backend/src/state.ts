import type {
  Account,
  Budget,
  Category,
  Currency,
  ExchangeRate,
  Holding,
  InstallmentPlan,
  Instrument,
  Insurer,
  Item,
  MarketPrice,
  Merchant,
  PhysicalAsset,
  Policy,
  PortfolioTrendPoint,
  SavingsGoal,
  ScheduledTransaction,
  ScheduledTransactionOccurrence,
  ScheduledTransferPlan,
  SubscriptionPlan,
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
  fixtureInstallmentPlans,
  fixtureInstruments,
  fixtureInsurers,
  fixtureItems,
  fixtureMarketPrices,
  fixtureMerchants,
  fixturePhysicalAssets,
  fixturePolicies,
  fixturePolicyPendingCharges,
  fixturePortfolioTrendPoints,
  fixturePriceHistory,
  fixtureSavingsGoals,
  fixtureScheduledOccurrences,
  fixtureScheduledPlans,
  fixtureScheduledTransferPlans,
  fixtureSecurityTxns,
  fixtureSettings,
  fixtureSubscriptionPlans,
  fixtureTransactions,
} from "./fixtures";
import { emitMockEvent } from "./tauri/event";

interface MockSettings {
  base_currency: string;
  log_level: string;
}

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

export type MockPhysicalAsset = Omit<
  PhysicalAsset,
  | "current_valuation_cents"
  | "current_valuation_currency_code"
  | "current_valuation_date"
  | "current_valuation_native_cents"
  | "native_currency"
>;

export interface MockAssetValuation {
  id: string;
  asset_id: string;
  valuation_date: string;
  amount_cents: number;
  currency_code: string;
}

export interface MockPriceHistory {
  instrument_id: string;
  trade_date: string;
  week_start: string;
  price_cents: number;
  currency_code: string;
  source: string;
  updated_at: string;
  version: number;
  device_id: string;
}

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
  scheduledPlans: ScheduledTransaction[];
  scheduledOccurrences: ScheduledTransactionOccurrence[];
  installmentPlans: InstallmentPlan[];
  subscriptionPlans: SubscriptionPlan[];
  scheduledTransferPlans: ScheduledTransferPlan[];
  items: Item[];
  policies: Policy[];
  policy_pending_charges: Record<string, string[]>;
  counters: Map<string, number>;
}

export const DEVICE_ID = "mock";

export const MANUAL = "manual";

export const MOCK_TODAY = "2026-01-15";

let current: MockLedger = seed();

export function reseed(): void {
  current = seed();
}

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
    scheduledPlans: structuredClone(fixtureScheduledPlans),
    scheduledOccurrences: structuredClone(fixtureScheduledOccurrences),
    installmentPlans: structuredClone(fixtureInstallmentPlans),
    subscriptionPlans: structuredClone(fixtureSubscriptionPlans),
    scheduledTransferPlans: structuredClone(fixtureScheduledTransferPlans),
    items: structuredClone(fixtureItems),
    policies: structuredClone(fixturePolicies),
    policy_pending_charges: structuredClone(fixturePolicyPendingCharges),
    counters: new Map(),
  };
}

export function nextId(prefix: string): string {
  const n = (current.counters.get(prefix) ?? 0) + 1;
  current.counters.set(prefix, n);
  return `${prefix}-${n}`;
}

export function nowIso(): string {
  return new Date().toISOString();
}

export function touch(row: { updated_at: string; version: number }): void {
  row.updated_at = nowIso();
  row.version += 1;
}

export function emitLedgerChanged(): void {
  emitMockEvent("ledger:changed");
}

export function emitPricesChanged(): void {
  emitMockEvent("ledger:prices-changed");
}

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

export function reqBool(args: Record<string, unknown>, key: string): boolean {
  const v = args[key];
  if (typeof v !== "boolean") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

export function obj<T>(args: Record<string, unknown>, key: string): T {
  return args[key] as T;
}

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
