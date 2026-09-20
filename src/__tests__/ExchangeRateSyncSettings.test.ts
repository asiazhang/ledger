import { describe, it, expect } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NButton } from "naive-ui";
import ExchangeRateSyncSettings from "@/settings/ExchangeRateSyncSettings.vue";

/**
 * 汇率同步卡片组件测试（issue #1545）：invoke 测试接缝布线（ADR-0085）。
 * 三态断言对准用户可观察结果（ADR-0087 断言强度）：
 * - 成功：呈现覆盖区间 / 条数，按钮恢复可点；
 * - 进行中：按钮禁用（真实浏览器不向禁用按钮派发点击，即「不可重复触发」的机制）；
 * - 失败：失败原因按码本地化（errorMessage），「数据源不可达」与「该来源无数据」可分辨。
 */

/** 命令契约静态快照：一次正常落库报告（ECB 单日多周点形态的合理取值）。 */
const REPORT = {
  pairs: 10,
  points: 30,
  earliest: "2026-06-27",
  latest: "2026-09-18",
  manual_protected: 0,
};

function mountCard() {
  return mount(ExchangeRateSyncSettings);
}

function syncButton(wrapper: ReturnType<typeof mountCard>) {
  return wrapper.findComponent(NButton);
}

describe("ExchangeRateSyncSettings.vue — 设置页「同步汇率」入口（issue #1545）", () => {
  it("渲染汇率同步卡片与手动触发按钮", () => {
    wireInvokeSeam();
    const wrapper = mountCard();
    expect(wrapper.text()).toContain("同步汇率");
    expect(wrapper.text()).toContain("立即同步");
  });

  it("点击同步调 sync_exchange_rates，成功后呈现覆盖区间 / 条数并恢复可点", async () => {
    wireInvokeSeam({ defaults: { sync_exchange_rates: REPORT } });
    const wrapper = mountCard();
    await syncButton(wrapper).trigger("click");
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([c]) => c === "sync_exchange_rates")).toBe(true);
    expect(wrapper.text()).toContain("同步完成");
    expect(wrapper.text()).toContain("30");
    expect(wrapper.text()).toContain("2026-06-27");
    expect(wrapper.text()).toContain("2026-09-18");
    expect(syncButton(wrapper).props("disabled")).toBe(false);
  });

  it("进行中不可重复触发：在途时按钮禁用，收尾后恢复", async () => {
    let release!: () => void;
    const inFlight = new Promise<typeof REPORT>((resolve) => {
      release = () => resolve(REPORT);
    });
    wireInvokeSeam({ overrides: { sync_exchange_rates: () => inFlight } });
    const wrapper = mountCard();

    const click = syncButton(wrapper).trigger("click");
    await flushPromises();
    expect(syncButton(wrapper).props("disabled")).toBe(true);
    expect(syncButton(wrapper).props("loading")).toBe(true);

    release();
    await click;
    await flushPromises();
    expect(syncButton(wrapper).props("disabled")).toBe(false);
    expect(wrapper.text()).toContain("同步完成");
  });

  it("失败原因按码本地化且可分辨：数据源不可达 ≠ 该来源无数据", async () => {
    // mock 的 message 刻意不同于 errors.json 模板文案：断言的是模板文本（按码
    // 查表插值路径），message 与模板同文时测试无法区分「按码本地化」与「透传」。
    wireInvokeSeam({
      overrides: {
        sync_exchange_rates: () =>
          Promise.reject({
            kind: "Invalid",
            code: "fx.source-unreachable",
            message: "后端原文（与模板不同文）：连接超时",
          }),
      },
    });
    const wrapper = mountCard();
    await syncButton(wrapper).trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("汇率数据源暂时不可达");
    expect(wrapper.text()).not.toContain("连接超时");

    // 换成「该来源无数据」码：就地错误位呈现另一句可分辨的原因。
    wireInvokeSeam({
      overrides: {
        sync_exchange_rates: () =>
          Promise.reject({
            kind: "Invalid",
            code: "fx.source-no-data",
            message: "后端原文（与模板不同文）：零记录",
          }),
      },
    });
    await syncButton(wrapper).trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("没有可用的汇率数据");
    expect(wrapper.text()).not.toContain("零记录");
    expect(wrapper.text()).not.toContain("暂时不可达");
  });
});
