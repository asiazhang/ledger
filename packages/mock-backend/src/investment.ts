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

/**
 * 投资标的域替身（issue #1969 / spec #1964 / ADR-0143）：投资页「标的」页签的
 * 字典读写闭环——读投影内存夹具（四维筛选 + 服务端分页 + anchor 定位）、写
 * 真的改内存并统一发 `ledger:changed`。刻意不追行为等价（决策 1）：
 *
 * - `invested` / `price_channel` 真后端为派生列（持仓谓词 / 价格通道判定单点
 *   在 Rust 投资域），替身按行直投影——夹具行即口径；持仓域替身落地前
 *   `only_invested` 筛选没有别的数据源可算。
 * - 按代码添加的两条通道不联网：「数据源权威名称与最新净值/现价」按代码
 *   确定性推导（规则见各函数，同代码恒同回显），形态对齐契约字段。
 * - `ledger:prices-changed` 不在本域替身发射（真后端按 price_written 广播；
 *   该信号面属行情价格票，落地前消费方在 mock 模式不刷新）。
 */

/** 标的删除的行内守卫文案与真后端码化错误语义对齐（issue #292 / ADR-0036 决策 5）。 */
const MANUAL = "manual";

// —— list_instruments：四维筛选 + symbol 排序 + 服务端分页 + anchor 定位 ——

/**
 * search 匹配目标（对齐真后端「代码 · 名称」label 子串语义的简化版）：对
 * symbol 与 name 分别做大小写不敏感子串匹配，任一命中即保留。
 */
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

// —— create_instrument / delete_instrument：自建标的写闭环 ——

/**
 * 手动创建入口守卫（对齐 ADR-0036 决策 3 的白名单语义）：股票归按代码查询/
 * 创建、基金归按代码即拉，手动字典只收债券/ETF/其他；名称必填（自建标的
 * 主身份是名称）。替身补一条存在性校验：币种须在夹具币种字典内（spec #1964
 * 「校验只做存在性必需」取向）。
 */
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

/**
 * 建档/复用单点（对齐真后端 write_instrument 语义）：按自然键（symbol, 类型）
 * 幂等 upsert——命中复用既有行 id 并只更新名称/市场（来源随行终身不变）；
 * 未命中新建行来源标 manual（非同步即手动）、市场缺省归一 unknown、无现价。
 * 价格/持仓派生列的直投影口径见模块头注。
 */
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

/**
 * 自建标的删除（对齐真后端守卫语义，issue #292 / ADR-0036 决策 5）：仅手动
 * 来源可删，同步来源拒删（名称与市场由按代码查询/创建带回权威信息维护）；
 * 物理删除（真后端不引入软删——标的字典查询面不被污染）。买卖流水引用
 * 守卫在替身暂缺对应数据面（投资交易域替身未落地），无引用可查。真后端
 * 删除不发失效信号，替身按「写路径必发 ledger:changed」统一发（spec #1964
 * 失效信号取向，多拉一次无害）。
 */
export function deleteInstrument(args: Record<string, unknown>): void {
  const id = str(args, "id");
  const row = findActive(db().instruments, id, "标的");
  if (row.source !== MANUAL) {
    throw new Error("同步来源标的不支持删除：名称与市场由按代码查询/创建带回权威信息维护");
  }
  db().instruments.splice(db().instruments.indexOf(row), 1);
  emitLedgerChanged();
}

// —— add_fund_by_code / add_instrument_by_code：按代码添加（确定性回显，不联网） ——

/** 净值占位确定性推导：code % 9000 + 1000（1000–9999，即 0.10–0.9999 元）；净值日期恒字面量。 */
const FUND_NAV_DATE = "2026-01-31";

/** 场内现价确定性推导：(ΣcharCode(symbol) % 900 + 100) × 1000（10.0–999.0 元区间）；价格日期恒字面量。 */
const STOCK_PRICE_DATE = "2026-01-30";

function charCodeSum(symbol: string): number {
  let sum = 0;
  for (const ch of symbol) sum += ch.charCodeAt(0);
  return sum;
}

function deterministicPriceCents(symbol: string): number {
  return ((charCodeSum(symbol) % 900) + 100) * 1000;
}

/** 落现价占位并返回生效行 id：自然键命中复用，未命中新建（价格通道随通道判定入参）。 */
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
      existing.name = name; // 数据源权威名称覆盖（adopt 语义）
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
    source: MANUAL, // 建档来源标记 manual（adopt_quote 语义，非同步即手动）
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

/**
 * 按代码即拉场外基金（issue #301 / ADR-0038）：不联网，名称/净值按代码确定性
 * 推导（同代码恒同回显）——name = `替身基金 ${code}`、nav = code % 9000 + 1000
 * （万分之一元刻度）、nav_date 恒字面量；字典形态与真后端一致：类型恒 fund、
 * 市场恒 unknown、币种恒 CNY（含 QDII 人民币份额）；fund_class 恒空串
 * （ADR-0130 决策 8）。同代码幂等复用。真后端落现价后广播价格失效信号，
 * 替身按 spec #1964 统一发 ledger:changed（prices-changed 属行情价格票）。
 */
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

/**
 * 场内通道代码归一（对齐真后端解析单点的归一规则）：sh/sz 要求 6 位数字；
 * hk 1–5 位数字左补零至 5 位；us 1–5 位字母归一大写。
 */
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

/**
 * 类型自动识别（替身确定性规则，非行情源探测）：真后端由行情源类型码探测
 * （ETF/LOF → etf，其余 → stock，单点在 market-sync），替身按 A 股场内基金
 * 代码段约定推导——sh 51/56/58 开头、sz 15 开头 → etf，其余与 hk/us → stock。
 */
function detectType(market: AddInstrumentChannel, symbol: string): InstrumentType {
  if (market === "sh" && /^(51|56|58)/.test(symbol)) return "etf";
  if (market === "sz" && symbol.startsWith("15")) return "etf";
  return "stock";
}

/** 美股交易所归属（替身确定性规则，非真实归属）：首字母 < N → nasdaq，其余 nyse。 */
function usExchangeOf(symbol: string): MarketType {
  return symbol[0] < "N" ? "nasdaq" : "nyse";
}

/**
 * 按代码添加·场内通道（issue #697 / ADR-0081）：不联网，识别回显按代码确定性
 * 推导——归一 symbol（港股补零/美股大写）、类型（etf 代码段）、市场（us 通道
 * 推导交易所归属）、币种（sh/sz→CNY、hk→HKD、us→USD）、现价
 * （(ΣcharCode % 900 + 100) × 1000）、价格日期恒字面量。同（symbol, type）
 * 自然键命中幂等复用（名称随数据源回显覆盖，adopt 语义）。
 */
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
