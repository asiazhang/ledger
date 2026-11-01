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

/**
 * 投资聚合读数域替身行为测试（issue #1971 / spec #1964 / ADR-0143）：接缝 =
 * 应用所见的 tauri 命令面（`installMockBackend` + `invoke`），断言持仓 / 概览 /
 * 走势 / 盈亏系列的契约形状——删除任一 index.ts 登记行即未命中报错、本文件
 * 变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 期望值全部为独立人算字面量（不复算实现公式），事实源 = 夹具：
 * - 持仓（fixtureHoldings，全挂券商账户）：国债 20 张缺价（三列全空）、GOLD
 *   5 份成本 0、ETF 1097 份成本 2250 元（= 买入 2052 + convert 结转 198）、
 *   基金 1000 份成本 1200 元（期初批次单价 1.20 元）、腾讯 100 股成本
 *   35000.00 HKD。
 * - 交易现金流（fixtureTransactions）：buy −2052 / sell +30 / dividend +85
 *   全走工资卡（funding ?? account），券商账户现金恒 0。
 *
 * 聚合读数不发 ledger:changed（纯读）；sync_instrument_info 为价格类写入，
 * 只发 ledger:prices-changed（真后端 WriteOp::SyncInstrumentInfo 信号集镜像）。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅事件并返回触发计数（失效信号「恰发一次」断言用）。 */
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
      // 现金腿：券商账户 = 定投转账 5000 元（#1973 定时夹具，工资卡 → 券商）；
      // buy/sell/dividend 现金腿全走工资卡（funding ?? account）不再叠加
      investment_cash_cents: 500_000,
      // 市值腿：123_450 + 385_000_000 + 224_885 + 275（国债缺价跳过，HKD 同币直填）
      holdings_market_value_cents: 385_348_610,
      investable_assets_cents: 385_848_610, // 500_000 + 市值腿
      total_market_value_cents: 385_348_610, // 与市值腿同一聚合
      // 3_450 + 35_000_000 + (-115) + 275
      unrealized_pnl_cents: 35_003_610,
      // 未实现 35_003_610 + 已实现 (-285_030) + 分红 8_500
      cumulative_pnl_cents: 34_727_080,
      missing_price_holding_count: 1, // 国债行
      has_investment_account: true, // 券商账户在库
    });
  });
});

describe("instrument_price_staleness：本地水位检查答无过期", () => {
  it("计数 0 + 阈值镜像真后端 PRICE_STALE_AFTER_DAYS", async () => {
    const result = await invoke<PriceStaleness>("instrument_price_staleness");
    // 夹具现价水位不模拟采集时刻，替身恒答无过期（issue #1971 从简口径）
    expect(result).toEqual({ stale_count: 0, threshold_days: 7 });
  });
});

describe("sync_instrument_info：同步统计 + 恰发一次价格失效信号", () => {
  it("有通道标的计入 synced、无通道计入 skipped，降级位恒 false", async () => {
    const pricesChanged = captureEvent("ledger:prices-changed");
    const ledgerChanged = captureEvent("ledger:changed");
    const result = await invoke<SyncInstrumentInfoResult>("sync_instrument_info");
    expect(result.synced).toBe(5); // fund/hk/etf/sh/custom（通道非 none）
    expect(result.skipped).toBe(1); // 国债 price_channel=none
    expect(result.message).toBe("已同步 5 只，跳过 1 只"); // 真后端同格式
    expect(result.bulk_degraded).toBe(false); // 不联网无降级面
    expect(pricesChanged()).toBe(1); // 恰发一次（价格类写入信号）
    expect(ledgerChanged()).toBe(0); // 真后端信号集仅 prices-changed（ADR-0031 映射）
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
        realized_pnl_cents: -285_030, // 30 元净额 − 2400 份 × 1.20 元成本 − 0.30 元费
        dividend_cents: 8_500,
        realized_gain_cents: -276_530,
      },
    ]);
    expect(summary.by_account).toEqual([
      // 账户名序 = 码点序：券商账户（券 U+5238）< 工资卡（工 U+5DE5），复刻
      // 真后端 String::cmp，非拼音序（#2004：localeCompare 在 zh_CN 环境翻转）。
      {
        account_id: "mock-acc-invest",
        account_name: "券商账户",
        native_currency: "CNY",
        realized_pnl_cents: -285_030,
        dividend_cents: 0, // 无分红事件 = 零，非缺料 null
        realized_gain_cents: -285_030,
      },
      {
        account_id: "mock-acc-bank",
        account_name: "工资卡",
        native_currency: "CNY",
        realized_pnl_cents: 0,
        dividend_cents: 8_500, // 腾讯分红到账工资卡（账户端）
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
        // 3_450 + (-115) + 275（未实现） − 285_030（已实现） + 8_500（分红）
        cumulative_pnl_cents: -272_920,
      },
      {
        currency_code: "HKD",
        cumulative_pnl_cents: 35_000_000, // 腾讯未实现腿（缺价国债腿跳过不计入）
      },
    ]);
  });

  it("折本位币单值 = 各组直加（同币直填），与概览累计收益同值", async () => {
    const total = await invoke<{ total_cents: number; native_currency: string }>(
      "cumulative_pnl_native_total",
    );
    expect(total).toEqual({ total_cents: 34_727_080, native_currency: "CNY" });
    const overview = await invoke<InvestmentOverview>("investment_overview");
    expect(total.total_cents).toBe(overview.cumulative_pnl_cents); // 同口径互证
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
        basis: "cumulative", // split 折算无现金投入 → 无解行 rate null
        rate: null,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-etf",
        currency_code: "CNY",
        basis: "annualized", // 夹具建仓时点已知（buy 2026-02-03）
        rate: -0.0005,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-fund",
        currency_code: "CNY",
        basis: "cumulative", // 期初存量（真实建仓时点未知，#1343）
        rate: 0.05,
      },
      {
        account_id: "mock-acc-invest",
        instrument_id: "mock-inst-hk",
        currency_code: "HKD",
        basis: "cumulative", // 期初存量
        rate: 0.12,
      },
      // 国债行不出现：缺价跳过的行前端渲染「-」（ADR-0115）
    ]);
    expect(summary.by_account).toEqual([
      {
        account_id: "mock-acc-invest",
        account_name: "券商账户",
        currency_code: "CNY",
        basis: "cumulative", // 名下合集含期初存量标的（基金 / 腾讯）整项未年化（#1346）
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
    // 全部行挂券商账户 → 按 instrument_id 字典序（真后端 ORDER BY account_id, instrument_id）
    expect(rows.map((r) => r.instrument_id)).toEqual([
      "mock-inst-bond",
      "mock-inst-custom",
      "mock-inst-etf",
      "mock-inst-fund",
      "mock-inst-hk",
    ]);

    // 有价形态整行对齐：ETF 行（数量/成本与明细页签对上——buy 1000 + convert 转入 97）
    expect(rows[2]).toEqual({
      id: "mock-hld-etf",
      account_id: "mock-acc-invest",
      instrument_id: "mock-inst-etf",
      quantity: 1097,
      cost_basis_cents: 225_000, // 205_200 + 19_800
      cost_currency_code: "CNY",
      latest_price_cents: 20_500,
      latest_price_currency_code: "CNY",
      latest_nav_date: null, // 场内 ETF 非基金净值
      market_value_cents: 224_885, // 1097 × 2.05 元
      unrealized_pnl_cents: -115,
      native_market_value_cents: 224_885, // 同币直填
      native_unrealized_pnl_cents: -115,
      updated_at: "2026-01-01T00:00:00Z",
    });

    // 缺价形态（国债）：市值 / 盈亏 / native 三列全空
    const bond = rows[0];
    expect(bond.latest_price_cents).toBeNull();
    expect(bond.latest_price_currency_code).toBeNull();
    expect(bond.market_value_cents).toBeNull();
    expect(bond.unrealized_pnl_cents).toBeNull();
    expect(bond.native_market_value_cents).toBeNull();
    expect(bond.native_unrealized_pnl_cents).toBeNull();

    // HKD 行 native 列同币直填（不折算，ADR-0143 决策 1）+ 基金行净值日期
    const hk = rows[4];
    expect(hk.market_value_cents).toBe(385_000_000); // 100 × 38500.00 HKD
    expect(hk.native_market_value_cents).toBe(385_000_000);
    expect(hk.cost_currency_code).toBe("HKD");
    expect(rows[3].latest_nav_date).toBe("2026-01-31"); // 基金现价 = 最新净值
  });
});
