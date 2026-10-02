import {
  mountView,
  mountMobile,
  mountPhone,
  listCalls,
  lastListFilter,
  tablePagination,
} from "./common";
import { describe, it, expect, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { createOverlayToken, resetOverlays } from "@ledger/ui-kit/overlayRegistry";

const HINT_ZH = "[ 上一页 · ] 下一页";

/**
 * 翻页快捷键（issue #1902 / ADR-0140）：裸键 `[` / `]` 步进页码，语义等同点击分页条
 * （以最新页码为基准走既有翻页出口，首/末页越界幂等无操作）。
 * 断言全部对准用户可观察结果（页码 / 重拉 / 提示渲染）；删除视图中的
 * usePaginationShortcuts 接线调用即「] 步进」等用例变红（接线证明，ADR-0087）。
 */
describe("TransactionsView 翻页快捷键", () => {
  beforeEach(() => {
    resetOverlays();
  });

  function pressKey(key: string, init: KeyboardEventInit = {}) {
    window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  }

  it("] 从第 1 页步进到第 2 页并按新页重拉", async () => {
    const wrapper = await mountView();
    pressKey("]");
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(2);
    expect(lastListFilter().page).toBe(2);
  });

  it("[ 回退上一页", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    pressKey("[");
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(1);
    expect(lastListFilter().page).toBe(1);
  });

  it("第一页按 [ 幂等无操作（页码不动、不重拉）", async () => {
    const wrapper = await mountView();
    const callsBefore = listCalls().length;
    pressKey("[");
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(1);
    expect(listCalls().length).toBe(callsBefore);
  });

  it("最后一页按 ] 幂等无操作（45 行 3 页，末页再按不动、不重拉）", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(3);
    await flushPromises();
    const callsBefore = listCalls().length;
    pressKey("]");
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(3);
    expect(listCalls().length).toBe(callsBefore);
  });

  it("焦点在输入框时不触发", async () => {
    const wrapper = await mountView();
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "]", bubbles: true }));
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(1);
    input.remove();
  });

  it("弹层打开时不触发（Overlay Suppression）", async () => {
    const wrapper = await mountView();
    const token = createOverlayToken("modal");
    token.set(true);
    pressKey("]");
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(1);
    token.set(false);
  });

  it("带修饰键不触发（组合键让给系统与视图快捷键）", async () => {
    const wrapper = await mountView();
    pressKey("]", { metaKey: true });
    pressKey("[", { ctrlKey: true });
    await flushPromises();
    expect(tablePagination(wrapper).page).toBe(1);
  });

  it("触控轴不绑监听（按键不翻页）", async () => {
    await mountPhone();
    const callsBefore = listCalls().length;
    pressKey("]");
    await flushPromises();
    expect(listCalls().length).toBe(callsBefore);
  });

  it("指针轴渲染键位提示（桌面档与移动档分页条旁同源）", async () => {
    const desktop = await mountView();
    expect(desktop.text()).toContain(HINT_ZH);
    const mobile = await mountMobile();
    expect(mobile.text()).toContain(HINT_ZH);
  });

  it("触控轴不渲染提示", async () => {
    const wrapper = await mountPhone();
    expect(wrapper.text()).not.toContain(HINT_ZH);
  });
});
