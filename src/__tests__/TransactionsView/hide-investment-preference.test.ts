// 路由替身经 common.ts 的 vi.mock 注册，必须先于任何直连组件导入（导入顺序即 mock 生效面）
import {
  makeTxn,
  setTxnDb,
  routeMock,
  setAccountDb,
  mountView,
  mountMobile,
  lastListFilter,
  cards,
} from "./common";
import { describe, it, expect } from "vitest";
import { defineComponent, h, nextTick } from "vue";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import { applyLocale } from "@ledger/i18n";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { makeAccount } from "../factories";
import type { Transaction } from "@ledger/types";

/**
 * 「隐藏投资相关流水」视图偏好接线（issue #1811 / ADR-0136）：
 * - 开关落交易页筛选栏，桌面 / 移动同构（ADR-0136 决策 5），文案经 i18n；
 * - 开关开启 → 主列表请求携带 `hide_investment_related=true`（调用事实）+ 行集与
 *   total 一步可观察收窄（外部可见效果）——双断言；删除 TransactionsView load()
 *   请求装配中的偏好接线调用点，本文件「调用事实」断言变红（ADR-0087 接线型负向
 *   条目）；偏好是行集先决收窄，与既有筛选 AND 组合，账户下钻不让位；
 * - 偏好与筛选分家（ADR-0136 决策 1）：清除筛选与 ESC 复位不触达开关，开关状态
 *   不参与清除筛选禁用态判定。
 * 偏好 store 本体（解析防御 / 跨启动保持 / 零后端调用）见
 * transaction-view-preferences.test.ts。
 */

const INVESTMENT_ACCOUNT = makeAccount({ id: "acc-inv", name: "证券", type: "investment" });

/** 混合行集：日常支出（保留）+ 充值转账（银行卡 → 投资账户，主列表通用 kind）+ 投资账户直接支付的费用支出；
 * extra 追加用例自有行（如 acc-1 日常支出，构造「下钻 ∩ 偏好」非空）。 */
function seedMixedRows(...extra: Transaction[]) {
  setAccountDb([
    makeAccount({ id: "acc-1", name: "现金", type: "cash" }),
    makeAccount({ id: "acc-2", name: "银行", type: "bank" }),
    INVESTMENT_ACCOUNT,
  ]);
  setTxnDb([
    makeTxn(1, "acc-2", { kind: "expense", date: "2026-01-05" }),
    makeTxn(2, "acc-1", {
      kind: "transfer",
      date: "2026-01-06",
      to_account_id: "acc-inv",
    }),
    makeTxn(3, "acc-inv", { kind: "expense", date: "2026-01-07" }),
    ...extra,
  ]);
}

/** 偏好开关（筛选栏唯一开关，aria-checked 即对外状态）。 */
function hideSwitch(wrapper: VueWrapper) {
  return wrapper.find(".n-switch");
}

async function toggleHide(wrapper: VueWrapper) {
  await hideSwitch(wrapper).trigger("click");
  await flushPromises();
}

/** 清除筛选按钮（工具栏一个；空态按钮仅在过滤无结果时渲染）。 */
function clearButton(wrapper: VueWrapper) {
  return wrapper.findAll("button").find((b) => b.text().includes("清除筛选"))!;
}

describe("开关在场（桌面 / 移动同构，ADR-0136 决策 5）", () => {
  it("桌面档：筛选栏开关在场，文案经 i18n，默认关", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("隐藏投资相关流水");
    expect(hideSwitch(wrapper).attributes("aria-checked")).toBe("false");
  });

  it("移动档：同一开关同构在场（不做双渲染），默认关", async () => {
    const wrapper = await mountMobile();
    expect(wrapper.text()).toContain("隐藏投资相关流水");
    expect(hideSwitch(wrapper).attributes("aria-checked")).toBe("false");
  });

  it("文案经 i18n：切 en-US 后开关文案随界面语言（还原 zh-CN 避免污染单例语言状态）", async () => {
    const wrapper = await mountView();
    await applyLocale("en-US");
    await nextTick();
    await nextTick();
    expect(wrapper.text()).toContain("Hide investment-related entries");
    await applyLocale("zh-CN");
    await nextTick();
    expect(wrapper.text()).toContain("隐藏投资相关流水");
  });
});

describe("开关 → 主列表请求携带隐藏参数 + 行集收窄（双断言；删除 load 装配偏好接线即红）", () => {
  it("开启：请求携带 hide_investment_related=true（调用事实），共 3 条 → 共 1 条（外部可见）", async () => {
    seedMixedRows();
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("共 3 条");
    await toggleHide(wrapper);
    // 调用事实断言（接线型负向条目）：删除主列表请求装配中的偏好接线调用点即红
    expect(lastListFilter().hide_investment_related).toBe(true);
    // 外部可见效果断言：行集与 total 同步收窄（充值转账与投资账户支付行消失）
    expect(wrapper.text()).toContain("共 1 条");
    expect(wrapper.text()).not.toContain("备注 2");
  });

  it("关闭：请求缺省不携带参数（契约只增、缺省行为不变），随时关掉回全量", async () => {
    seedMixedRows();
    const wrapper = await mountView();
    await toggleHide(wrapper);
    await toggleHide(wrapper);
    expect(lastListFilter().hide_investment_related).toBeUndefined();
    expect(wrapper.text()).toContain("共 3 条");
  });

  it("移动档同构：开启后卡片列表同步收窄", async () => {
    seedMixedRows();
    const wrapper = await mountMobile();
    expect(cards(wrapper)).toHaveLength(3);
    await toggleHide(wrapper);
    expect(lastListFilter().hide_investment_related).toBe(true);
    expect(cards(wrapper)).toHaveLength(1);
  });

  it("与既有筛选 AND 组合（行集 = 偏好先决收窄 ∩ 筛选）：账户过滤在收窄后行集上照常生效", async () => {
    seedMixedRows();
    seedMixedRows(makeTxn(4, "acc-1", { kind: "expense", date: "2026-01-08" }));
    const wrapper = await mountView();
    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "acc-1");
    await flushPromises();
    expect(wrapper.text()).toContain("共 2 条");
    await toggleHide(wrapper);
    expect(lastListFilter()).toMatchObject({
      involving_account_id: "acc-1",
      hide_investment_related: true,
    });
    expect(wrapper.text()).toContain("共 1 条");
  });

  it("URL 账户下钻同样不让位（ADR-0136 决策 3）：?account= 入口与手动过滤同口径，偏好照常收窄", async () => {
    seedMixedRows(makeTxn(4, "acc-1", { kind: "expense", date: "2026-01-08" }));
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-1" });
    expect(wrapper.text()).toContain("共 2 条");
    await toggleHide(wrapper);
    expect(lastListFilter()).toMatchObject({
      involving_account_id: "acc-1",
      hide_investment_related: true,
    });
    expect(wrapper.text()).toContain("共 1 条");
  });

  it("离开再回来：偏好驱动首拉请求（重挂即收窄，恢复的是裁决不是默认态）", async () => {
    seedMixedRows();
    const first = await mountView();
    await toggleHide(first);
    first.unmount();
    const second = await mountView();
    expect(lastListFilter().hide_investment_related).toBe(true);
    expect(second.text()).toContain("共 1 条");
  });
});

describe("偏好与筛选分家（ADR-0136 决策 1）：复位通道不触达偏好，偏好不入禁用态判定", () => {
  it("清除筛选不触达开关：过滤复位照常，开关仍开，后续请求仍携带隐藏参数", async () => {
    seedMixedRows();
    const wrapper = await mountView();
    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "acc-1");
    await flushPromises();
    await toggleHide(wrapper);
    expect(clearButton(wrapper).attributes("disabled")).toBeUndefined();
    await clearButton(wrapper).trigger("click");
    await flushPromises();
    const f = lastListFilter();
    expect(f).not.toHaveProperty("involving_account_id");
    expect(f.hide_investment_related).toBe(true);
    expect(hideSwitch(wrapper).attributes("aria-checked")).toBe("true");
  });

  it("开关状态不影响清除按钮禁用态：无过滤时开启开关，清除按钮仍禁用", async () => {
    seedMixedRows();
    const wrapper = await mountView();
    expect(clearButton(wrapper).attributes("disabled")).toBeDefined();
    await toggleHide(wrapper);
    expect(hideSwitch(wrapper).attributes("aria-checked")).toBe("true");
    expect(clearButton(wrapper).attributes("disabled")).toBeDefined();
  });

  it("ESC 复位不触达开关：过滤与分页复位照常，开关状态原样", async () => {
    // 窗口行为守卫宿主（App.vue 同构：守卫全局唯一，ESC 消费方，先例 session-retention）
    const GuardHost = defineComponent({
      setup() {
        useWindowGuard();
        return () => h("div");
      },
    });
    const guard = mount(GuardHost);
    seedMixedRows();
    const wrapper = await mountView();
    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "acc-1");
    await flushPromises();
    await toggleHide(wrapper);
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    await flushPromises();
    const f = lastListFilter();
    expect(f).toMatchObject({ page: 1 });
    expect(f).not.toHaveProperty("involving_account_id");
    expect(f.hide_investment_related).toBe(true);
    expect(hideSwitch(wrapper).attributes("aria-checked")).toBe("true");
    guard.unmount();
  });
});
