import { describe, it, expect } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NDropdown, NModal } from "naive-ui";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { mountView, rowMenu, rowMenuKeys } from "./common";

function pressKey(key: string) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}

type ViewWrapper = Awaited<ReturnType<typeof mountView>>;

function createKindDropdown(wrapper: ViewWrapper) {
  return wrapper
    .findAllComponents(NDropdown)
    .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "expense"))!;
}

function createKindLabels(wrapper: ViewWrapper): string[] {
  return (createKindDropdown(wrapper).props("options") as Array<{ key?: string; label?: string }>)
    .filter((o) => o.key !== "create-lending-divider")
    .map((o) => o.label ?? "");
}

describe("TransactionsView 触控交互轴（issue #843 两轴对比）", () => {
  it("触控轴：记一笔裸键不绑定监听（按键不开弹窗）", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const wrapper = await mountView();
    pressKey("a");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
  });

  it("指针轴：裸键行为不变（a 直达支出弹窗）", async () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const wrapper = await mountView();
    pressKey("a");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(true);
    expect(wrapper.findComponent(NModal).props("title")).toBe("记一笔 · 支出");
  });

  it("触控轴：记一笔下拉键位标注退役；指针轴照渲染（两轴对比）", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const touch = await mountView();
    expect(createKindLabels(touch)).toEqual(["支出", "收入", "转账", "借出", "借入"]);

    setFakeMedia({ hover: "hover", pointer: "fine" });
    const pointer = await mountView();
    expect(createKindLabels(pointer)).toEqual(["支出 a", "收入 i", "转账 z", "借出", "借入"]);
  });

  it("触控轴：「⋯」常显可点，点开与右键同一菜单（edit/refund/add-item/delete）", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const wrapper = await mountView();
    await wrapper.findAll(".row-actions-btn")[0].trigger("click");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "refund", "add-item", "menu-divider", "delete"]);
  });

  it("触控轴：金额点按查看全文（点按气泡）；指针轴点击无气泡（行为不变）", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const touch = await mountView();
    await touch.find(".amount-cell").trigger("click");
    await flushPromises();
    const touchPopover = document.body.querySelector(".n-popover");
    expect(touchPopover).not.toBeNull();
    expect(touchPopover!.textContent).toContain(touch.find(".amount-cell").text());

    setFakeMedia({ hover: "hover", pointer: "fine" });
    const pointer = await mountView();
    await pointer.find(".amount-cell").trigger("click");
    await flushPromises();
    expect(document.body.querySelector(".n-popover")).toBeNull();
  });
});
