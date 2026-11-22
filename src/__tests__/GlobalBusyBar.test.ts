import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { deferred } from "@ledger/test-support/deferred";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import GlobalBusyBar from "@/components/GlobalBusyBar.vue";
import { trackBusy, resetGlobalBusy } from "@ledger/api/global-busy";

beforeEach(() => {
  vi.useFakeTimers();
  resetGlobalBusy();
});

afterEach(() => {
  vi.useRealTimers();
});

async function mountBusyBar() {
  const d = deferred<void>();
  void trackBusy(d.promise);
  const wrapper = mount(GlobalBusyBar);
  await vi.advanceTimersByTimeAsync(300);
  await flushPromises();
  return { wrapper, resolve: () => d.resolve() };
}

describe("GlobalBusyBar 窗口分级分支（issue #842）", () => {
  it("桌面档：忙碌条渲染，无移动档定位类名", async () => {
    const { wrapper } = await mountBusyBar();
    expect(wrapper.find(".global-busy-bar").exists()).toBe(true);
    expect(wrapper.find(".global-busy-bar.is-mobile-tier").exists()).toBe(false);
  });

  it("缩窗换档：839 → 移动档类名即现；900 → 恢复（定位类名随档位实时切换）", async () => {
    const { wrapper } = await mountBusyBar();
    setFakeMedia({ width: 839 });
    await flushPromises();
    expect(wrapper.find(".global-busy-bar.is-mobile-tier").exists()).toBe(true);
    setFakeMedia({ width: 900 });
    await flushPromises();
    expect(wrapper.find(".global-busy-bar.is-mobile-tier").exists()).toBe(false);
  });
});
