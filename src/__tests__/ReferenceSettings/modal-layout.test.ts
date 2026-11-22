import { describe, it, expect } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import MerchantEditModal from "@/merchants/MerchantEditModal.vue";
import CategoryEditModal from "@/categories/CategoryEditModal.vue";
import type { Merchant, Category } from "@ledger/types";

const mockMerchant: Merchant = {
  id: "mch-1",
  name: "京东",
  updated_at: "2026-01-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
};

const mockCategory: Category = {
  id: "cat-1",
  name: "餐饮",
  kind: "expense",
  parent_id: null,
  icon: null,
  sort_order: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
};

function modalCard(): HTMLElement {
  const card = document.body.querySelector<HTMLElement>(".n-card");
  expect(card, "弹窗卡片（NCard）应存在").not.toBeNull();
  return card!;
}

function expectCardSizeSm(card: HTMLElement) {
  expect(card.style.width).toBe("420px");
  expect(card.classList.contains("n-card--bordered")).toBe(false);
}

function expectRightAlignedSinglePrimary(card: HTMLElement, text: string) {
  const btn = Array.from(card.querySelectorAll("button")).find(
    (b) => b.textContent?.trim() === text,
  );
  expect(btn, `「${text}」主键应存在`).toBeTruthy();
  expect(btn!.classList.contains("n-button--primary-type"), "保存应为 primary 主键").toBe(true);
  expect(btn!.classList.contains("n-button--block"), "保存不应再是全宽 block").toBe(false);
  const row = btn!.closest(".n-space") as HTMLElement | null;
  expect(row, "保存应包在 NSpace 按钮行内").not.toBeNull();
  expect(row!.style.justifyContent).toBe("flex-end");
}

describe("参考数据管理弹窗排版统一（issue #637）", () => {
  it("商户编辑弹窗卡片归 sm 档、默认无边框，保存键右对齐非全宽", async () => {
    mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    });
    await flushPromises();

    const card = modalCard();
    expectCardSizeSm(card);
    expectRightAlignedSinglePrimary(card, "保存");
  });

  it("分类编辑弹窗卡片归 sm 档、默认无边框，保存键右对齐非全宽", async () => {
    mount(CategoryEditModal, {
      props: { show: true, category: mockCategory },
    });
    await flushPromises();

    const card = modalCard();
    expectCardSizeSm(card);
    expectRightAlignedSinglePrimary(card, "保存");
  });
});
