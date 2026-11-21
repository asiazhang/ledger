import type {
  ExchangeRate,
  ExchangeRateInput,
  ExchangeRateSyncReport,
  ManualPriceInput,
  ManualPriceResult,
  MarketPrice,
  MarketPriceInput,
} from "@ledger/types";
import {
  DEVICE_ID,
  MANUAL,
  db,
  emitLedgerChanged,
  emitPricesChanged,
  findActive,
  nextId,
  nowIso,
  obj,
  touch,
} from "./state";
import type { MockPriceHistory } from "./state";

export function listExchangeRates(): ExchangeRate[] {
  return [...db().exchangeRates].sort((a, b) =>
    `${a.base_code}/${a.quote_code}`.localeCompare(`${b.base_code}/${b.quote_code}`),
  );
}

export function createExchangeRate(args: Record<string, unknown>): string {
  const input = obj<ExchangeRateInput>(args, "input");
  if (input.rate <= 0) throw new Error("汇率必须大于 0");
  const id = upsertExchangeRate(input);
  emitLedgerChanged();
  return id;
}

function upsertExchangeRate(input: ExchangeRateInput): string {
  const rows = db().exchangeRates;
  const existing = rows.find(
    (r) => r.base_code === input.base_code && r.quote_code === input.quote_code,
  );
  if (existing) {
    existing.rate = input.rate;
    existing.priced_at = input.priced_at;
    existing.source = input.source ?? null;
    touch(existing);
    return existing.id;
  }
  const row: ExchangeRate = {
    id: nextId("mock-fx"),
    base_code: input.base_code,
    quote_code: input.quote_code,
    rate: input.rate,
    priced_at: input.priced_at,
    source: input.source ?? null,
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
  };
  rows.push(row);
  return row.id;
}

export function syncExchangeRates(): ExchangeRateSyncReport {
  return {
    full_backfilled: false,
    persist: {
      pairs: 30,
      points: 30,
      earliest: "2025-10-20",
      latest: "2026-01-15",
      manual_protected: db().exchangeRates.filter((r) => r.source === MANUAL).length,
    },
  };
}

export function listMarketPrices(): MarketPrice[] {
  return [...db().marketPrices].sort(
    (a, b) =>
      a.instrument_id.localeCompare(b.instrument_id) || b.priced_at.localeCompare(a.priced_at),
  );
}

export function createMarketPrice(args: Record<string, unknown>): string {
  const input = obj<MarketPriceInput>(args, "input");
  if (input.price_cents <= 0) throw new Error("价格必须大于 0");
  findActive(db().instruments, input.instrument_id, "标的");
  const id = upsertCurrentPrice(input);
  emitPricesChanged();
  return id;
}

function upsertCurrentPrice(input: MarketPriceInput): string {
  const rows = db().marketPrices;
  const existing = rows.find((r) => r.instrument_id === input.instrument_id);
  let id: string;
  if (existing) {
    existing.price_cents = input.price_cents;
    existing.currency_code = input.currency_code;
    existing.priced_at = input.priced_at;
    existing.source = input.source ?? null;
    touch(existing);
    id = existing.id;
  } else {
    id = nextId("mock-mp");
    const now = nowIso();
    rows.push({
      id,
      instrument_id: input.instrument_id,
      price_cents: input.price_cents,
      currency_code: input.currency_code,
      priced_at: input.priced_at,
      source: input.source ?? null,
      created_at: now,
      updated_at: now,
      version: 1,
      device_id: DEVICE_ID,
    });
  }
  const instrument = db().instruments.find((i) => i.id === input.instrument_id);
  if (instrument) instrument.price_cents = input.price_cents;
  return id;
}

function isoWeekMonday(isoDate: string): string {
  const d = new Date(`${isoDate}T00:00:00Z`);
  const day = d.getUTCDay();
  d.setUTCDate(d.getUTCDate() + (day === 0 ? -6 : 1 - day));
  return d.toISOString().slice(0, 10);
}

function parseIsoDate(raw: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(raw)) throw new Error("日期格式须为 YYYY-MM-DD");
  const d = new Date(`${raw}T00:00:00Z`);
  if (Number.isNaN(d.getTime())) throw new Error("日期格式须为 YYYY-MM-DD");
  return raw;
}

export function recordManualPrice(args: Record<string, unknown>): ManualPriceResult {
  const input = obj<ManualPriceInput>(args, "input");
  if (input.price_cents <= 0) throw new Error("价格必须大于 0");
  const tradeDate = parseIsoDate(input.date.trim());
  const instrument = findActive(db().instruments, input.instrument_id, "标的");
  const currency = instrument.currency_code;

  const history = db().priceHistory.filter((h) => h.instrument_id === instrument.id);
  const latestBefore = history.reduce<string | null>(
    (max, h) => (max === null || h.trade_date > max ? h.trade_date : max),
    null,
  );

  const weekStart = isoWeekMonday(tradeDate);
  const weekRow = history.find((h) => h.week_start === weekStart);
  if (weekRow) {
    weekRow.trade_date = tradeDate;
    weekRow.price_cents = input.price_cents;
    weekRow.source = MANUAL;
    touch(weekRow);
  } else {
    const row: MockPriceHistory = {
      instrument_id: instrument.id,
      trade_date: tradeDate,
      week_start: weekStart,
      price_cents: input.price_cents,
      currency_code: currency,
      source: MANUAL,
      updated_at: nowIso(),
      version: 1,
      device_id: DEVICE_ID,
    };
    db().priceHistory.push(row);
  }

  const currentPriceWritten = latestBefore === null || tradeDate >= latestBefore;
  if (currentPriceWritten) {
    upsertCurrentPrice({
      instrument_id: instrument.id,
      price_cents: input.price_cents,
      currency_code: currency,
      priced_at: tradeDate,
      source: MANUAL,
    });
  }

  emitPricesChanged();
  return { history_written: true, current_price_written: currentPriceWritten };
}
