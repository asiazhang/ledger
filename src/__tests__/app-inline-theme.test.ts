import { describe, it, expect, afterEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createMemoryHistory, createRouter } from "vue-router";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { openMobileDrawer } from "@ledger/test-support/mobile-nav";
import App from "@/App.vue";
import { routes } from "@/router";
import { useAppStore } from "@/stores/app";
import { darkThemeClass, lightThemeClass } from "@ledger/theme/app-theme.css.ts";
import { accentColor, darkOverrides, lightOverrides } from "@ledger/theme/overrides";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { makeAccount, makeTransaction } from "./factories";
import type { Account, AccountBalance, Transaction } from "@ledger/types";

/**
 * 根配置非内联主题变量模式回归与等价性验收（issue #2186）：
 * - 断言对准渲染 DOM 节点是否脱离内联主题变量（style 不含 `--n-*`）并由共享
 *   样式类（`__<component>-...` + `<head>` 内对应 CSS 规则）承接；删掉 App.vue
 *   的 `inline-theme-disabled` 接线即红（ADR-0087 断言强度）。
 * - 覆盖桌面档侧栏布局、移动档导航抽屉、交易列表（表格/标签/分页/金额语义色/
 *   强调色链接）、账户列表（行菜单/弹窗表单）与设置页在暗/亮主题间的实时切换。
 */

const TEST_ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-1", name: "现金", type: "cash" }),
  makeAccount({ id: "acc-2", name: "工资卡", type: "bank" }),
];

const TEST_BALANCES: AccountBalance[] = [
  { account: TEST_ACCOUNTS[0], balance_cents: 120_000 },
  { account: TEST_ACCOUNTS[1], balance_cents: 500_000 },
];

const TEST_TXNS: Transaction[] = [
  makeTransaction({
    id: "txn-1",
    date: "2026-03-15",
    kind: "expense",
    amount_cents: 4500,
    amount_native_cents: 4500,
    account_id: "acc-1",
    note: "午餐",
  }),
  makeTransaction({
    id: "txn-2",
    date: "2026-03-14",
    kind: "income",
    amount_cents: 100_000,
    amount_native_cents: 100_000,
    account_id: "acc-2",
    note: "工资",
  }),
];

async function mountRealApp(initialPath = "/transactions") {
  const seam = wireInvokeSeam({
    defaults: {
      get_boot_status: { phase: "ready", error_code: null },
      list_policies: [],
      list_items: [],
      list_account_balances: TEST_BALANCES,
      savings_goal_progress: [],
      get_log_level: { level: "info" },
      list_backups: [],
      get_auto_backup_state: {
        enabled: true,
        last_backup_at: null,
        consecutive_failures: 0,
        failure_alerting: false,
      },
      app_build_info: { version: "0.11.2", platform: "linux", arch: "x86_64" },
      get_data_location_info: {
        active_dir: "/tmp/ledger",
        configured_dir: null,
        pending_restart: false,
        fallback_reason: null,
      },
      report_date_range: { min_date: "2026-01-01", max_date: "2026-03-31" },
      list_scheduled_transactions: [],
      list_budgets: [],
      list_books: { books: [], active_book_id: "default" },
      get_sync_status: { enabled: false },
      get_update_surface: "release-page",
      get_update_check_status: null,
      list_transactions: { items: TEST_TXNS, total: TEST_TXNS.length },
    },
    overrides: {
      list_accounts: () => TEST_ACCOUNTS,
    },
    refreshReferenceStores: true,
  });
  await seam.ready;

  const router = createRouter({ history: createMemoryHistory(), routes });
  await router.push(initialPath);
  await router.isReady();

  const wrapper = mount(App, {
    attachTo: document.body,
    global: { plugins: [router] },
  });
  await flushPromises();
  return { wrapper, router };
}

/** 从 <head> 内全部样式表文本中提取指定共享主题类的规则体。 */
function findMountedThemeRuleText(themeClass: string): string {
  const needle = `.${themeClass}`;
  for (const styleEl of document.head.querySelectorAll("style")) {
    const css = styleEl.textContent ?? "";
    if (css.includes(needle)) return css;
  }
  return "";
}

/** 断言组件节点脱离逐元素内联 `--n-*` 样式，并携带非空共享主题类与 `<head>` 挂载规则。 */
function expectSharedThemeClassWithoutInlineVars(el: Element, label: string): string {
  const inlineStyle = el.getAttribute("style") ?? "";
  expect(inlineStyle, `${label} 不应写入逐元素内联主题变量（style="${inlineStyle}"）`).not.toMatch(
    /--n-/,
  );

  const themeClass = [...el.classList].find((c) => /^__[a-z-]+/.test(c));
  expect(themeClass, `${label} 应挂载共享主题类（实际 class="${el.className}"）`).toBeTruthy();

  const cssText = findMountedThemeRuleText(themeClass!);
  expect(cssText, `<head> 应挂载共享主题类 .${themeClass} 的 CSS 变量规则`).toMatch(/--n-/);
  return themeClass!;
}

/** 断言选择器命中的全部组件节点均已脱离内联 `--n-*` 主题变量。 */
function expectAllDetached(root: ParentNode, selector: string): string[] {
  const els = [...root.querySelectorAll(selector)];
  expect(els.length, `未找到组件节点 ${selector}`).toBeGreaterThan(0);
  return els.map((el, idx) => expectSharedThemeClassWithoutInlineVars(el, `${selector}[${idx}]`));
}

/** 颜色规范化（jsdom 将十六进制颜色读回为 rgb(...)）。 */
function normalizeCssColor(color: string): string {
  const probe = document.createElement("span");
  probe.style.color = color;
  return probe.style.color;
}

/** 从 document.body 的下拉菜单浮层中按文案定位选项元素。 */
function findDropdownOption(label: string): HTMLElement | undefined {
  return [...document.body.querySelectorAll(".n-dropdown-option-body")].find((el) =>
    el.textContent?.includes(label),
  ) as HTMLElement | undefined;
}

describe("根配置非内联主题变量模式（issue #2186）", () => {
  afterEach(() => {
    document.body.classList.remove(darkThemeClass, lightThemeClass);
  });

  it("应用壳层与交易列表组件节点不再写入逐元素内联主题变量，改由共享样式类承接", async () => {
    const { wrapper } = await mountRealApp("/transactions");

    // 壳层布局 + 侧栏 + 菜单 + 按钮
    expectAllDetached(wrapper.element, ".n-layout");
    expectAllDetached(wrapper.element, ".n-layout-sider");
    expectAllDetached(wrapper.element, ".n-menu");
    expectAllDetached(wrapper.element, ".n-button");

    // 交易列表：表格 + 类型标签 + 分页条 + 下拉筛选（NSelect 主题类在 .n-base-selection）
    expectAllDetached(wrapper.element, ".n-data-table");
    expectAllDetached(wrapper.element, ".n-tag");
    expectAllDetached(wrapper.element, ".n-pagination");
    expectAllDetached(wrapper.element, ".n-base-selection");
  });

  it("暗色与亮色切换：共享主题类、根主题类、自定义覆写、强调色与语义色同步更新且零内联主题变量", async () => {
    const { wrapper } = await mountRealApp("/transactions");
    const appStore = useAppStore();

    // 暗色初态
    expect(appStore.theme).toBe("dark");
    expect(document.body.classList.contains(darkThemeClass)).toBe(true);
    expect(document.body.classList.contains(lightThemeClass)).toBe(false);

    const [darkMenuClass] = expectAllDetached(wrapper.element, ".n-menu");
    const [darkTableClass] = expectAllDetached(wrapper.element, ".n-data-table");
    expect(findMountedThemeRuleText(darkMenuClass)).toContain(darkOverrides.Menu.itemColorActive);

    const darkLink = wrapper.find(".account-link");
    expect(darkLink.exists()).toBe(true);
    expect((darkLink.element as HTMLElement).style.color).toBe(
      normalizeCssColor(accentColor("dark").base),
    );

    const darkExpenseAmount = wrapper.find(".amount-cell");
    expect(darkExpenseAmount.exists()).toBe(true);
    expect((darkExpenseAmount.element as HTMLElement).style.color).toBe(
      normalizeCssColor(kindSemanticColor("expense", "dark")),
    );

    // 切换亮色
    appStore.setTheme("light");
    await flushPromises();

    expect(document.body.classList.contains(lightThemeClass)).toBe(true);
    expect(document.body.classList.contains(darkThemeClass)).toBe(false);

    const [lightMenuClass] = expectAllDetached(wrapper.element, ".n-menu");
    const [lightTableClass] = expectAllDetached(wrapper.element, ".n-data-table");
    expect(lightMenuClass).not.toBe(darkMenuClass);
    expect(lightTableClass).not.toBe(darkTableClass);

    // 亮色强调色与主题覆盖通过共享样式类承接（如主按钮携带 lightOverrides.common.primaryColor）
    const primaryBtn = wrapper.find(".n-button--primary-type");
    expect(primaryBtn.exists()).toBe(true);
    const lightBtnClass = expectSharedThemeClassWithoutInlineVars(
      primaryBtn.element,
      ".n-button--primary-type",
    );
    expect(findMountedThemeRuleText(lightBtnClass).toUpperCase()).toContain(
      lightOverrides.common.primaryColor.toUpperCase(),
    );

    // 自定义链接强调色与交易语义色同步切换到亮色
    expect((wrapper.find(".account-link").element as HTMLElement).style.color).toBe(
      normalizeCssColor(accentColor("light").base),
    );
    expect((wrapper.find(".amount-cell").element as HTMLElement).style.color).toBe(
      normalizeCssColor(kindSemanticColor("expense", "light")),
    );

    // 切回暗色：共享类与覆写恢复
    appStore.setTheme("dark");
    await flushPromises();
    expect(document.body.classList.contains(darkThemeClass)).toBe(true);
    const [restoredMenuClass] = expectAllDetached(wrapper.element, ".n-menu");
    expect(restoredMenuClass).toBe(darkMenuClass);
  });

  it("抽查视图与交互弹层（账户页行菜单/弹窗表单、设置页外观开关、移动档导航抽屉）同样脱离内联主题变量", async () => {
    const { wrapper, router } = await mountRealApp("/accounts");

    // 账户页：表格、新建表单卡片（Card / FormItem / Input / Select）、行操作菜单
    expectAllDetached(wrapper.element, ".n-data-table");
    expectAllDetached(wrapper.element, ".n-card");
    expectAllDetached(wrapper.element, ".n-form-item");
    expectAllDetached(wrapper.element, ".n-input");
    expectAllDetached(wrapper.element, ".n-base-selection");

    const moreBtn = wrapper.find('button[aria-label="更多操作"]');
    expect(moreBtn.exists()).toBe(true);
    await moreBtn.trigger("click");
    await flushPromises();
    expectAllDetached(document.body, ".n-dropdown-menu");
    const deleteOption = findDropdownOption("删除");
    expect(deleteOption).toBeTruthy();
    expect(deleteOption!.getAttribute("style") ?? "").toContain("--n-prefix-color");

    // 点击「编辑」打开编辑账户弹窗（teleport 到 body 的 NModal + NCard + NFormItem + NInput）
    const editOption = findDropdownOption("编辑");
    expect(editOption).toBeTruthy();
    editOption!.click();
    await flushPromises();
    expectAllDetached(document.body, ".n-modal");
    expectAllDetached(document.body, ".n-card");
    expectAllDetached(document.body, ".n-form-item");
    expectAllDetached(document.body, ".n-input");

    // 导航到设置页：Tabs / Card / Switch / Select 均脱离内联变量，切外观开关即时切换主题
    await router.push("/settings");
    await flushPromises();
    expectAllDetached(wrapper.element, ".n-tabs");
    expectAllDetached(wrapper.element, ".n-card");
    expectAllDetached(wrapper.element, ".n-switch");

    const themeSwitch = wrapper.find(".n-switch");
    await themeSwitch.trigger("click");
    await flushPromises();
    expect(useAppStore().theme).toBe("light");
    expect(document.body.classList.contains(lightThemeClass)).toBe(true);
    expectAllDetached(wrapper.element, ".n-card");

    // 切到移动档：导航抽屉（AppDrawer 的 .n-drawer-container + NMenu + .group-more-link）脱离内联主题变量
    setFakeMedia({ width: 839 });
    await flushPromises();
    await openMobileDrawer(wrapper);
    expectAllDetached(document.body, ".n-drawer-container");
    expect(document.body.querySelector(".n-drawer")?.getAttribute("style") ?? "").not.toMatch(
      /--n-/,
    );
    expectAllDetached(document.body, ".n-menu");
    expect(document.body.querySelectorAll(".group-more-link").length).toBeGreaterThan(0);
  });
});
