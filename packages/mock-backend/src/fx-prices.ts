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

/**
 * 汇率与行情价格域替身（issue #1972 / spec #1964 / ADR-0143）：设置页汇率卡
 * 与投资页录价在 mock 模式可用——汇率（当期表按货币对 upsert）、行情现价
 * （现价缓存 per-instrument upsert + 标的字典现价列同步）、手动报价（两落点：
 * 价格历史周采样覆盖 + 按最新点映像规则决定现价 upsert）全部内存态读写闭环。
 * 刻意不追行为等价（决策 1），与真后端的差异点：
 *
 * - **信号差异**（多拉一次无害，spec #1964 统一取向；先例 deleteInstrument）：
 *   `create_exchange_rate` / `create_market_price` 真后端刻意零信号（signals
 *   映射 NO_SIGNALS 行：当期汇率表不在 prices-changed 覆盖内、create_market_price
 *   为半成品通道），替身按「写路径必发失效信号」分别发 `ledger:changed` /
 *   `ledger:prices-changed`——票面判据，界面刷新闭环优先。
 * - **`sync_exchange_rates` 答确定性常量报告**：不真取数、不落库、不发进度
 *   事件（`ledger:fx-sync-progress` 恒静默）；报告为字面量（manual_protected
 *   与种子手动货币对数一致，避免「报告零保护、界面手动行明明在」的误导）。
 * - **按代码添加（adopt）不落现价缓存行**：该通道直改标的字典现价列
 *   （#1969 既有口径），本域现价缓存表无其行——list_market_prices 当前无
 *   前端消费面，差异可接受。
 * - **手动报价日期不归一**：真后端把 "2026-8-8" canonical 化为 "2026-08-08"，
 *   替身只做格式校验（YYYY-MM-DD 串），非 canonical 串直接报错。
 */

// —— 汇率：当期表读写（按货币对 upsert） ——

/** 在用汇率行，按 base_code, quote_code 排序（与真后端 list_exchange_rates 同序）。 */
export function listExchangeRates(): ExchangeRate[] {
  return [...db().exchangeRates].sort((a, b) =>
    `${a.base_code}/${a.quote_code}`.localeCompare(`${b.base_code}/${b.quote_code}`),
  );
}

/** 汇率写入（按货币对 upsert，对齐真后端 write_exchange_rate 语义）：命中复用既有行 id 只更新语义列，未命中新建。 */
export function createExchangeRate(args: Record<string, unknown>): string {
  const input = obj<ExchangeRateInput>(args, "input");
  if (input.rate <= 0) throw new Error("汇率必须大于 0"); // 对齐 fx.rate-positive 守卫文案
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

// —— 汇率同步：常量态报告（不取数、不落库、不发进度事件） ——

/** 确定性同步报告（字面量，见模块头注）：典型增量形态、覆盖区间与条数可展示。 */
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

// —— 行情现价缓存：读写（per-instrument upsert + 标的现价列同步） ——

/** 现价缓存行，按 instrument_id, priced_at 倒序（与真后端 list_market_prices 同序）。 */
export function listMarketPrices(): MarketPrice[] {
  return [...db().marketPrices].sort(
    (a, b) =>
      a.instrument_id.localeCompare(b.instrument_id) || b.priced_at.localeCompare(a.priced_at),
  );
}

export function createMarketPrice(args: Record<string, unknown>): string {
  const input = obj<MarketPriceInput>(args, "input");
  if (input.price_cents <= 0) throw new Error("价格必须大于 0"); // 对齐 instrument.price-positive 守卫文案
  findActive(db().instruments, input.instrument_id, "标的"); // 外键存在性（spec #1964 取向）
  const id = upsertCurrentPrice(input);
  emitPricesChanged();
  return id;
}

/** 现价写入单点（对齐真后端 upsert_market_price 语义）：per-instrument upsert 复用行 id，并同步标的字典现价列（两表一致性）。 */
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
  // 标的字典现价列同步（标的列表价格直投影，真后端两列由同步编排维护一致）
  const instrument = db().instruments.find((i) => i.id === input.instrument_id);
  if (instrument) instrument.price_cents = input.price_cents;
  return id;
}

// —— 手动报价：两落点写入（历史周采样 + 最新点映像规则现价） ——

/** ISO 周一日期（周采样自然键第二维，与真后端 week_start 生成列同义）。 */
function isoWeekMonday(isoDate: string): string {
  const d = new Date(`${isoDate}T00:00:00Z`);
  const day = d.getUTCDay(); // 0 = 周日
  d.setUTCDate(d.getUTCDate() + (day === 0 ? -6 : 1 - day));
  return d.toISOString().slice(0, 10);
}

/** YYYY-MM-DD 串校验（含历法有效性；canonical 归一从简，见模块头注）。 */
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
  const currency = instrument.currency_code; // 价格币种随标的字典（真后端同规则）

  // 落点前置事实：写入前的最新历史点（现价 = 最新一条历史的即时映像）
  const history = db().priceHistory.filter((h) => h.instrument_id === instrument.id);
  const latestBefore = history.reduce<string | null>(
    (max, h) => (max === null || h.trade_date > max ? h.trade_date : max),
    null,
  );

  // 落点一：价格历史周采样幂等覆盖（同周后写覆盖先写，与同步同规则）
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

  // 落点二：现价缓存 upsert——报价不早于最新点（或首价）时写入；回填旧价只沉淀历史
  const currentPriceWritten = latestBefore === null || tradeDate >= latestBefore;
  if (currentPriceWritten) {
    upsertCurrentPrice({
      instrument_id: instrument.id,
      price_cents: input.price_cents,
      currency_code: currency,
      priced_at: tradeDate,
      source: MANUAL, // 手动落价无净值日期语义（nav_date 形状不在 TS 契约内）
    });
  }

  // 实际写入任一落点即发（ADR-0031；Ok 路径历史落点必写 → 恒发，与真后端一致）
  emitPricesChanged();
  return { history_written: true, current_price_written: currentPriceWritten };
}
