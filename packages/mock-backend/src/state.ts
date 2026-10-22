import type {
  Account,
  Budget,
  Category,
  Currency,
  Instrument,
  Insurer,
  Merchant,
  Transaction,
} from "@ledger/types";
import {
  fixtureAccounts,
  fixtureBudgets,
  fixtureCategories,
  fixtureCurrencies,
  fixtureInstruments,
  fixtureInsurers,
  fixtureMerchants,
  fixtureSettings,
  fixtureTransactions,
} from "./fixtures";
import { emitMockEvent } from "./tauri/event";

/**
 * 替身后端的共享内存状态与跨域助手（issue #1965，自 ledger.ts 下沉）：
 * 内存形态（五字典 + 预算 + 交易 + 设置 + id 计数器）、重播种、递增 id、触碰同步
 * 字段、失效信号出口、参数窄化与存在性校验助手——供各域实现文件复用，
 * 域模块不再自持状态。写入改内存、读投影内存、刷新重播种（ADR-0143 决策 2/6）。
 */

/** 账本级设置的内存形态（issue #1965 首批：本位币基准 + 日志等级；后续域随需扩展） */
export interface MockSettings {
  base_currency: string;
  log_level: string;
}

/** 夹具种子快照形态（五字典 + 标的字典 + 预算 + 交易 + 设置 + id 计数器） */
export interface MockLedger {
  currencies: Currency[];
  accounts: Account[];
  categories: Category[];
  merchants: Merchant[];
  insurers: Insurer[];
  instruments: Instrument[];
  budgets: Budget[];
  transactions: Transaction[];
  settings: MockSettings;
  counters: Map<string, number>;
}

/** 设备 id 恒为 mock（替身内全部写入行的 device_id）。 */
export const DEVICE_ID = "mock";

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
    budgets: structuredClone(fixtureBudgets),
    transactions: structuredClone(fixtureTransactions),
    settings: { ...fixtureSettings },
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
