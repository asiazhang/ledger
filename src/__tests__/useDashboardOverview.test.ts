import { describe, it, expect, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { withSetup } from "@ledger/test-support/mount";
import { useReferenceStore } from "@/stores/reference";
import { useDashboardOverview } from "@/dashboard/useDashboardOverview";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink, makeOverview, resetToastSink } from "./factories";

const mockOverview = makeOverview({ net_worth_cents: 1234567, accounts_balance_cents: 1000000 });

const BASE_DEFAULTS = { dashboard_overview: mockOverview };

beforeEach(async () => {
  wireInvokeSeam({ defaults: BASE_DEFAULTS });
  resetToastSink();
  const store = useReferenceStore();
  await store.refresh();
});

describe("useDashboardOverview 首页净资产数据层（issue #143）", () => {
  it("加载 dashboard_overview 并装配出总览数据", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    const { overview, loading, error, refresh } = withSetup(() => useDashboardOverview());
    await refresh();
    expect(loading.value).toBe(false);
    expect(error.value).toBeNull();
    expect(overview.value).toEqual(mockOverview);
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "dashboard_overview")).toBe(true);
    expect(sink.error).not.toHaveBeenCalled();
  });

  it("命令报错（如缺汇率）时进入兜底状态：overview 置空、error 带后端中文错误信息，不抛异常", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error("缺少 USD→CNY 汇率，无法折算")),
      },
    });
    const sink = makeFakeSink();
    registerToastSink(sink);
    const { overview, loading, error } = withSetup(() => useDashboardOverview());
    await flushPromises();
    expect(loading.value).toBe(false);
    expect(overview.value).toBeNull();
    expect(error.value).toBe("缺少 USD→CNY 汇率，无法折算");
    expect(sink.error).toHaveBeenCalledTimes(1);
    expect(sink.error).toHaveBeenCalledWith("缺少 USD→CNY 汇率，无法折算");
  });

  it("非 Error 抛出值（如 Tauri 字符串错误）也能兜底为文案", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { dashboard_overview: () => Promise.reject("缺汇率") },
    });
    const { error, refresh } = withSetup(() => useDashboardOverview());
    await refresh();
    expect(error.value).toBe("缺汇率");
  });

  it("成功后再次报错：overview 清空并切换到错误态；再次成功则恢复", async () => {
    const { overview, error, refresh } = withSetup(() => useDashboardOverview());
    await refresh();
    expect(overview.value).not.toBeNull();

    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error("缺少 HKD→CNY 汇率")),
      },
    });
    await refresh();
    expect(overview.value).toBeNull();
    expect(error.value).toBe("缺少 HKD→CNY 汇率");

    wireInvokeSeam({ defaults: BASE_DEFAULTS });
    await refresh();
    expect(overview.value).toEqual(mockOverview);
    expect(error.value).toBeNull();
  });

  it("失败 toast 只在失败那次弹出：错误态↔成功态往返中 sink 各就各位", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        dashboard_overview: () => Promise.reject(new Error("缺少 HKD→CNY 汇率")),
      },
    });
    const { refresh } = withSetup(() => useDashboardOverview());
    await flushPromises();
    expect(sink.error).toHaveBeenCalledTimes(1);

    wireInvokeSeam({ defaults: BASE_DEFAULTS });
    await refresh();
    expect(sink.error).toHaveBeenCalledTimes(1);
  });
});
