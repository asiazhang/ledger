import { describe, it, expect, vi, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import ItemsView from "@/views/ItemsView.vue";
import type { ItemWithDailyCost } from "@ledger/types";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}));

const mockItem: ItemWithDailyCost = {
  id: "item-1",
  name: "手机",
  purchase_date: "2025-01-01",
  total_cost_cents: 1_000_000,
  currency_code: "CNY",
  cost_native_cents: 1_000_000,
  status: "in_use",
  disposal_date: null,
  residual_value_cents: null,
  purchase_transaction_id: null,
  note: null,
  created_at: "2025-01-01T00:00:00Z",
  updated_at: "2025-01-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
  used_days: 1000,
  numerator_cents: 1_000_000,
  per_day_cents: 1000,
};

beforeEach(() => {
  wireInvokeSeam({
    defaults: {
      list_transactions: { items: [], total: 0 },
      list_items: [mockItem],
    },
  });
});

function modalCard(testid: string): HTMLElement {
  const modal = document.body.querySelector<HTMLElement>(`[data-testid="${testid}"]`);
  expect(modal, `应能找到弹窗 [${testid}]`).not.toBeNull();
  expect(modal!.classList.contains("n-card"), "弹窗根应为 NCard 卡片").toBe(true);
  return modal!;
}

function expectCardSize(card: HTMLElement, width: string) {
  expect(card.style.width).toBe(width);
  expect(card.classList.contains("n-card--bordered")).toBe(false);
}

async function openModalByButton(wrapper: ReturnType<typeof mount>, text: string) {
  const btn = wrapper.findAll("button").find((b) => b.text() === text);
  expect(btn, `应能找到「${text}」按钮`).toBeTruthy();
  await btn!.trigger("click");
  await flushPromises();
}

describe("ItemsView 物品弹窗族排版统一（issue #634）", () => {
  it.each([
    ["编辑", "item-edit-modal", "420px"],
    ["处置", "item-dispose-modal", "420px"],
    ["详情", "item-detail-modal", "480px"],
  ])("「%s」弹窗卡片宽度归对应档位且默认无边框", async (buttonText, testid, width) => {
    const wrapper = mount(ItemsView);
    await flushPromises();

    await openModalByButton(wrapper, buttonText);
    expectCardSize(modalCard(testid), width);
  });
});
