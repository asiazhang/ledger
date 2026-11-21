import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { deferred } from "@ledger/test-support/deferred";
import {
  mockInvoke,
  wireInvokeSeam,
  type AppInvokeHandler,
} from "@ledger/test-support/invoke-mock";
import { api } from "@ledger/api";
import { busyVisible, resetGlobalBusy } from "@ledger/api/global-busy";

beforeEach(() => {
  vi.useFakeTimers();
  resetGlobalBusy();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("globalBusy 全局忙碌状态模块（issue #500，统一 invoke 封装收口）", () => {
  it("在途 IO 超过 300ms 阈值后忙碌条可见，计数归零即隐藏", async () => {
    const d = deferred<void>();
    wireInvokeSeam({ overrides: { list_currencies: () => d.promise } });
    const pending = api.listCurrencies();

    expect(busyVisible.value).toBe(false);
    await vi.advanceTimersByTimeAsync(299);
    expect(busyVisible.value).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(busyVisible.value).toBe(true);

    d.resolve();
    await pending;
    expect(busyVisible.value).toBe(false);
  });

  it("阈值内结束的快操作从不显示忙碌条", async () => {
    wireInvokeSeam();
    await api.listCurrencies();
    await vi.advanceTimersByTimeAsync(1000);
    expect(busyVisible.value).toBe(false);
  });

  it("并发调用聚合计数：部分完成仍显示，全部完成才隐藏", async () => {
    const a = deferred<void>();
    const b = deferred<void>();
    wireInvokeSeam({
      overrides: {
        list_currencies: () => a.promise,
        list_accounts: () => b.promise,
      },
    });
    const p1 = api.listCurrencies();
    const p2 = api.listAccounts();

    await vi.advanceTimersByTimeAsync(300);
    expect(busyVisible.value).toBe(true);

    a.resolve();
    await p1;
    expect(busyVisible.value).toBe(true);

    b.resolve();
    await p2;
    expect(busyVisible.value).toBe(false);
  });

  it("reject 路径正常递减：条不卡死，错误契约不变照常上抛", async () => {
    const d = deferred<void>();
    wireInvokeSeam({ overrides: { list_currencies: () => d.promise } });
    const pending = api.listCurrencies();

    await vi.advanceTimersByTimeAsync(300);
    expect(busyVisible.value).toBe(true);

    d.reject({ kind: "internal", message: "炸了" });
    await expect(pending).rejects.toEqual({ kind: "internal", message: "炸了" });
    expect(busyVisible.value).toBe(false);

    const d2 = deferred<void>();
    wireInvokeSeam({ overrides: { list_currencies: () => d2.promise } });
    const pending2 = api.listCurrencies();
    await vi.advanceTimersByTimeAsync(300);
    expect(busyVisible.value).toBe(true);
    d2.resolve();
    await pending2;
    expect(busyVisible.value).toBe(false);
  });

  it("invoke 替身返回裸值（非 thenable）也纳入聚合计数", async () => {
    mockInvoke.mockImplementation((() => []) as unknown as AppInvokeHandler);
    await api.listCurrencies();
    await vi.advanceTimersByTimeAsync(1000);
    expect(busyVisible.value).toBe(false);

    const d = deferred<void>();
    mockInvoke.mockReturnValue(d.promise);
    const pending = api.listAccounts();
    await vi.advanceTimersByTimeAsync(300);
    expect(busyVisible.value).toBe(true);
    d.resolve();
    await pending;
    expect(busyVisible.value).toBe(false);
  });

  it("重叠在途窗口聚合：快调用与慢调用重叠使聚合窗口持续在途，跨阈值即点亮", async () => {
    const slow = deferred<void>();
    wireInvokeSeam({
      overrides: {
        list_currencies: () => slow.promise,
        list_accounts: [],
      },
    });
    const pSlow = api.listCurrencies();

    await vi.advanceTimersByTimeAsync(100);
    const pFast = api.listAccounts();
    await pFast;
    expect(busyVisible.value).toBe(false);

    await vi.advanceTimersByTimeAsync(200);
    expect(busyVisible.value).toBe(true);

    slow.resolve();
    await pSlow;
    expect(busyVisible.value).toBe(false);
  });
});
