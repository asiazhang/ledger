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
    const text = wrapper.text();
    expect(text).not.toContain("买入");
    expect(text).not.toContain("卖出");
    expect(text).toContain("共 1 条");
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
