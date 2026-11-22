import { describe, it, expect, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { defineComponent, h } from "vue";
import { NSelect } from "naive-ui";
import {
  routeMock,
  makeTxn,
  setTxnDb,
  mountView,
  listCalls,
  lastListFilter,
  bodyRows,
  tablePagination,
  pushMock,
} from "./common";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { createOverlayToken, resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { fireViewReset, clearViewResets } from "@/composables/viewResetRegistry";
import type { VueWrapper } from "@vue/test-utils";

async function setAccount(wrapper: VueWrapper, id: string | null) {
  wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", id);
  await flushPromises();
}

function defaultDb(count = 45) {
  return Array.from({ length: count }, (_, i) => makeTxn(i + 1, i % 2 === 0 ? "acc-2" : "acc-1"));
}

beforeEach(() => {
  resetOverlays();
  clearViewResets();
});

describe("TransactionsView 会话内保留（issue #893）：同会话卸载重挂恢复，新会话冷启动", () => {
  it("筛选 + 翻页后卸载重挂（同一会话）：以恢复状态现拉，离开期间新账进来即见，不写回 URL", async () => {
    const first = await mountView();
    await setAccount(first, "acc-1");
    tablePagination(first).onChange(2);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({
      page: 2,
      page_size: 20,
      involving_account_id: "acc-1",
    });
    first.unmount();

    setTxnDb(defaultDb(46));
    mockInvoke.mockClear();
    const second = await mountView();
    expect(lastListFilter()).toMatchObject({
      page: 2,
      page_size: 20,
      involving_account_id: "acc-1",
    });
    expect(second.text()).toContain("共 23 条");
    expect(bodyRows(second).length).toBe(3);
    expect(pushMock).not.toHaveBeenCalled();
    second.unmount();
  });

  it("新 pinia 表达冷启动：回默认无筛选态、第 1 页", async () => {
    const first = await mountView();
    await setAccount(first, "acc-1");
    tablePagination(first).onChange(2);
    await flushPromises();
    first.unmount();

    setActivePinia(createPinia());
    mockInvoke.mockClear();
    const second = await mountView();
    const f = lastListFilter();
    expect(f).toMatchObject({ page: 1, page_size: 20 });
    expect(f).not.toHaveProperty("involving_account_id");
    expect(second.text()).toContain("共 45 条");
    second.unmount();
  });

  it("会话内保留零持久化：选择筛选与卸载重挂全程 localStorage 零写入", async () => {
    const first = await mountView();
    const keysBefore = Object.keys(localStorage);
    await setAccount(first, "acc-1");
    tablePagination(first).onChange(2);
    await flushPromises();
    first.unmount();
    const second = await mountView();
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: "acc-1" });
    expect(Object.keys(localStorage)).toEqual(keysBefore);
    second.unmount();
  });

  it("URL 下钻参数在场永远赢：覆盖保留态对应维度；无参数回访恢复离开时的选择（两种往返互不干扰）", async () => {
    const first = await mountView();
    await setAccount(first, "acc-1");
    tablePagination(first).onChange(2);
    await flushPromises();
    first.unmount();

    routeMock.query = { account: "acc-2" };
    const second = await mountView();
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: "acc-2",
    });
    expect(second.text()).toContain("共 23 条");
    second.unmount();

    routeMock.query = {};
    setTxnDb(defaultDb());
    mockInvoke.mockClear();
    const third = await mountView();
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: "acc-2",
    });
    expect(wrapperText(third)).toContain("共 23 条");
    third.unmount();
  });

  it("页码恢复钳制：恢复页码超出当前数据有效范围时回落，不落空页（走既有回退出口）", async () => {
    const first = await mountView();
    tablePagination(first).onChange(3);
    await flushPromises();
    expect(bodyRows(first).length).toBe(5);
    first.unmount();

    setTxnDb(defaultDb(20));
    mockInvoke.mockClear();
    const second = await mountView();
    const pages = listCalls().map(
      ([, args]) => (args as { filter: { page?: number } }).filter.page,
    );
    expect(pages[0]).toBe(3);
    expect(pages[pages.length - 1]).toBe(1);
    expect(bodyRows(second).length).toBe(20);
    expect(tablePagination(second).page).toBe(1);
    second.unmount();
  });
});

describe("TransactionsView ESC 复位接线（spec #892）：注册 → 守卫消费 → 清除保留态本身", () => {
  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard();
        return () => h("div");
      },
    });
    return mount(Host);
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  }

  it("无弹层 ESC 触发模块复位出口：清全部过滤 + 翻页归零 + 重拉；复位后离开再回来 = 默认", async () => {
    const guard = mountGuardHost();
    const first = await mountView();
    await setAccount(first, "acc-1");
    tablePagination(first).onChange(2);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: "acc-1" });

    fireEscape();
    await flushPromises();
    const f = lastListFilter();
    expect(f).toMatchObject({ page: 1, page_size: 20 });
    expect(f).not.toHaveProperty("involving_account_id");
    first.unmount();

    mockInvoke.mockClear();
    const second = await mountView();
    const f2 = lastListFilter();
    expect(f2).toMatchObject({ page: 1 });
    expect(f2).not.toHaveProperty("involving_account_id");
    expect(second.text()).toContain("共 45 条");
    second.unmount();
    guard.unmount();
  });

  it("无过滤时 ESC 幂等：不产生重拉（「无保留状态无操作」覆盖 ESC 通道）", async () => {
    const guard = mountGuardHost();
    const first = await mountView();
    await flushPromises();
    const before = listCalls().length;
    fireEscape();
    await flushPromises();
    expect(listCalls().length).toBe(before);
    first.unmount();
    guard.unmount();
  });

  it("有弹层时 ESC 不复位：弹层库默认关闭行为接管，保留态不动（一次按键只做一件事）", async () => {
    const guard = mountGuardHost();
    const first = await mountView();
    await setAccount(first, "acc-1");
    await flushPromises();
    const token = createOverlayToken("modal");
    token.set(true);
    fireEscape();
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-1" });
    token.set(false);
    first.unmount();
    guard.unmount();
  });

  it("视图卸载后复位注册自动撤销：守卫消费不再触达本视图（导航离开不滞留注册态）", async () => {
    const guard = mountGuardHost();
    const first = await mountView();
    expect(fireViewReset()).toBe(true);
    await setAccount(first, "acc-1");
    await flushPromises();
    first.unmount();
    expect(fireViewReset()).toBe(false);
    guard.unmount();
  });
});

function wrapperText(wrapper: VueWrapper): string {
  return wrapper.text();
}
