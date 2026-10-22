import type {
  AccountMwr,
  AccountPnl,
  CurrencyCumulativePnl,
  CumulativePnlNativeTotal,
  Holding,
  InstrumentMwr,
  InvestmentOverview,
  MoneyWeightedReturnSummary,
  PortfolioValueTrend,
  PortfolioTrendPoint,
  PriceStaleness,
  RealizedPnlSummary,
  SyncInstrumentInfoResult,
  YearPnl,
} from "@ledger/types";
import { accountBalanceCents } from "./ledger";
import { argsOf, db, obj } from "./state";
import { emitMockEvent } from "./tauri/event";

/**
 * 投资聚合读数域替身（issue #1971 / spec #1964 / ADR-0143）：投资页「持仓」
 * 「概览」「盈亏」页签的聚合读投影。刻意不追行为等价（决策 1），各命令的
 * 从简口径见对应函数；本域共性口径：
 *
 * - **数据源 = 夹具直投影**：持仓行（fixtureHoldings）是全部聚合的地基，
 *   数量 / 成本与投资交易夹具对得上（见 fixtures.ts 持仓块注释）；FIFO 批次
 *   消耗、事件周汇率折算不模拟——native 折算列一律同币直填（HKD 行数值即
 *   本位币展示值，ADR-0143「复杂口径页面数字与真后端不一致是接受的差异」）。
 * - **聚合从夹具真推导**（直和 / 差值），不做硬编码常量表——测试以独立人算
 *   字面量断言，夹具改动经聚合传播（改夹具一行、聚合逐位跟随）。
 * - 除 sync_instrument_info（价格类写入，发 `ledger:prices-changed`）外全部
 *   纯读，不发失效信号。
 */

// —— list_holdings：当前持仓快照（账户 × 标的） ——

/** 持仓行投影：真后端 ORDER BY account_id, instrument_id（v_holdings 读序）。 */
export function listHoldings(): Holding[] {
  return [...db().holdings].sort(
    (a, b) =>
      a.account_id.localeCompare(b.account_id) || a.instrument_id.localeCompare(b.instrument_id),
  );
}

// —— 三腿聚合共享件：已实现（sell）与分红（dividend）两腿的事件直和 ————
//（未实现腿 = 持仓行 unrealized 直和，无事件面，各命令就地求和）

/** 已实现腿替身口径：卖出净额 − 卖出份额 × 该（账户 × 标的）当前持仓均价 −
 * 卖出费。均价 = 持仓行 cost_basis ÷ quantity（当前剩余批次回溯，不追 FIFO；
 * 无持仓行的标的卖出成本按 0，替身不追平仓批次）。 */
function sellRealizedCents(
  t: { amount_cents: number; account_id: string | null; is_deleted: boolean },
  st: { instrument_id: string; quantity: number | null; fee_cents: number },
): number {
  const holding = db().holdings.find(
    (h) => h.instrument_id === st.instrument_id && h.account_id === t.account_id,
  );
  const avgCost = holding && holding.quantity > 0 ? holding.cost_basis_cents / holding.quantity : 0;
  return t.amount_cents - (st.quantity ?? 0) * avgCost - st.fee_cents;
}

interface SellEvent {
  year: string;
  account_id: string | null;
  instrument_id: string;
  currency_code: string;
  realized: number;
}

interface DividendEvent {
  year: string;
  account_id: string | null;
  instrument_id: string;
  currency_code: string;
  amount: number;
}

/** 两腿事件集：软删行排除；口径见 sellRealizedCents / dividend 直和。 */
function legEvents(filter?: { account_id?: string | null; instrument_id?: string | null }): {
  sells: SellEvent[];
  dividends: DividendEvent[];
} {
  const sells: SellEvent[] = [];
  const dividends: DividendEvent[] = [];
  for (const t of db().transactions) {
    if (t.is_deleted) continue;
    if (filter?.account_id && t.account_id !== filter.account_id) continue;
    const st = db().securityTransactions.find((s) => s.transaction_id === t.id);
    if (!st) continue;
    if (filter?.instrument_id && st.instrument_id !== filter.instrument_id) continue;
    if (t.kind === "sell") {
      sells.push({
        year: t.date.slice(0, 4),
        account_id: t.account_id,
        instrument_id: st.instrument_id,
        currency_code: t.currency_code,
        realized: sellRealizedCents(t, st),
      });
    } else if (t.kind === "dividend") {
      dividends.push({
        year: t.date.slice(0, 4),
        account_id: t.account_id,
        instrument_id: st.instrument_id,
        currency_code: t.currency_code,
        amount: t.amount_cents,
      });
    }
  }
  return { sells, dividends };
}

// —— money_weighted_return_summary：资金加权收益率三面（ADR-0115） ——

/** 替身口径：现金流求解（XIRR）不模拟——rate 为夹具字面量（覆盖可计算 /
 * 无解两形态）；行集 = 有价持仓行（缺价行不出现、前端渲染「-」）；basis 按
 * #1343 口径人定——期初存量标的（基金 / 腾讯，夹具交易前已在库）整项
 * cumulative，夹具建仓时点已知的 ETF 给 annualized；GOLD split 无现金投入
 * → rate null（投入为零无解）。查询区间（MwrRange）不模拟——区间开始存量
 * 折期初投入的输入假设属真后端求解面。 */
export function moneyWeightedReturnSummary(): MoneyWeightedReturnSummary {
  const RATES: Record<string, number | null> = {
    "mock-inst-custom": null,
    "mock-inst-etf": -0.0005,
    "mock-inst-fund": 0.05,
    "mock-inst-hk": 0.12,
  };
  const by_instrument: InstrumentMwr[] = db()
    .holdings.filter((h) => h.latest_price_cents !== null)
    .sort((a, b) => a.instrument_id.localeCompare(b.instrument_id))
    .map((h) => ({
      account_id: h.account_id,
      instrument_id: h.instrument_id,
      currency_code: h.cost_currency_code,
      basis:
        h.instrument_id === "mock-inst-etf" ? ("annualized" as const) : ("cumulative" as const),
      rate: RATES[h.instrument_id] ?? null,
    }));

  const by_account: AccountMwr[] = [
    {
      account_id: "mock-acc-invest",
      account_name: db().accounts.find((a) => a.id === "mock-acc-invest")?.name ?? "",
      currency_code: "CNY",
      basis: "cumulative", // 名下合集含期初存量标的（#1346）
      rate: 0.08,
    },
  ];

  const total = [
    { currency_code: "CNY", basis: "cumulative" as const, rate: 0.04 },
    { currency_code: "HKD", basis: "cumulative" as const, rate: 0.12 },
  ];

  return { by_instrument, by_account, total };
}

// —— realized_pnl_summary：按年 / 按账户两张单值表（ADR-0129 两腿拆解） ——

/** 替身口径：两腿事件集（legEvents）直聚合——行键 = 两腿键集之并（只有
 * 分红、没有卖出的年份 / 账户同样成行，真后端同语义）；空腿 = 0（无匹配是
 * 零、非缺料 null，事件周汇率折算不模拟、无 null 形态）；筛选真做（账户落
 * t.account_id、标的落扩展行 instrument_id，真后端同列同义）。 */
export function realizedPnlSummary(args: Record<string, unknown>): RealizedPnlSummary {
  const filter = (obj<{ account_id?: string | null; instrument_id?: string | null } | null>(
    argsOf(args),
    "filter",
  ) ?? {}) as { account_id?: string | null; instrument_id?: string | null };
  const native = db().settings.base_currency;
  const { sells, dividends } = legEvents(filter);

  const years = new Set([...sells.map((s) => s.year), ...dividends.map((d) => d.year)]);
  const by_year: YearPnl[] = [...years].sort().map((year) => {
    const realized = sells.filter((s) => s.year === year).reduce((sum, s) => sum + s.realized, 0);
    const dividend = dividends.filter((d) => d.year === year).reduce((sum, d) => sum + d.amount, 0);
    return {
      year,
      native_currency: native,
      realized_pnl_cents: realized,
      dividend_cents: dividend,
      realized_gain_cents: realized + dividend,
    };
  });

  const accounts = db().accounts;
  const accountId = (id: string | null): string => id ?? "";
  const accountIds = new Set([
    ...sells.map((s) => accountId(s.account_id)),
    ...dividends.map((d) => accountId(d.account_id)),
  ]);
  const by_account: AccountPnl[] = [...accountIds]
    .map((id) => {
      const realized = sells
        .filter((s) => accountId(s.account_id) === id)
        .reduce((sum, s) => sum + s.realized, 0);
      const dividend = dividends
        .filter((d) => accountId(d.account_id) === id)
        .reduce((sum, d) => sum + d.amount, 0);
      return {
        account_id: id,
        account_name: accounts.find((a) => a.id === id)?.name ?? "",
        native_currency: native,
        realized_pnl_cents: realized,
        dividend_cents: dividend,
        realized_gain_cents: realized + dividend,
      };
    })
    .sort(
      (a, b) =>
        a.account_name.localeCompare(b.account_name) || a.account_id.localeCompare(b.account_id),
    );

  return { by_year, by_account };
}

// —— cumulative_pnl_summary / cumulative_pnl_native_total：三腿合计（ADR-0109） ——

/** 三腿（未实现 + 已实现 + 分红）合计按计价币种分组：未实现腿按持仓行计价
 * 币种（cost_currency_code）分组、缺价行跳过不以零计入；已实现与分红腿按
 * 交易行币种分组（真后端同列）。**与真后端的差异**：真后端未实现腿按账户
 * 币种分组（v_holdings 已折算账户币，组内恒同币），替身不折算——HKD 持仓
 * 行挂 CNY 账户，按账户币种分组会组内混币，故按计价币种分组保证组内可加。 */
function cumulativeLegs(): Map<string, number> {
  const legs = new Map<string, number>();
  const add = (currency: string, cents: number): void => {
    legs.set(currency, (legs.get(currency) ?? 0) + cents);
  };
  for (const h of db().holdings) {
    if (h.unrealized_pnl_cents === null) continue; // 缺价跳过、不以零计入
    add(h.cost_currency_code, h.unrealized_pnl_cents);
  }
  const { sells, dividends } = legEvents();
  for (const s of sells) add(s.currency_code, s.realized);
  for (const d of dividends) add(d.currency_code, d.amount);
  return legs;
}

/** 三腿合计按币种分组（币种升序，真后端 ORDER BY currency_code）；三腿皆
 * 空的币种不出现。 */
export function cumulativePnlSummary(): CurrencyCumulativePnl[] {
  return [...cumulativeLegs()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([currency_code, cents]) => ({ currency_code, cumulative_pnl_cents: cents }));
}

/** 折本位币单值：替身同币直填——各组数值直加（不折算），与概览的累计收益
 * 同一三腿口径，数值互等；缺汇率的码化上抛不模拟（无汇率面）。 */
export function cumulativePnlNativeTotal(): CumulativePnlNativeTotal {
  const total = [...cumulativeLegs().values()].reduce((sum, cents) => sum + cents, 0);
  return { total_cents: total, native_currency: db().settings.base_currency };
}

// —— portfolio_value_trend：组合市值周点曲线 ————

/** 替身口径：夹具确定性序列直投影（fixturePortfolioTrendPoints，不追历史
 * 真实）；区间过滤真做（start/end 含端点裁剪，与真后端裁剪语义同形）；
 * backfill 状态不携带（序列非空，无空态补全面）。 */
export function portfolioValueTrend(args: Record<string, unknown>): PortfolioValueTrend {
  const range = (obj<{ start_date?: string | null; end_date?: string | null } | null>(
    argsOf(args),
    "filter",
  ) ?? {}) as { start_date?: string | null; end_date?: string | null };
  const points: PortfolioTrendPoint[] = db().portfolioTrendPoints.filter(
    (p) =>
      (range.start_date == null || p.date >= range.start_date) &&
      (range.end_date == null || p.date <= range.end_date),
  );
  return { currency_code: db().settings.base_currency, points };
}

// —— instrument_price_staleness：本地水位检查（零网络请求） ————

/** 替身口径：现价水位（行情采集时刻 / 净值日期）不随夹具维护，恒答无过期
 *（计数 0 不提示，页面不打扰）；阈值镜像真后端 PRICE_STALE_AFTER_DAYS = 7。 */
export function instrumentPriceStaleness(): PriceStaleness {
  return { stale_count: 0, threshold_days: 7 };
}

// —— sync_instrument_info：标的信息同步（价格类写入） ————

/** 替身口径：不联网——有通道标的（price_channel ≠ none）计入 synced、无通道
 * 计入 skipped，内存价格即「同步后价格」（维持夹具值）；文案镜像真后端格式
 *（market-sync incremental.rs：`已同步 N 只，跳过 M 只`）；降级位恒 false
 *（无批量面可失败）。信号镜像真后端 WriteOp::SyncInstrumentInfo 信号集：
 * synced > 0 即发 `ledger:prices-changed`（实际写入即发、零写入不广播，
 * ADR-0031），不发 ledger:changed。 */
export function syncInstrumentInfo(): SyncInstrumentInfoResult {
  const rows = db().instruments.filter((i) => !i.is_deleted);
  const synced = rows.filter((i) => i.price_channel !== "none").length;
  const skipped = rows.length - synced;
  if (synced > 0) emitMockEvent("ledger:prices-changed");
  return {
    synced,
    skipped,
    message: synced + skipped === 0 ? "暂无标的可同步" : `已同步 ${synced} 只，跳过 ${skipped} 只`,
    bulk_degraded: false,
  };
}

// —— investment_overview：两腿（投资账户现金 + 持仓市值）与合计 ————

/** 概览替身口径（spec #1532 的从简版）：native 全列同币直填（本位币取
 * settings.base_currency，缺汇率 / 折算不模拟）；现金腿 = 投资类型未删未隐藏
 * 账户的余额直和（复用 ledger.ts 唯一余额口径）；市值 / 未实现腿 = 持仓行
 * native 列直和（缺价行跳过并计数）；累计收益 = 未实现 + 已实现 + 分红三腿
 *（真后端同口径，ADR-0109）。 */
export function investmentOverview(): InvestmentOverview {
  const native = db().settings.base_currency;

  let marketValue: number | null = null;
  let unrealized: number | null = null;
  let missing = 0;
  for (const h of db().holdings) {
    if (h.native_market_value_cents === null || h.native_unrealized_pnl_cents === null) {
      missing++;
      continue;
    }
    marketValue = (marketValue ?? 0) + h.native_market_value_cents;
    unrealized = (unrealized ?? 0) + h.native_unrealized_pnl_cents;
  }

  const { sells, dividends } = legEvents();
  const realized = sells.reduce((sum, s) => sum + s.realized, 0);
  const dividend = dividends.reduce((sum, d) => sum + d.amount, 0);

  const investmentCash = db()
    .accounts.filter((a) => !a.is_deleted && !a.is_hidden && a.type === "investment")
    .reduce((sum, a) => sum + accountBalanceCents(a.id), 0);

  return {
    native_currency: native,
    investment_cash_cents: investmentCash,
    holdings_market_value_cents: marketValue ?? 0,
    investable_assets_cents: investmentCash + (marketValue ?? 0),
    total_market_value_cents: marketValue ?? 0,
    unrealized_pnl_cents: unrealized ?? 0,
    cumulative_pnl_cents: (unrealized ?? 0) + realized + dividend,
    missing_price_holding_count: missing,
    has_investment_account: db().accounts.some((a) => !a.is_deleted && a.type === "investment"),
  };
}
