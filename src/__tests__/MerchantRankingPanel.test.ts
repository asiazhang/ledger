import { describe, it, expect, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import MerchantRankingPanel from "@/reports/MerchantRankingPanel.vue";
import { setFakeMedia } from "@ledger/test-support/media-mock";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}));
import { useReferenceStore } from "@/stores/reference";
import { paletteColor } from "@ledger/utils/category-chart";
import { formatAmount } from "@ledger/money";
import type { Merchant } from "@ledger/types";
import type { MerchantSharesReport } from "@ledger/types";

const mockMerchants: Merchant[] = [
  {
    id: "m1",
    name: "京东",
    is_deleted: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  },
  {
    id: "m2",
    name: "红旗连锁",
    is_deleted: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  },
  {
    id: "m3",
    name: "退款户",
    is_deleted: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  },
];

const rows = [
  { merchant_id: "m1", merchant_name: "京东", amount_cents: 170000, transaction_count: 9 },
  { merchant_id: "m2", merchant_name: "红旗连锁", amount_cents: 85000, transaction_count: 5 },
  { merchant_id: "m3", merchant_name: "退款户", amount_cents: -5000, transaction_count: 1 },
];
const report: MerchantSharesReport = { rows, total_cents: 340000 };

function mountPanel(props: { report?: MerchantSharesReport; topN?: number } = {}) {
  const pinia = createPinia();
  setActivePinia(pinia);
  useReferenceStore().merchants = mockMerchants;
  return mount(MerchantRankingPanel, {
    props: { report: props.report ?? report, topN: props.topN ?? 5 },
    global: { plugins: [pinia] },
  });
}

type Wrapper = ReturnType<typeof mountPanel>;

function bodyRows(wrapper: Wrapper) {
  return wrapper.findAll('[data-testid="merchant-table"] tbody tr');
}

type RowWrapper = ReturnType<typeof bodyRows>[number];

function cellText(row: RowWrapper, testid: string): string {
  return row.find(`[data-testid="${testid}"]`).text();
}

describe("MerchantRankingPanel 表格化（issue #618）", () => {
  it("五列表头齐备且无排名序号列：商户名 | 金额分布 | 金额 | 占比 | 笔数", () => {
    const wrapper = mountPanel();
    const headers = wrapper
      .findAll('[data-testid="merchant-table"] thead th')
      .map((th) => th.text().trim());
    expect(headers).toEqual(["商户名", "金额分布", "金额", "占比", "笔数"]);
  });

  it("行序 = 后端返回序：商户名、金额数字、占比%、笔数逐行渲染", () => {
    const wrapper = mountPanel();
    const trs = bodyRows(wrapper);
    expect(trs).toHaveLength(3);
    expect(cellText(trs[0], "merchant-name")).toBe("京东");
    expect(cellText(trs[1], "merchant-name")).toBe("红旗连锁");
    expect(cellText(trs[2], "merchant-name")).toBe("退款户");
    expect(cellText(trs[0], "merchant-amount")).toBe(formatAmount(170000));
    expect(cellText(trs[1], "merchant-amount")).toBe(formatAmount(85000));
    expect(cellText(trs[2], "merchant-amount")).toBe(formatAmount(-5000));
    expect(cellText(trs[0], "merchant-share")).toBe("50%");
    expect(cellText(trs[1], "merchant-share")).toBe("25%");
    expect(cellText(trs[2], "merchant-share")).toBe("-1%");
    expect(cellText(trs[0], "merchant-count")).toBe("9");
    expect(cellText(trs[1], "merchant-count")).toBe("5");
    expect(cellText(trs[2], "merchant-count")).toBe("1");
  });

  it("内嵌条：条长 = 金额 ÷ 显示区最大金额（width 100%/50%），配色按名次取色", () => {
    const wrapper = mountPanel();
    const trs = bodyRows(wrapper);
    const fill0 = trs[0].find('[data-testid="merchant-bar"]');
    const fill1 = trs[1].find('[data-testid="merchant-bar"]');
    expect(fill0.attributes("style")).toContain("width: 100%");
    expect(fill1.attributes("style")).toContain("width: 50%");
    const rgb = (hex: string) => {
      const n = parseInt(hex.slice(1), 16);
      return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
    };
    expect(fill0.attributes("style")).toContain(rgb(paletteColor(0)));
    expect(fill1.attributes("style")).toContain(rgb(paletteColor(1)));
  });

  it("负净额行不画条（width 0%），金额与占比照实显示", () => {
    const wrapper = mountPanel();
    const fill = bodyRows(wrapper)[2].find('[data-testid="merchant-bar"]');
    expect(fill.attributes("style")).toContain("width: 0%");
    expect(cellText(bodyRows(wrapper)[2], "merchant-amount")).toBe(formatAmount(-5000));
    expect(cellText(bodyRows(wrapper)[2], "merchant-share")).toBe("-1%");
  });

  it("点商户名上报下钻意图：emit drilldown（携带 merchant_id，载荷归报表视图构造）", async () => {
    const wrapper = mountPanel();
    await bodyRows(wrapper)[0].find('[data-testid="merchant-name"]').trigger("click");
    expect(wrapper.emitted("drilldown")).toEqual([["m1"]]);
    await bodyRows(wrapper)[1].find('[data-testid="merchant-name"]').trigger("click");
    expect(wrapper.emitted("drilldown")).toEqual([["m1"], ["m2"]]);
  });

  it("TopN 档位受控渲染与切换上报：emit update:topN", async () => {
    const wrapper = mountPanel({ topN: 10 });
    expect(
      (wrapper.find('[data-testid="merchant-topn-10"] input').element as HTMLInputElement).checked,
    ).toBe(true);
    await wrapper.find('[data-testid="merchant-topn-5"] input').setValue(true);
    expect(wrapper.emitted("update:topN")).toEqual([[5]]);
  });

  it("空排行显示空态提示（NEmpty 保留），不渲染表格", () => {
    const wrapper = mountPanel({ report: { rows: [], total_cents: 0 } });
    expect(wrapper.find('[data-testid="merchant-empty"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="merchant-table"]').exists()).toBe(false);
  });
});

describe("MerchantRankingPanel 移动档堆叠（issue #849 / ADR-0088 决策 11 票⑨）", () => {
  it("窄屏改两列：商户名（副行金额分布条）+ 金额（副行占比 · 笔数），信息零丢失", () => {
    setFakeMedia({ width: 400, hover: "hover", pointer: "fine" });
    const wrapper = mountPanel();
    const headers = wrapper
      .findAll('[data-testid="merchant-table"] thead th')
      .map((th) => th.text().trim());
    expect(headers).toEqual(["商户名", "金额"]);

    const trs = bodyRows(wrapper);
    expect(trs).toHaveLength(3);
    expect(cellText(trs[0], "merchant-name")).toBe("京东");
    expect(trs[0].find('[data-testid="merchant-bar-track"]').exists()).toBe(true);
    expect(trs[0].find('[data-testid="merchant-bar"]').exists()).toBe(true);
    expect(cellText(trs[0], "merchant-amount")).toBe(formatAmount(170000));
    expect(cellText(trs[0], "merchant-share")).toBe("50%");
    expect(cellText(trs[0], "merchant-count")).toBe("9");
    expect(trs[2].find('[data-testid="merchant-bar"]').attributes("style")).toContain("width: 0%");
    expect(cellText(trs[2], "merchant-amount")).toBe(formatAmount(-5000));
  });

  it("窄屏点商户名仍上报下钻意图（跳转载荷仍归报表视图构造，同口径不回归）", async () => {
    setFakeMedia({ width: 400, hover: "hover", pointer: "fine" });
    const wrapper = mountPanel();
    await bodyRows(wrapper)[1].find('[data-testid="merchant-name"]').trigger("click");
    expect(wrapper.emitted("drilldown")).toEqual([["m2"]]);
  });

  it("TopN 档位控件触控轴达标 ≥48px（头部控件同样过基线），指针轴不挂", () => {
    setFakeMedia({ width: 400, hover: "none", pointer: "coarse" });
    const touch = mountPanel();
    expect(touch.find('[data-testid="merchant-topn-5"]').attributes("style")).toContain(
      "min-height: 48px",
    );
    setFakeMedia({ width: 1280, hover: "hover", pointer: "fine" });
    const desktop = mountPanel();
    expect(desktop.find('[data-testid="merchant-topn-5"]').attributes("style") ?? "").not.toContain(
      "min-height",
    );
  });
});
