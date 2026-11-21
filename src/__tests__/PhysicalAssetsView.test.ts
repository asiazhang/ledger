import { describe, it, expect, beforeEach } from "vitest";
import { assertMobileTierScrollX } from "@ledger/test-support/mobile-scroll-x";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { findButtonByTestId, findBodyButtonByTestId } from "@ledger/test-support/dom";
import { mount, flushPromises, DOMWrapper } from "@vue/test-utils";
import PhysicalAssetsView from "@/views/PhysicalAssetsView.vue";
import PhysicalAssetFormModal from "@/physical-asset/PhysicalAssetFormModal.vue";
import { makePhysicalAsset, makePhysicalAssetList } from "./factories";
import type { Currency, PhysicalAsset, PhysicalAssetList } from "@ledger/types";

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector);
}

function formInput(testid: string) {
  const modal = bodyQuery('[data-testid="physical-asset-form-modal"]')!;
  return new DOMWrapper<HTMLInputElement>(modal.querySelector(`[data-testid="${testid}"] input`));
}

function saveButton() {
  return findBodyButtonByTestId("physical-asset-save")!;
}

const mockCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
  { code: "USD", name: "美元", symbol: "$", decimal_places: 2 },
];

function baseAsset(over: Partial<PhysicalAsset> = {}): PhysicalAsset {
  return makePhysicalAsset({ id: "asset-1", ...over });
}

let list: PhysicalAssetList;

const VIEW_OVERRIDES = {
  list_currencies: mockCurrencies,
  list_physical_assets: () => Promise.resolve(list),
  create_physical_asset: (args?: Record<string, unknown>) => {
    const { input } = args as { input: { name: string; initial_valuation_cents: number } };
    const id = `asset-new-${input.name}`;
    list = makePhysicalAssetList({
      assets: [
        baseAsset({
          id,
          name: input.name,
          current_valuation_cents: input.initial_valuation_cents,
          current_valuation_native_cents: input.initial_valuation_cents,
        }),
      ],
      holding_total_native_cents: input.initial_valuation_cents,
    });
    return Promise.resolve(id);
  },
};

beforeEach(() => {
  list = makePhysicalAssetList();
  wireInvokeSeam({ overrides: VIEW_OVERRIDES });
});

describe("PhysicalAssetsView 实物资产视图冒烟（issue #466）", () => {
  it("挂载即拉取：合计卡显示在持估值合计（折本位币），列表渲染名称/估值/日期/状态", async () => {
    list = makePhysicalAssetList({
      assets: [baseAsset()],
      holding_total_native_cents: 5_000_000,
    });
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    expect(wrapper.find('[data-testid="physical-asset-holding-total"]').text()).toContain("5,0000");
    expect(wrapper.text()).toContain("客厅油画");
    expect(wrapper.text()).toContain("2026-01-01");
    expect(wrapper.text()).toContain("在持");
  });

  it("空列表显示空态引导", async () => {
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    expect(wrapper.find('[data-testid="physical-asset-empty-guide"]').exists()).toBe(true);
  });

  it("点「新建资产」打开建档弹窗：表单字段就位", async () => {
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, "physical-asset-new").trigger("click");
    await flushPromises();
    const modal = bodyQuery('[data-testid="physical-asset-form-modal"]');
    expect(modal).not.toBeNull();
    expect(formInput("physical-asset-name").exists()).toBe(true);
    expect(formInput("physical-asset-valuation").exists()).toBe(true);
  });

  it("建档成功：调用 create_physical_asset 后列表与合计刷新、弹窗关闭", async () => {
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, "physical-asset-new").trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="physical-asset-form-modal"]')).not.toBeNull();
    await formInput("physical-asset-name").setValue("代步车");
    await formInput("physical-asset-valuation").setValue("80000");
    await saveButton().trigger("click");
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_physical_asset");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: {
        name: "代步车",
        initial_valuation_cents: 8_000_000,
        initial_valuation_currency_code: "CNY",
        purchase_price_cents: null,
        purchase_currency_code: null,
      },
    });
    expect(wrapper.text()).toContain("代步车");
    expect(wrapper.find('[data-testid="physical-asset-holding-total"]').text()).toContain("8,0000");
    expect(wrapper.findComponent(PhysicalAssetFormModal).emitted("update:show")).toContainEqual([
      false,
    ]);
  });

  it("点行内「编辑」打开编辑弹窗：预填名称、无估值字段（估值只能经「更新估值」变更，T2）", async () => {
    list = makePhysicalAssetList({ assets: [baseAsset({ name: "代步车" })] });
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, "physical-asset-edit").trigger("click");
    await flushPromises();
    const modal = bodyQuery('[data-testid="physical-asset-form-modal"]');
    expect(modal).not.toBeNull();
    expect(formInput("physical-asset-name").exists()).toBe(true);
    expect(formInput("physical-asset-name").element as HTMLInputElement).toBeTruthy();
    expect(formInput("physical-asset-valuation").exists()).toBe(false);
  });

  it("点行内「更新估值」打开估值弹窗（T2）", async () => {
    list = makePhysicalAssetList({ assets: [baseAsset()] });
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, "physical-asset-update-valuation").trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="physical-asset-valuation-modal"]')).not.toBeNull();
    expect(bodyQuery('[data-testid="physical-asset-valuation-amount"]')).not.toBeNull();
  });

  it("建档缺名称：客户端校验拦截，不调用 create_physical_asset", async () => {
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, "physical-asset-new").trigger("click");
    await flushPromises();
    await formInput("physical-asset-valuation").setValue("80000");
    let createCalls = 0;
    wireInvokeSeam({
      overrides: {
        ...VIEW_OVERRIDES,
        create_physical_asset: () => {
          createCalls++;
          return Promise.resolve("x");
        },
      },
    });
    await saveButton().trigger("click");
    await flushPromises();
    expect(createCalls).toBe(0);
  });
});

describe("PhysicalAssetsView 实物资产弹窗族排版统一（issue #635）", () => {
  function visibleModalCard(): HTMLElement {
    const cards = [...document.querySelectorAll<HTMLElement>(".n-card")].filter((el) => {
      let node: Element | null = el;
      while (node && node !== document.body) {
        if ((node as HTMLElement).style.display === "none") return false;
        node = node.parentElement;
      }
      return true;
    });
    expect(cards, "当前应恰有一个可见弹窗卡片").toHaveLength(1);
    return cards[0];
  }

  function expectModalCard(width: string) {
    const card = visibleModalCard();
    expect(card.style.width).toBe(width);
    expect(card.classList.contains("n-card--bordered")).toBe(false);
  }

  async function openBy(testid: string) {
    list = makePhysicalAssetList({ assets: [baseAsset()] });
    const wrapper = mount(PhysicalAssetsView);
    await flushPromises();
    await findButtonByTestId(wrapper, testid).trigger("click");
    await flushPromises();
    return wrapper;
  }

  it.each([
    ["physical-asset-new", "建档", "480px"],
    ["physical-asset-update-valuation", "更新估值", "420px"],
    ["physical-asset-dispose", "处置", "420px"],
  ])("「%s」弹窗（%s）卡片宽度归对应档位且默认无边框", async (testid, _name, width) => {
    await openBy(testid);
    expectModalCard(width);
  });

  it("估值弹窗：日期提示为表单下方段落式说明，无内联 margin-left 挤占", async () => {
    await openBy("physical-asset-update-valuation");
    const modal = bodyQuery('[data-testid="physical-asset-valuation-modal"]')!;
    const hint = modal.querySelector<HTMLElement>(".form-hint");
    expect(hint?.textContent).toContain("留空 = 今天");
    expect(modal.querySelector<HTMLElement>('[style*="margin-left"]')).toBeNull();
  });

  it("处置弹窗：提示为表单下方段落式说明，无空 label 表单项 hack", async () => {
    await openBy("physical-asset-dispose");
    const modal = bodyQuery('[data-testid="physical-asset-dispose-modal"]')!;
    const hint = modal.querySelector<HTMLElement>(".form-hint");
    expect(hint?.textContent).toContain("已处置");
    const labels = [...modal.querySelectorAll<HTMLElement>(".n-form-item-label")];
    expect(labels.some((el) => el.textContent!.trim() === "")).toBe(false);
  });
});

describe("PhysicalAssetsView 移动档横向滚动下限（issue #849 / ADR-0088 决策 11 票⑨）", () => {
  it("窄屏表格挂 scroll-x = 固定列宽总和（横向滚动吸收窄屏）；桌面档不挂零变化", async () => {
    await assertMobileTierScrollX(() => mount(PhysicalAssetsView));
  });
});
