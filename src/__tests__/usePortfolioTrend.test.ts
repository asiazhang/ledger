import { describe, it, expect, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import {
  mockInvoke,
  wireInvokeSeam,
  type InvokeSeamOverride,
} from "@ledger/test-support/invoke-mock";
import { withSetup } from "@ledger/test-support/mount";
import { useReferenceStore } from "@/stores/reference";
import {
  toTrendRange,
  toTrendChartSeries,
  isTrendEmpty,
  usePortfolioTrend,
} from "@/investment/usePortfolioTrend";
import type { PortfolioValueTrend } from "@ledger/types";

const portfolioTrend: PortfolioValueTrend = {
  currency_code: "CNY",
  points: [
    { date: "2026-06-05", market_value_cents: 100000 },
    { date: "2026-06-12", market_value_cents: 110000 },
    { date: "2026-06-19", market_value_cents: 95000 },
  ],
};

/**
 * 默认布线表：组合走势（动态函数归 overrides）。
 * 参考字典命令走接缝内建兜底，不在此枚举。
 */
const BASE_OVERRIDES: Record<string, InvokeSeamOverride> = {
  portfolio_value_trend: () => Promise.resolve(portfolioTrend),
};

beforeEach(async () => {
  wireInvokeSeam({ overrides: BASE_OVERRIDES });
  const store = useReferenceStore();
  await store.refresh();
});

describe("toTrendRange 预设区间 → 查询区间（纯函数）", () => {
  it("全部 = 不设界（空区间对象）", () => {
    expect(toTrendRange("all", new Date(2026, 7, 27))).toEqual({});
  });

  it("1 月：起点为一个月前同日", () => {
    expect(toTrendRange("1m", new Date(2026, 7, 27))).toEqual({
      start_date: "2026-07-27",
      end_date: null,
    });
  });

  it("3 月：起点为三个月前同日", () => {
    expect(toTrendRange("3m", new Date(2026, 7, 27))).toEqual({
      start_date: "2026-05-27",
      end_date: null,
    });
  });

  it("1 年：起点为一年前同日", () => {
    expect(toTrendRange("1y", new Date(2026, 7, 27))).toEqual({
      start_date: "2025-08-27",
      end_date: null,
    });
  });

  it("月末溢出钳制到目标月最后一日（3-31 减 1 月 → 2-28）", () => {
    expect(toTrendRange("1m", new Date(2026, 2, 31))).toEqual({
      start_date: "2026-02-28",
      end_date: null,
    });
  });

  it("3 年：起点为三年前同日（issue #1907 档位扩展）", () => {
    expect(toTrendRange("3y", new Date(2026, 7, 27))).toEqual({
      start_date: "2023-08-27",
      end_date: null,
    });
  });

  it("5 年：起点为五年前同日（issue #1907 档位扩展）", () => {
    expect(toTrendRange("5y", new Date(2026, 7, 27))).toEqual({
      start_date: "2021-08-27",
      end_date: null,
    });
  });

  it("月末溢出钳制到目标月最后一日（3-31 减 1 月 → 2-28）", () => {
    expect(toTrendRange("1y", new Date(2024, 1, 29))).toEqual({
      start_date: "2023-02-28",
      end_date: null,
    });
  });
});

describe("toTrendChartSeries 点位映射 → 图表数据（纯函数）", () => {
  it("连续周采样映射为 labels + values，label 用真实采样日", () => {
    expect(
      toTrendChartSeries([
        { date: "2026-06-05", value: 100000 },
        { date: "2026-06-12", value: 110000 },
      ]),
    ).toEqual({
      labels: ["2026-06-05", "2026-06-12"],
      values: [100000, 110000],
    });
  });

  it("x 轴按日期连续：缺周生成槽位并填 null（由 spanGaps 连点跨越）", () => {
    expect(
      toTrendChartSeries([
        { date: "2026-06-05", value: 100000 },
        { date: "2026-06-19", value: 95000 },
      ]),
    ).toEqual({
      // 首末点所在周的周一之间逐周生成槽位；中间缺周 label 用周一、值为 null
      labels: ["2026-06-05", "2026-06-08", "2026-06-19"],
      values: [100000, null, 95000],
    });
  });

  it("空点序列 → 空 labels/values", () => {
    expect(toTrendChartSeries([])).toEqual({ labels: [], values: [] });
  });
});

describe("isTrendEmpty 空态判定（纯函数）", () => {
  it("无采样点为空态", () => {
    expect(isTrendEmpty([])).toBe(true);
  });

  it("有采样点非空", () => {
    expect(isTrendEmpty([{ date: "2026-06-05", value: 1 }])).toBe(false);
  });
});

describe("usePortfolioTrend 走势数据层", () => {
  it("默认组合模式：加载组合市值曲线并映射为图表序列", async () => {
    const { refresh, chartSeries, currencyCode, isEmpty } = withSetup(() => usePortfolioTrend());
    await refresh();
    expect(mockInvoke.mock.calls.some(([c]) => c === "portfolio_value_trend")).toBe(true);
    expect(chartSeries.value).toEqual({
      labels: ["2026-06-05", "2026-06-12", "2026-06-19"],
      values: [100000, 110000, 95000],
    });
    expect(currencyCode.value).toBe("CNY");
    expect(isEmpty.value).toBe(false);
  });

  it("区间切换重新拉取：预设起止日期进入查询参数", async () => {
    const { refresh, setPreset } = withSetup(() => usePortfolioTrend());
    await refresh();
    setPreset("3m");
    await refresh();
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "portfolio_value_trend");
    // 初始 refresh + preset 变化触发 watch + 显式 refresh
    expect(calls.length).toBeGreaterThanOrEqual(2);
    const last = calls.at(-1)![1] as { filter: { start_date: string; end_date: string | null } };
    expect(last.filter.end_date).toBeNull();
    expect(last.filter.start_date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("无历史数据：points 为空 → 空态判定为真", async () => {
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        portfolio_value_trend: { currency_code: "CNY", points: [] },
      },
    });
    const { refresh, isEmpty } = withSetup(() => usePortfolioTrend());
    await refresh();
    expect(isEmpty.value).toBe(true);
  });

  it("补全状态读投影直出（issue #1377）：空采样点时暴露给空态渲染", async () => {
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        portfolio_value_trend: {
          currency_code: "CNY",
          points: [],
          backfill: { state: "running", done: 7, total: 20 },
        },
      },
    });
    const { refresh, backfill } = withSetup(() => usePortfolioTrend());
    await refresh();
    expect(backfill.value).toEqual({ state: "running", done: 7, total: 20 });
  });

  it("有采样点时不携带补全状态（后端缺省不序列化，投影恒为 null）", async () => {
    const { refresh, backfill } = withSetup(() => usePortfolioTrend());
    await refresh();
    expect(backfill.value).toBeNull();
  });

  it("加载完成后 loading 复位；命令异常时 loading 同样复位", async () => {
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        portfolio_value_trend: () => Promise.reject(new Error("boom")),
      },
    });
    const { refresh, loading } = withSetup(() => usePortfolioTrend());
    // 挂载自动首刷先吃到拒绝布线并重置去重短路键，冲刷落定后手动 refresh 才会重发
    await flushPromises();
    await expect(refresh()).rejects.toThrow("boom");
    expect(loading.value).toBe(false);
  });
});
