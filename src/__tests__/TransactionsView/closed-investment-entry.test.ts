// 路由替身经 common.ts 的 vi.mock 注册，必须先于任何直连组件导入（导入顺序即 mock 生效面）
import {
  mountView,
  mountMobile,
  makeTxn,
  setTxnDb,
  openCreateDropdown,
  closeShownModal,
  openCreateFab,
} from "./common";
import { describe, it, expect, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NModal, NSelect } from "naive-ui";
import { resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import TransactionForm from "@/transaction/TransactionForm.vue";
import { availableCreateKinds, isCreateKindAvailable } from "@ledger/utils/create-entry-kinds";

/**
 * 交易页创建闭集收窄为支出/收入/转账（ADR-0135 决策 5 / issue #1782）：买入/卖出的
 * 记一笔入口迁至投资页「明细」页签头部，交易页全部创建入口（桌面下拉、移动悬浮按钮、
 * 裸键）同源收窄，且收窄是无条件的——关闭投资时投资页整页不可达（ADR-0116 决策 4
 * 修订注记：入口语义由整页覆盖），交易页创建闭集不随功能开关变化、重开亦不回添买卖。
 * 主列表行集与类型筛选的可选集不受创建入口收窄影响（ADR-0135 / issue #1783）。
 */

const CREATE_LABELS = ["支出 a", "收入 i", "转账 z", "借出", "借入"];
const FAB_LABELS = ["支出", "收入", "转账"];

function setInvestmentsClosed(closed: boolean) {
  useFeatureToggleStore().setFeatureClosed("investments", closed);
}

function pressKey(key: string) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}

describe("创建闭集判定源（ADR-0135 决策 5 / issue #1782）", () => {
  it("可用类型恒为三通用 kind：expense/income/transfer（不随投资功能开关变化）", () => {
    expect(availableCreateKinds()).toEqual(["expense", "income", "transfer"]);
    setInvestmentsClosed(true);
    expect(availableCreateKinds()).toEqual(["expense", "income", "transfer"]);
    setInvestmentsClosed(false);
    expect(availableCreateKinds()).toEqual(["expense", "income", "transfer"]);
  });

  it("单类型判定：买入/卖出恒不可用（创建入口已迁投资页明细页签），三通用 kind 恒可用", () => {
    expect(isCreateKindAvailable("buy")).toBe(false);
    expect(isCreateKindAvailable("sell")).toBe(false);
    expect(isCreateKindAvailable("expense")).toBe(true);
    expect(isCreateKindAvailable("income")).toBe(true);
    expect(isCreateKindAvailable("transfer")).toBe(true);
  });
});

describe("创建闭集收窄：桌面记一笔下拉与移动悬浮按钮（同一判定源，一处生效两处）", () => {
  it.each<[label: string, closed: boolean]>([
    ["投资开启：下拉只剩支出/收入/转账 + 借贷变体", false],
    ["关闭投资：下拉闭集不受影响（投资页整页不可达覆盖入口语义）", true],
  ])("%s", async (_label, closed) => {
    setInvestmentsClosed(closed);
    const wrapper = await mountView();
    expect(await openCreateDropdown(wrapper)).toEqual(CREATE_LABELS);
  });

  it.each<[label: string, closed: boolean]>([
    ["投资开启：FAB 只剩支出/收入/转账", false],
    ["关闭投资：FAB 闭集不受影响", true],
  ])("%s", async (_label, closed) => {
    setInvestmentsClosed(closed);
    const wrapper = await mountMobile();
    expect(await openCreateFab(wrapper)).toEqual(FAB_LABELS);
  });
});

describe("裸键退役（ADR-0135 决策 5 / issue #1782：交易页 b/s 退役，a/z/i 不变）", () => {
  beforeEach(() => {
    resetOverlays();
  });

  it("b/s 不再触发记一笔弹窗（命中但不可用：原样放行、不吞键）", async () => {
    const wrapper = await mountView();
    pressKey("b");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    pressKey("s");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
  });

  it("a/z/i 照常直达对应类型弹窗", async () => {
    const wrapper = await mountView();
    pressKey("a");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("title")).toBe("记一笔 · 支出");
    expect(wrapper.findComponent(TransactionForm).props("kind")).toBe("expense");
    // 关闭再重开弹窗路径由同一入口覆盖：换 z 直接开转账
    await closeShownModal(wrapper);
    pressKey("z");
    await flushPromises();
    await closeShownModal(wrapper);
    pressKey("i");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("title")).toBe("记一笔 · 收入");
    expect(wrapper.findComponent(TransactionForm).props("kind")).toBe("income");
  });
});

describe("主列表行集与类型收窄不随创建入口收窄变化（ADR-0135 / issue #1783）", () => {
  it("主列表不呈现投资行，类型下拉恒为四通用 kind（呈现面与创建入口正交）", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { kind: "buy" }),
      makeTxn(2, "acc-1", { kind: "sell" }),
      makeTxn(3, "acc-1", { kind: "expense" }),
    ]);
    const wrapper = await mountView();
    // 投资行不在主列表（历史投资行在搜索中仍可达）；「共 N 条」随之收窄
    const text = wrapper.text();
    expect(text).not.toContain("买入");
    expect(text).not.toContain("卖出");
    expect(text).toContain("共 1 条");
    // 类型下拉恒为四通用 kind（创建入口收窄不影响呈现面维度）
    const kindFilter = wrapper
      .findAllComponents(AppSelect)
      .map((select) => select.findComponent(NSelect).props("options") as Array<{ value: string }>)
      .find((options) => options.some((option) => option.value === "expense"));
    expect(kindFilter, "类型筛选应存在").toBeDefined();
    expect(kindFilter!.map((option) => option.value)).toEqual([
      "income",
      "expense",
      "transfer",
      "refund",
    ]);
  });
});
