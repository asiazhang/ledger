import {
  mountView,
  mountViewSync,
  mountMobile,
  cards,
  listCalls,
  lastListFilter,
  tablePagination,
  bodyRows,
  openMenuOnRow,
  selectRowMenu,
} from "./common";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { clickDialogButton } from "@ledger/test-support/dom";
import { describe, it, expect } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NDataTable, NPagination } from "naive-ui";
import { fireProp } from "@ledger/test-support/component-vm";

describe("TransactionsView 服务端分页", () => {
  it("默认以 page=1 page_size=20 查询并渲染「共 N 条」总数", async () => {
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20 });
    expect(wrapper.text()).toContain("共 45 条");
    expect(bodyRows(wrapper).length).toBe(20);
  });

  it("翻页以新的 page 重新查询", async () => {
    const wrapper = await mountView();
    const before = listCalls().length;
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 });
    expect(bodyRows(wrapper).length).toBe(20);
  });

  it("切换页大小以新的 page_size 查询并重置到第 1 页", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    tablePagination(wrapper).onUpdatePageSize(50);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 50 });
    expect(bodyRows(wrapper).length).toBe(45);
  });

  it("删除当前页一条后以当前页码刷新", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    const before = listCalls().length;
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    await clickDialogButton("删除");
    await flushPromises();
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 });
  });

  it("删除当前页最后一条后回退到上一页避免空页", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(3);
    await flushPromises();
    expect(bodyRows(wrapper).length).toBe(5);
    for (let i = 0; i < 4; i++) {
      await openMenuOnRow(wrapper, 0);
      await selectRowMenu(wrapper, "delete");
      await clickDialogButton("删除");
      await flushPromises();
    }
    expect(bodyRows(wrapper).length).toBe(1);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    await clickDialogButton("删除");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 });
    expect(bodyRows(wrapper).length).toBe(20);
  });

  it("查询期间 loading 状态可见", async () => {
    let resolveList!: (v: unknown) => void;
    wireInvokeSeam({
      overrides: {
        list_transactions: () =>
          new Promise((resolve) => {
            resolveList = resolve;
          }),
      },
    });
    const wrapper = mountViewSync();
    await flushPromises();
    expect(wrapper.findComponent(NDataTable).props("loading")).toBe(true);
    resolveList({ items: [], total: 0 });
    await flushPromises();
    expect(wrapper.findComponent(NDataTable).props("loading")).toBe(false);
  });
});

describe("TransactionsView 移动档分页（issue #846 两档一致）", () => {
  it("移动档默认 20/页，翻页与页大小切换与桌面同语义（同一出口）", async () => {
    const wrapper = await mountMobile();
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20 });
    const pagination = wrapper.findComponent(NPagination);
    expect(pagination.exists()).toBe(true);
    fireProp(pagination, "onUpdate:page", 2);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 });
    expect(cards(wrapper).length).toBe(20);
    fireProp(pagination, "onUpdate:pageSize", 50);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 50 });
    expect(cards(wrapper).length).toBe(45);
  });

  it("移动档删末页最后一条自动回退上一页（ADR-0045 页码回退入口两档同源）", async () => {
    const wrapper = await mountMobile();
    fireProp(wrapper.findComponent(NPagination), "onUpdate:page", 3);
    await flushPromises();
    expect(cards(wrapper).length).toBe(5);
    for (let i = 0; i < 5; i++) {
      await cards(wrapper)[0].find(".row-actions-btn").trigger("click");
      await flushPromises();
      await selectRowMenu(wrapper, "delete");
      await clickDialogButton("删除");
      await flushPromises();
    }
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 });
    expect(cards(wrapper).length).toBe(20);
  });
});
