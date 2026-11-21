import type {
  AddFundResult,
  AddInstrumentChannel,
  AddStockInstrumentResult,
  Instrument,
  InstrumentInput,
  InstrumentListFilter,
  InstrumentListResult,
  InstrumentType,
  MarketType,
} from "@ledger/types";
import {
  DEVICE_ID,
  MANUAL,
  argsOf,
  db,
  emitLedgerChanged,
  findActive,
  nextId,
  nowIso,
  obj,
  str,
  touch,
} from "./state";

function searchMatches(inst: Instrument, term: string): boolean {
  const t = term.toLowerCase();
  const inSymbol = inst.symbol.toLowerCase().includes(t);
  const inName = (inst.name ?? "").toLowerCase().includes(t);
  return inSymbol || inName;
}

export function listInstruments(args: Record<string, unknown>): InstrumentListResult {
  const filter = (obj<InstrumentListFilter | null>(argsOf(args), "filter") ??
    {}) as Partial<InstrumentListFilter>;

  const search = (filter.search ?? "").trim();
  const rows = [...db().instruments]
    .filter((r) => (search === "" ? true : searchMatches(r, search)))
    .filter((r) => (filter.market ? r.market === filter.market : true))
    .filter((r) => (filter.type ? r.type === filter.type : true))
    .filter((r) => (filter.only_invested === true ? r.invested : true))
    .sort((a, b) => (a.symbol < b.symbol ? -1 : a.symbol > b.symbol ? 1 : 0));

  const pageSize = Math.min(Math.max(filter.page_size ?? 50, 1), 500);
  const anchorIdx = filter.anchor_id ? rows.findIndex((r) => r.id === filter.anchor_id) : -1;
  const anchorPage = anchorIdx >= 0 ? Math.floor(anchorIdx / pageSize) + 1 : undefined;
  const page = anchorPage ?? Math.max(filter.page ?? 1, 1);
  const items = rows.slice((page - 1) * pageSize, page * pageSize);

  return {
    items,
    total: rows.length,
    ...(anchorPage !== undefined ? { anchor_page: anchorPage } : {}),
  };
}

function guardManualInput(input: InstrumentInput): void {
  if (input.type === "stock") {
    throw new Error("股票类标的不支持手动创建：请用「添加投资标的」按代码查询");
  }
  if (input.type === "fund") {
    throw new Error("基金类标的不支持手动创建：请用「添加基金」按 6 位代码自动回填");
  }
  if ((input.symbol ?? "").trim() === "") throw new Error("标的代码不能为空");
  if ((input.name ?? "").trim() === "") throw new Error("标的名称不能为空");
  if (!db().currencies.some((c) => c.code === input.currency_code)) {
    throw new Error(`替身后端：未知币种 ${String(input.currency_code)}`);
  }
}

function upsertInstrument(input: InstrumentInput): string {
  const rows = db().instruments;
  const existing = rows.find((r) => r.symbol === input.symbol && r.type === input.type);
  if (existing) {
    if (existing.name !== input.name || existing.market !== (input.market ?? "unknown")) {
      existing.name = input.name ?? null;
      existing.market = input.market ?? "unknown";
      touch(existing);
    }
    return existing.id;
  }
  const now = nowIso();
  const row: Instrument = {
    id: nextId("mock-inst"),
    symbol: input.symbol,
    type: input.type,
    name: input.name ?? null,
    currency_code: input.currency_code,
    market: input.market ?? "unknown",
    created_at: now,
    source: MANUAL,
    price_cents: null,
    invested: false,
    price_channel: "none",
    updated_at: now,
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  rows.push(row);
  return row.id;
}

export function createInstrument(args: Record<string, unknown>): string {
  const input = obj<InstrumentInput>(argsOf(args), "input");
  guardManualInput(input);
  const id = upsertInstrument(input);
  emitLedgerChanged();
  return id;
}

export function deleteInstrument(args: Record<string, unknown>): void {
  const id = str(args, "id");
  const row = findActive(db().instruments, id, "标的");
  if (row.source !== MANUAL) {
    throw new Error("同步来源标的不支持删除：名称与市场由按代码查询/创建带回权威信息维护");
  }
  db().instruments.splice(db().instruments.indexOf(row), 1);
  emitLedgerChanged();
}

const FUND_NAV_DATE = "2026-01-31";

const STOCK_PRICE_DATE = "2026-01-30";

function charCodeSum(symbol: string): number {
  let sum = 0;
  for (const ch of symbol) sum += ch.charCodeAt(0);
  return sum;
}

function deterministicPriceCents(symbol: string): number {
  return ((charCodeSum(symbol) % 900) + 100) * 1000;
}

function adoptRow(
  symbol: string,
  type: InstrumentType,
  name: string,
  market: MarketType,
  currency: string,
  priceCents: number,
  priceChannel: "fund_nav" | "quote",
): string {
  const rows = db().instruments;
  const existing = rows.find((r) => r.symbol === symbol && r.type === type);
  if (existing) {
    if (existing.name !== name) {
      existing.name = name;
      touch(existing);
    }
    existing.price_cents = priceCents;
    existing.price_channel = priceChannel;
    return existing.id;
  }
  const now = nowIso();
  const row: Instrument = {
    id: nextId("mock-inst"),
    symbol,
    type,
    name,
    currency_code: currency,
    market,
    created_at: now,
    source: MANUAL,
    price_cents: priceCents,
    invested: false,
    price_channel: priceChannel,
    updated_at: now,
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  rows.push(row);
  return row.id;
}

export function addFundByCode(args: Record<string, unknown>): AddFundResult {
  const code = str(args, "code");
  if (!/^\d{6}$/.test(code)) throw new Error("基金代码须为 6 位数字");

  const name = `替身基金 ${code}`;
  const navCents = (Number(code) % 9000) + 1000;
  const instrumentId = adoptRow(code, "fund", name, "unknown", "CNY", navCents, "fund_nav");
  emitLedgerChanged();
  return {
    instrument_id: instrumentId,
    symbol: code,
    name,
    fund_class: "",
    nav_cents: navCents,
    nav_date: FUND_NAV_DATE,
    price_written: true,
  };
}

function normalizeStockCode(market: AddInstrumentChannel, code: string): string {
  if (market === "sh" || market === "sz") {
    if (!/^\d{6}$/.test(code)) throw new Error(`${market} 通道代码须为 6 位数字`);
    return code;
  }
  if (market === "hk") {
    if (!/^\d{1,5}$/.test(code)) throw new Error("hk 通道代码须为 1–5 位数字");
    return code.padStart(5, "0");
  }
  if (/^[A-Za-z]{1,5}$/.test(code)) return code.toUpperCase();
  throw new Error("us 通道代码须为 1–5 位字母");
}

function detectType(market: AddInstrumentChannel, symbol: string): InstrumentType {
  if (market === "sh" && /^(51|56|58)/.test(symbol)) return "etf";
  if (market === "sz" && symbol.startsWith("15")) return "etf";
  return "stock";
}

function usExchangeOf(symbol: string): MarketType {
  return symbol[0] < "N" ? "nasdaq" : "nyse";
}

export function addInstrumentByCode(args: Record<string, unknown>): AddStockInstrumentResult {
  const market = str(args, "market") as AddInstrumentChannel;
  const code = str(args, "code");
  if (market !== "sh" && market !== "sz" && market !== "hk" && market !== "us") {
    throw new Error(`场内通道仅支持 sh/sz/hk/us：${market}`);
  }

  const symbol = normalizeStockCode(market, code);
  const type = detectType(market, symbol);
  const resolvedMarket: MarketType = market === "us" ? usExchangeOf(symbol) : market;
  const currency = market === "sh" || market === "sz" ? "CNY" : market === "hk" ? "HKD" : "USD";
  const name = `替身标的 ${symbol}`;
  const priceCents = deterministicPriceCents(symbol);

  const instrumentId = adoptRow(symbol, type, name, resolvedMarket, currency, priceCents, "quote");
  emitLedgerChanged();
  return {
    instrument_id: instrumentId,
    symbol,
    name,
    type,
    market: resolvedMarket,
    currency_code: currency,
    price_cents: priceCents,
    price_date: STOCK_PRICE_DATE,
    price_written: true,
  };
}
