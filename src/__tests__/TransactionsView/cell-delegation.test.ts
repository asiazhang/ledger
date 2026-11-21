import {
  pushMock,
  makeTxn,
  mountView,
  setAccountDb,
  setMerchantDb,
  setTxnDb,
  bodyRows,
  rowMenu,
} from "./common";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { messageApi, messageCalls } from "@ledger/test-support/message-mock";
import { makeAccount, makeMerchant, makeFunding, makePurchase } from "../factories";
import { useReferenceStore } from "@/stores/reference";
import type { Transaction } from "@ledger/types";

/**
 * 表级事件委托（issue #2189 行内高频组件瘦身）：交易表格单元格内的下钻/复制/
 * 行菜单动作经单元格语义属性（data-tx-action）冒泡到表格容器统一分发——本文件
 * 锁定「删除事件委托接线即变红」的可失败路径：动作按钮在场 + 点击经委托产生
 * 用户可观察结果（路由跳转 / 剪贴板写入 + toast）。链接视觉与可点击裁决的
 * 渲染矩阵归 transaction-columns 单测与 Link 组件测试（一缝一测）。
 */

/** 单元格动作按钮查找口（列渲染的稳定钩子：data-tx-action 语义属性）。 */
function actionButtons(wrapper: Awaited<ReturnType<typeof mountView>>, action: string) {
  return wrapper.findAll(`button[data-tx-action="${action}"]`);
}

/** 备注复制按钮（复制通道唯一入口，无备注行不渲染）。 */
function copyNoteButtons(wrapper: Awaited<ReturnType<typeof mountView>>) {
  return actionButtons(wrapper, "copy-note");
}

beforeEach(() => {
  // 剪贴板替身（jsdom 无 clipboard API）：默认成功，失败用例按需覆写
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});

describe("TransactionsView 账户/商户/来源下钻委托（issue #2189）", () => {
  it("出资分解行：首条出资账户可点击下钻（涉及账户过滤视图）", async () => {
    setAccountDb([
      makeAccount({ id: "acc-1", name: "现金", type: "cash" }),
      makeAccount({ id: "acc-2", name: "银行", type: "bank" }),
    ]);
    const row: Partial<Transaction> = {
      account_id: null,
      fundings: [
        makeFunding({ account_id: "acc-2", amount_cents: 3000 }),
        makeFunding({ account_id: "acc-1", amount_cents: 2000 }),
      ],
    };
    setTxnDb([makeTxn(1, "acc-1", row)]);
    const wrapper = await mountView();
    // 分解行只渲染首条出资账户链接（等 2 账户标注在场，首账户可点击）
    const links = actionButtons(wrapper, "account-drill");
    expect(links).toHaveLength(1);
    expect(links[0].text().trim()).toBe("银行");
    expect(links[0].attributes("data-tx-account")).toBe("acc-2");
    await links[0].trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { account: "acc-2" },
    });
  });

  it("黑洞/隐藏账户渲染为不可点击占位文本（不提供落空的跳转）", async () => {
    // is_hidden 账户不在 list_accounts 返回（参考数据 accountMap 不含），渲染 '-'
    setTxnDb([makeTxn(1, "acc-hidden")]);
    const wrapper = await mountView();
    expect(actionButtons(wrapper, "account-drill")).toHaveLength(0);
    expect(bodyRows(wrapper)[0].find('td[data-col-key="account_id"]').text()).toBe("-");
  });

  it("在用与软删商户都可点击下钻；未知商户 id 渲染占位文本不可点击", async () => {
    setMerchantDb([
      makeMerchant({ id: "mch-1", name: "京东" }),
      makeMerchant({ id: "mch-del", name: "已闭店商城", is_deleted: true }),
    ]);
    await useReferenceStore().refresh();
    setTxnDb([
      makeTxn(1, "acc-1", { merchant_id: "mch-1" }),
      makeTxn(2, "acc-1", { merchant_id: "mch-del" }),
      makeTxn(3, "acc-1", { merchant_id: "mch-missing" }),
    ]);
    const wrapper = await mountView();
    const links = actionButtons(wrapper, "merchant-drill");
    expect(links.map((l) => l.text().trim())).toEqual(["京东", "已闭店商城"]);
    await links[1].trigger("click");
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "transactions",
      query: { merchant: "mch-del" },
    });
    // 未知商户：占位文本（不可点击，'-')
    const cells = wrapper.findAll('td[data-col-key="merchant_id"]');
    expect(cells[2].text()).toBe("-");
    expect(cells[2].find("button[data-tx-action]").exists()).toBe(false);
  });

  it("有效来源点击经委托精确跳转：保单落独立路由 + focus 实体定位参数", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        source: { kind: "policy", entity_id: "pol-1", display_name: "重疾险", status: null },
      }),
    ]);
    const wrapper = await mountView();
    const link = actionButtons(wrapper, "source-jump");
    expect(link).toHaveLength(1);
    expect(link[0].attributes("data-tx-source-kind")).toBe("policy");
    expect(link[0].attributes("data-tx-source-entity")).toBe("pol-1");
    await link[0].trigger("click");
    // 保单是出厂收纳成员：落资产「更多」保单页签 + focus（深模块注入谓词后解析）
    expect(pushMock).toHaveBeenLastCalledWith({
      name: "assets-more",
      query: { tab: "policies", focus: "pol-1" },
    });
  });

  it("软删保单来源：占位文本 +「已删除」标注，不可点击", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        source: { kind: "policy", entity_id: "pol-2", display_name: "医疗险", status: "deleted" },
      }),
    ]);
    const wrapper = await mountView();
    expect(actionButtons(wrapper, "source-jump")).toHaveLength(0);
    const cell = wrapper.find('td[data-col-key="source"]');
    expect(cell.text()).toContain("医疗险");
    expect(cell.text()).toContain("已删除");
  });
});

describe("TransactionsView 触控轴单元格形态（issue #2189 分流）", () => {
  it("触控轴：来源类型全称常驻小字（title 不可达的一击可达替代）；指针轴不渲染", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        source: {
          kind: "subscription",
          entity_id: "sub-1",
          display_name: "流媒体会员",
          status: null,
        },
      }),
    ]);
    // 桌面档 + 触控轴（平板横屏形态，宽度轴不动——mountPhone 是移动档卡片，不适用）
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const touch = await mountView();
    const touchLabel = touch.find(".tx-source-type-label");
    expect(touchLabel.exists()).toBe(true);
    expect(touchLabel.text()).toBe("订阅计划");
    // 指针轴基线（显式换档）：类型全称收进悬停 title，不渲染常驻小字
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const pointer = await mountView();
    expect(pointer.find(".tx-source-type-label").exists()).toBe(false);
  });
});

describe("TransactionsView 备注复制委托（issue #2189）", () => {
  it("备注行悬停行内的复制按钮：点击写剪贴板并弹成功提示", async () => {
    setTxnDb([makeTxn(1, "acc-1", { note: "完整备注文本" })]);
    const wrapper = await mountView();
    const buttons = copyNoteButtons(wrapper);
    expect(buttons).toHaveLength(1);
    await buttons[0].trigger("click");
    await flushPromises();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("完整备注文本");
    expect(messageApi.success).toHaveBeenCalledTimes(1);
  });

  it("剪贴板失败：弹失败提示（成功路径不弹 error）", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    setTxnDb([makeTxn(1, "acc-1", { note: "写不进去" })]);
    const wrapper = await mountView();
    const buttons = copyNoteButtons(wrapper);
    await buttons[0].trigger("click");
    await flushPromises();
    expect(messageApi.error).toHaveBeenCalledTimes(1);
    expect(messageCalls().some((c) => c.method === "success")).toBe(false);
  });

  it("无备注行与商品名形态不渲染复制按钮（购买项行只读呈现）", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { note: null }),
      makeTxn(2, "acc-1", {
        purchases: [makePurchase({ name: "进口猫粮", quantity: 2 })],
      }),
    ]);
    const wrapper = await mountView();
    expect(copyNoteButtons(wrapper)).toHaveLength(0);
  });

  it("操作列「⋯」点击（委托）打开与右键同一 RowContextMenu：购买项续行无按钮不可触发", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        purchases: [
          makePurchase({ name: "猫粮", quantity: 1 }),
          makePurchase({ name: "洗衣液", quantity: 1 }),
        ],
      }),
    ]);
    const wrapper = await mountView();
    // 订单块首行一个「⋯」（合并格），续行不渲染重复操作格
    expect(actionButtons(wrapper, "row-menu")).toHaveLength(1);
    await actionButtons(wrapper, "row-menu")[0].trigger("click");
    await flushPromises();
    // 与行右键同一 RowContextMenu 编排（识别按 delete 菜单项，见 common.rowMenu）
    expect(rowMenu(wrapper).props("show")).toBe(true);
  });
});
