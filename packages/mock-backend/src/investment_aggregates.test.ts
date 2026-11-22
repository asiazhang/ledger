import { beforeEach, describe, expect, it } from "vitest";
import type {
  CurrencyCumulativePnl,
  Holding,
  InvestmentOverview,
  MoneyWeightedReturnSummary,
  PortfolioValueTrend,
  PriceStaleness,
  RealizedPnlSummary,
  SyncInstrumentInfoResult,
} from "@ledger/types";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

beforeEach(() => {
  installMockBackend();
});

function captureEvent(event: string): () => number {
  let count = 0;
  void listen(event, () => {
    count++;
  });
  return () => count;
}

describe("investment_overview：两腿与合计、缺价计数、has_investment_account", () => {
  it("现金腿 + 持仓市值腿与合计、未实现 / 累计收益、缺价计数均为人算字面量", async () => {
    const overview = await invoke<InvestmentOverview>("investment_overview");
    expect(overview).toEqual({
      native_currency: "CNY",
      investment_cash_cents: 500_000,
      holdings_market_value_cents: 385_348_610,
      investable_assets_cents: 385_848_610,
      total_market_value_cents: 385_348_610,
      unrealized_pnl_cents: 35_003_610,
      cumulative_pnl_cents: 34_727_080,
      missing_price_holding_count: 1,
      has_investment_account: true,
    });
  });
});

describe("instrument_price_staleness：本地水位检查答无过期", () => {
  it("计数 0 + 阈值镜像真后端 PRICE_STALE_AFTER_DAYS", async () => {
    const result = await invoke<PriceStaleness>("instrument_price_staleness");
    expect(result).toEqual({ stale_count: 0, threshold_days: 7 });
  });
});

describe("sync_instrument_info：同步统计 + 恰发一次价格失效信号", () => {
  it("有通道标的计入 synced、无通道计入 skipped，降级位恒 false", async () => {
    const pricesChanged = captureEvent("ledger:prices-changed");
    const ledgerChanged = captureEvent("ledger:changed");
    const result = await invoke<SyncInstrumentInfoResult>("sync_instrument_info");
    expect(result.synced).toBe(5);
    expect(result.skipped).toBe(1);
    expect(result.message).toBe("已同步 5 只，跳过 1 只");
    expect(result.bulk_degraded).toBe(false);
    expect(pricesChanged()).toBe(1);
    expect(ledgerChanged()).toBe(0);
  });
});

describe("portfolio_value_trend：组合市值周点曲线，区间过滤生效", () => {
  it("无区间返回夹具全量序列，终点对齐当前组合总市值", async () => {
    const trend = await invoke<PortfolioValueTrend>("portfolio_value_trend", { filter: null });
    expect(trend.currency_code).toBe("CNY");
    expect(trend.points.map((p) => p.date)).toEqual([
      "2026-01-05",
      "2026-01-12",
      "2026-01-19",
      "2026-01-26",
      "2026-02-02",
      "2026-02-09",
      "2026-02-16",
      "2026-02-23",
    ]);
    expect(trend.points.at(-1)).toEqual({ date: "2026-02-23", market_value_cents: 385_348_610 });
  });

  it("区间过滤生效：start 裁剪头部 / end 裁剪尾部（含端点）", async () => {
    const recent = await invoke<PortfolioValueTrend>("portfolio_value_trend", {
      filter: { start_date: "2026-02-01", end_date: null },
    });
    expect(recent.points.map((p) => p.date)).toEqual([
      "2026-02-02",
      "2026-02-09",
      "2026-02-16",
      "2026-02-23",
    ]);
    const early = await invoke<PortfolioValueTrend>("portfolio_value_trend", {
      filter: { start_date: null, end_date: "2026-01-31" },
    });
    expect(early.points.map((p) => p.date)).toEqual([
      "2026-01-05",
      "2026-01-12",
      "2026-01-19",
      "2026-01-26",
    ]);
  });
});

describe("realized_pnl_summary：按年 / 按账户两张单值表，筛选生效", () => {
  it("无筛选：两腿各自成行、空腿为 0 不为 null，按年升序 / 账户名序", async () => {
    const summary = await invoke<RealizedPnlSummary>("realized_pnl_summary", { filter: null });
    expect(summary.by_year).toEqual([
      {
        year: "2026",
        native_currency: "CNY",
        realized_pnl_cents: -285_030,
        dividend_cents: 8_500,
        realized_gain_cents: -276_530,
      },
    ]);
    expect(summary.by_account).toEqual([
      {
        account_id: "mock-acc-invest",
        account_name: "券商账户",
        native_currency: "CNY",
        realized_pnl_cents: -285_030,
        dividend_cents: 0,
        realized_gain_cents: -285_030,
      },
      {
        account_id: "mock-acc-bank",
        account_name: "工资卡",
        native_currency: "CNY",
        realized_pnl_cents: 0,
        dividend_cents: 8_500,
        realized_gain_cents: 8_500,
      },
    ]);
  });

  it("账户筛选只留该账户事件，标的筛选只留该标的事件", async () => {
    const byBank = await invoke<RealizedPnlSummary>("realized_pnl_summary", {
      filter: { account_id: "mock-acc-bank", instrument_id: null },
    });
    expect(byBank.by_year[0]).toMatchObject({
      realized_pnl_cents: 0,
      dividend_cents: 8_500,
      realized_gain_cents: 8_500,
    });
    expect(byBank.by_account.map((r) => r.account_id)).toEqual(["mock-acc-bank"]);

    const byHk = await invoke<RealizedPnlSummary>("realized_pnl_summary", {
      filter: { account_id: null, instrument_id: "mock-inst-hk" },
    });
    expect(byHk.by_year[0]).toMatchObject({
      realized_pnl_cents: 0,
      dividend_cents: 8_500,
    });
    expect(byHk.by_account.map((r) => r.account_id)).toEqual(["mock-acc-bank"]);
  });
});

describe("cumulative_pnl_summary / cumulative_pnl_native_total：三腿合计按币种分组 / 折本位币单值", () => {
  it("三腿按计价币种分组：CNY 三腿相加、HKD 仅未实现腿", async () => {
    const groups = await invoke<CurrencyCumulativePnl[]>("cumulative_pnl_summary");
    expect(groups).toEqual([
      {
        currency_code: "CNY",
        cumulative_pnl_cents: -272_920,
      },
      {
        currency_code: "HKD",
        cumulative_pnl_cents: 35_000_000,
      },
    ]);
  });

  it("折本位币单值 = 各组直加（同币直填），与概览累计收益同值", async () => {
    const total = await invoke<{ total_cents: number; native_currency: string }>(
      "cumulative_pnl_native_total",
    );
    expect(total).toEqual({ total_cents: 34_727_080, native_currency: "CNY" });
    const overview = await invoke<InvestmentOverview>("investment_overview");
    expect(total.total_cents).toBe(overview.cumulative_pnl_cents);
  });
});

describe("money_weighted_return_summary：单标的 / 账户级 / 全账级三面", () => {
  it("缺价行不出现，rate 字面量、无解行 null 形态覆盖，basis 按期初存量口径", async () => {
    const summary = await invoke<MoneyWeightedReturnSummary>("money_weighted_return_summary", {
      range: null,
    });
    expect(summary.by_instrument).toEqual([
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-custom",
        currency_code: "CNY",
        basis: "cumulative",
        rate: null,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-etf",
        currency_code: "CNY",
        basis: "annualized",
        rate: -0.0005,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-fund",
        currency_code: "CNY",
        basis: "cumulative",
        rate: 0.05,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-hk",
        currency_code: "HKD",
        basis: "cumulative",
        rate: 0.12,
      },
    ]);
    expect(summary.by_account).toEqual([
      {
        account_id: "mock-acc-invest",
        account_name: "券商账户",
        currency_code: "CNY",
        basis: "cumulative",
        rate: 0.08,
      },
    ]);
    expect(summary.total).toEqual([
      { currency_code: "CNY", basis: "cumulative", rate: 0.04 },
      { currency_code: "HKD", basis: "cumulative", rate: 0.12 },
    ]);
  });
});

describe("list_holdings：持仓夹具直投影", () => {
  it("五标的行按（账户, 标的）排序：缺价行空值形态、native 列同币直填", async () => {
    const rows = await invoke<Holding[]>("list_holdings");
    expect(rows.map((r) => r.instrument_id)).toEqual([
      "mock-inst-bond",
      "mock-inst-custom",
      "mock-inst-etf",
      "mock-inst-fund",
      "mock-inst-hk",
    ]);

    expect(rows[2]).toEqual({
      id: "mock-hld-etf",
      account_id: "mock-acc-invest",
      instrument_id: "mock-inst-etf",
      quantity: 1097,
      cost_basis_cents: 225_000,
      cost_currency_code: "CNY",
      latest_price_cents: 20_500,
      latest_price_currency_code: "CNY",
      latest_nav_date: null,
      market_value_cents: 224_885,
      unrealized_pnl_cents: -115,
      native_market_value_cents: 224_885,
      native_unrealized_pnl_cents: -115,
      updated_at: "2026-01-01T00:00:00Z",
    });

    const bond = rows[0];
    expect(bond.latest_price_cents).toBeNull();
    expect(bond.latest_price_currency_code).toBeNull();
    expect(bond.market_value_cents).toBeNull();
    expect(bond.unrealized_pnl_cents).toBeNull();
    expect(bond.native_market_value_cents).toBeNull();
    expect(bond.native_unrealized_pnl_cents).toBeNull();

    const hk = rows[4];
    expect(hk.market_value_cents).toBe(385_000_000);
    expect(hk.native_market_value_cents).toBe(385_000_000);
    expect(hk.cost_currency_code).toBe("HKD");
    expect(rows[3].latest_nav_date).toBe("2026-01-31");
  });
});
