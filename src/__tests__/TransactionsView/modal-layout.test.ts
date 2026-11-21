import { describe, it, expect } from "vitest";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { mountView, openMenuOnRow, selectRowMenu } from "./common";

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

async function openCreateModal(wrapper: VueWrapper) {
  const btn = wrapper.findAll("button").find((b) => b.text().includes("记一笔"))!;
  await btn.trigger("click");
  await flushPromises();
}

async function openRowModal(wrapper: VueWrapper, key: "refund" | "add-item" | "edit") {
  await openMenuOnRow(wrapper, 0);
  await selectRowMenu(wrapper, key);
}

describe("TransactionsView 交易弹窗族排版统一（issue #632）", () => {
  it.each<[key: "create" | "refund" | "edit" | "add-item", width: string]>([
    ["create", "480px"],
    ["refund", "480px"],
    ["edit", "480px"],
    ["add-item", "420px"],
  ])("「%s」弹窗卡片宽度归对应档位且默认无边框", async (key, width) => {
    const wrapper = await mountView();
    if (key === "create") {
      await openCreateModal(wrapper);
    } else {
      await openRowModal(wrapper, key);
    }
    expectModalCard(width);
  });
});
