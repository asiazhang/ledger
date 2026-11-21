import { describe, it, expect, vi, beforeEach } from "vitest";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink } from "./factories";
import { fireProp } from "@ledger/test-support/component-vm";
import { messageCalls } from "@ledger/test-support/message-mock";
import { mount, flushPromises } from "@vue/test-utils";
import {
  NCard,
  NDataTable,
  NDialogProvider,
  NDropdown,
  NForm,
  NInput,
  NInputNumber,
  NModal,
  NSelect,
} from "naive-ui";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { h, nextTick } from "vue";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import AccountsView from "@/views/AccountsView.vue";
import AccountLink from "@/accounts/AccountLink.vue";
import {
  makeGoalPair,
  makeSavingsGoal,
  makeSavingsGoalProgress,
  makeSubscriptionPlan,
  makeTransferPlan,
} from "./factories";
import { amountPrivacyEnabled, formatAmount } from "@ledger/money";
import type {
  Account,
  AccountBalance,
  SavingsGoalProgress,
  ScheduledTransactionWithExt,
} from "@ledger/types";

const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRouter: () => ({ push: pushMock }),
}));

function makeAccount(id: string, name: string): Account {
  return {
    id,
    name,
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: false,
  };
}

const mockBalances: AccountBalance[] = [
  { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
  { account: makeAccount("acc-2", "银行"), balance_cents: -500 },
];

let goalProgress: SavingsGoalProgress[] = [];

beforeEach(async () => {
  pushMock.mockReset();
  goalProgress = [];
  await wireInvokeSeam({
    defaults: { list_account_balances: mockBalances },
    overrides: {
      list_accounts: mockBalances.map((b) => b.account),
      savings_goal_progress: () => Promise.resolve(goalProgress),
    },
    refreshReferenceStores: true,
  }).ready;
  await useSavingsGoalsStore()
    .refresh()
    .catch(() => {});
});

describe("AccountsView 账户名下钻（issue #97）", () => {
  it("账户名称渲染为可点击组件（标题提示查看该账户的交易）", async () => {
    const wrapper = mountView();
    await flushPromises();
    const links = wrapper.findAllComponents(AccountLink);
    expect(links.length).toBe(2);
    expect(links[0].text()).toBe("现金");
    expect(links[0].attributes("title")).toBe("查看该账户的交易");
  });

  it("点击账户名称跳转交易页并携带该账户过滤参数", async () => {
    const wrapper = mountView();
    await flushPromises();
    const links = wrapper.findAllComponents(AccountLink);
    await links[1].find("button").trigger("click");
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: { account: "acc-2" },
    });
  });

  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }
});

describe("AccountsView 行菜单冒烟（issue #551：右键 + 「⋯」双入口）", () => {
  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
  }

  async function openMenuOnRow(wrapper: ReturnType<typeof mount>, index = 0) {
    await bodyRows(wrapper)[index].trigger("contextmenu");
    await flushPromises();
  }

  async function openMenuOnMoreButton(wrapper: ReturnType<typeof mount>, index = 0) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger("click");
    await flushPromises();
  }

  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  it("行右键弹出行菜单：编辑 / 调整余额 / 删除", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(false);
    await openMenuOnRow(wrapper, 0);
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual([
      "edit",
      "adjust-balance",
      "archive",
      "menu-divider",
      "delete",
    ]);
  });

  it("操作列「⋯」按钮弹出行菜单（第二入口，与右键共用同一菜单）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnMoreButton(wrapper, 1);
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual([
      "edit",
      "adjust-balance",
      "archive",
      "menu-divider",
      "delete",
    ]);
  });

  it("菜单选中「编辑」分派到编辑弹窗并回填目标行（第二行「银行」）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnRow(wrapper, 1);
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
    const editModal = wrapper
      .findAllComponents(NModal)
      .find((m) => m.props("title") === "编辑账户")!;
    expect(editModal.props("show")).toBe(true);
    const editForm = wrapper.findAllComponents(NForm)[1];
    expect(editForm.findComponent(NInput).props("value")).toBe("银行");
  });
});

describe("AccountsView 移动档（issue #847 / ADR-0088 决策 11 票⑦，词汇表「窗口分级」）", () => {
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
  }

  function moreButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button[aria-label="更多操作"]');
  }

  const mobileBalances: AccountBalance[] = [
    { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
    { account: { ...makeAccount("acc-2", "银行"), type: "bank" }, balance_cents: -500 },
  ];

  async function wireMobileBalances() {
    await wireInvokeSeam({
      defaults: { list_account_balances: mobileBalances },
      overrides: { list_accounts: mobileBalances.map((b) => b.account) },
    }).ready;
  }

  it("桌面档零变化：五列全列、行内新增表单、「⋯」无 48px 扩径", async () => {
    const wrapper = mountView();
    await flushPromises();
    const table = wrapper.findComponent(NDataTable);
    expect((table.props("columns") as unknown[]).length).toBe(5);
    const createForm = wrapper.findAllComponents(NForm)[0];
    expect(createForm.props("inline")).toBe(true);
    const btn = moreButtons(wrapper)[0].element as HTMLElement;
    expect(btn.style.width).toBe("");
  });

  it("移动档列结构三分：名称（类型/币种并入副行）、余额、操作——无横向滚动前提", async () => {
    setFakeMedia({ width: 600 });
    await wireMobileBalances();
    const wrapper = mountView();
    await flushPromises();
    const table = wrapper.findComponent(NDataTable);
    const columns = table.props("columns") as Array<{ key?: string }>;
    expect(columns.map((c) => c.key)).toEqual(["account.name", "balance_cents", "actions"]);
    expect(bodyRows(wrapper)[1].text()).toContain("银行卡 · CNY");
  });

  it("移动档：新增账户表单纵向堆叠（标签上置 + 控件满宽）", async () => {
    setFakeMedia({ width: 600 });
    const wrapper = mountView();
    await flushPromises();
    const createForm = wrapper.findAllComponents(NForm)[0];
    expect(createForm.props("inline")).toBe(false);
    expect(createForm.props("labelPlacement")).toBe("top");
  });

  it("移动档：「⋯」按钮 48px 触控目标（ADR-0088 全局验收基线，桌面不挂）", async () => {
    setFakeMedia({ width: 600 });
    const wrapper = mountView();
    await flushPromises();
    for (const btn of moreButtons(wrapper)) {
      const el = btn.element as HTMLElement;
      expect(el.style.width).toBe("48px");
      expect(el.style.height).toBe("48px");
    }
  });

  it("移动档：账户名换行不截断（触屏无悬停全文，悬停替代「空间够则常驻」）；桌面保持 nowrap", async () => {
    setFakeMedia({ width: 600 });
    const mobile = mountView();
    await flushPromises();
    const mobileLink = mobile.findAllComponents(AccountLink)[0].element as HTMLElement;
    expect(mobileLink.style.whiteSpace).toBe("normal");
    mobile.unmount();

    setFakeMedia({ width: 1280 });
    const desktop = mountView();
    await flushPromises();
    const desktopLink = desktop.findAllComponents(AccountLink)[0].element as HTMLElement;
    expect(desktopLink.style.whiteSpace).toBe("");
  });

  it("跨断点缩窗实时换列（五列 ⇄ 三列）", async () => {
    const wrapper = mountView();
    await flushPromises();
    const table = wrapper.findComponent(NDataTable);
    expect((table.props("columns") as unknown[]).length).toBe(5);
    setFakeMedia({ width: 600 });
    await flushPromises();
    expect((table.props("columns") as unknown[]).length).toBe(3);
    setFakeMedia({ width: 1280 });
    await flushPromises();
    expect((table.props("columns") as unknown[]).length).toBe(5);
  });

  it("触控轴：「⋯」与右键两轴一致——同一菜单、同一选项闭集", async () => {
    setFakeMedia({ width: 600, hover: "none", pointer: "coarse" });
    const wrapper = mountView();
    await flushPromises();
    const expected = ["edit", "adjust-balance", "archive", "menu-divider", "delete"];

    await bodyRows(wrapper)[0].trigger("contextmenu");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual(expected);
    fireProp(rowMenu(wrapper), "onClickoutside");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(false);
    await moreButtons(wrapper)[1].trigger("click");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual(expected);
  });

  it("移动档：金额隐私模式生效（余额掩码，名称/类型/币种副行不掩）", async () => {
    setFakeMedia({ width: 600 });
    await wireMobileBalances();
    const wrapper = mountView();
    await flushPromises();
    const visible = formatAmount(1000, {
      code: "CNY",
      name: "人民币",
      symbol: "¥",
      decimal_places: 2,
    });
    expect(wrapper.text()).toContain(visible);
    amountPrivacyEnabled.value = true;
    await nextTick();
    expect(wrapper.text()).toContain("••••");
    expect(wrapper.text()).not.toContain(visible);
    expect(bodyRows(wrapper)[1].text()).toContain("银行卡 · CNY");
    amountPrivacyEnabled.value = false;
    await nextTick();
    expect(wrapper.text()).toContain(visible);
  });

  const CNY = { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 };

  function dialogContent(): HTMLElement {
    return [...document.body.querySelectorAll<HTMLElement>(".n-dialog__content")].at(-1)!;
  }

  function dialogButton(text: string): HTMLElement | undefined {
    const buttons = [...document.body.querySelectorAll<HTMLElement>(".n-dialog__action button")];
    return buttons.filter((b) => b.textContent?.trim() === text).at(-1);
  }

  it("移动档：归档确认框可读可用——余额句在正文容器内、无固定宽度（不横向溢出）", async () => {
    setFakeMedia({ width: 600 });
    const wrapper = mountView();
    await flushPromises();
    await moreButtons(wrapper)[0].trigger("click");
    await flushPromises();
    fireProp(rowMenu(wrapper), "onSelect", "archive");
    await flushPromises();

    const content = dialogContent();
    expect(content.textContent).toContain("现金");
    expect(content.textContent).toContain(formatAmount(1000, CNY));
    expect(content.textContent).toContain("仍计入净资产与余额合计");

    const lines = content.firstElementChild as HTMLElement;
    expect(lines.style.display).toBe("flex");
    expect(lines.style.flexDirection).toBe("column");
    expect(lines.style.width).toBe("");
    expect(lines.style.minWidth).toBe("");
    expect(content.children).toHaveLength(1);
    const lineTexts = [...lines.children].map((el) => el.textContent ?? "");
    expect(lineTexts.some((t) => t.includes("仍计入净资产与余额合计"))).toBe(true);

    expect(dialogButton("归档")).toBeTruthy();
  });

  it("移动档：已归档分区展开后行可读（名称链接 + 余额），该行菜单只有「恢复」", async () => {
    setFakeMedia({ width: 600 });
    const archivedRows: AccountBalance[] = [
      { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
      {
        account: { ...makeAccount("acc-2", "银行"), type: "bank", is_archived: true },
        balance_cents: -500,
      },
    ];
    await wireInvokeSeam({
      defaults: { list_account_balances: archivedRows },
      overrides: { list_accounts: archivedRows.map((b) => b.account) },
    }).ready;
    const wrapper = mountView();
    await flushPromises();

    await wrapper.find('[data-testid="accounts-archived-toggle"]').trigger("click");
    await flushPromises();
    const archivedCard = wrapper.find('[data-testid="accounts-archived-card"]');
    const archivedTableRows = archivedCard.findAll("tbody tr");
    expect(archivedTableRows).toHaveLength(1);
    expect(archivedTableRows[0].text()).toContain("银行卡 · CNY");
    expect(archivedTableRows[0].text()).toContain(formatAmount(-500, CNY));

    const archivedLink = archivedCard.findAllComponents(AccountLink)[0];
    expect(archivedLink.text()).toBe("银行");
    expect(archivedLink.attributes("title")).toBe("查看该账户的交易");

    await moreButtons(wrapper)[1].trigger("click");
    await flushPromises();
    const menu = wrapper
      .findAllComponents(NDropdown)
      .find((d) =>
        (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "restore"),
      )!;
    expect((menu.props("options") as Array<{ key: string }>).map((o) => o.key)).toEqual([
      "restore",
    ]);
  });
});

describe("AccountsView 信用卡档案（spec #1327 / ADR-0119）", () => {
  const cardBalances: AccountBalance[] = [
    {
      account: {
        ...makeAccount("acc-credit", "招行信用卡"),
        type: "credit",
        credit_limit_cents: 5_000_000,
        statement_day: 5,
        due_day: 25,
      },
      balance_cents: -320_000,
    },
    {
      account: {
        ...makeAccount("acc-hot", "中信信用卡"),
        type: "credit",
        credit_limit_cents: 1_000_000,
        statement_day: 20,
        due_day: 8,
      },
      balance_cents: -950_000,
    },
    { account: makeAccount("acc-cash", "现金"), balance_cents: 1000 },
    {
      account: { ...makeAccount("acc-nolimit", "无额度卡"), type: "credit" },
      balance_cents: -500,
    },
    {
      account: {
        ...makeAccount("acc-clear", "已还清卡"),
        type: "credit",
        credit_limit_cents: 1_000_000,
      },
      balance_cents: 0,
    },
  ];

  const CNY = { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 };

  async function wireCards() {
    await wireInvokeSeam({
      defaults: { list_account_balances: cardBalances, update_account: null },
      overrides: { list_accounts: cardBalances.map((b) => b.account) },
      refreshReferenceStores: true,
    }).ready;
  }

  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  async function openEditOnRow(wrapper: ReturnType<typeof mount>, index: number) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger("click");
    await flushPromises();
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
  }

  function editForm(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NForm)[1];
  }

  function editModal(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NModal).find((m) => m.props("title") === "编辑账户")!;
  }

  it("列表：信用卡行在既有余额单元格内多一行使用率；无欠款/未设额度/非信用卡行都不显示", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    const rows = bodyRows(wrapper);
    expect(rows[0].text()).toContain("已用 6%");
    expect(rows[1].text()).toContain("已用 95%");
    expect(rows[2].text(), "现金账户不是信用卡").not.toContain("已用");
    expect(rows[3].text(), "未设额度算不出使用率").not.toContain("已用");
    expect(rows[4].text(), "已还清（0%）不显示").not.toContain("已用");
  });

  it("列表：使用率 ≥90% 才用警示色（其余用弱化小字，两者不同）", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    const line = (wrapper: ReturnType<typeof mount>, index: number, text: string) =>
      bodyRows(wrapper)
        [index].findAll("div")
        .find((d) => d.text() === text)!;
    expect(line(wrapper, 1, "已用 95%").attributes("style")).toContain("color");
    expect(line(wrapper, 0, "已用 6%").attributes("style")).not.toContain("color");
  });

  it("移动档：信用卡小字仍在余额单元格内，列结构不变（三列）", async () => {
    setFakeMedia({ width: 600 });
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    const table = wrapper.findComponent(NDataTable);
    expect((table.props("columns") as unknown[]).length).toBe(3);
    expect(bodyRows(wrapper)[0].text()).toContain("已用 6%");
  });

  it("新增表单：选「信用卡」才出现额度/账单日/还款日三个输入", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    const createForm = () => wrapper.findAllComponents(NForm)[0];
    expect(createForm().text()).not.toContain("信用额度");

    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "credit");
    await flushPromises();
    expect(createForm().text()).toContain("信用额度");
    expect(createForm().text()).toContain("账单日");
    expect(createForm().text()).toContain("还款日");

    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "bank");
    await flushPromises();
    expect(createForm().text()).not.toContain("信用额度");
  });

  it("编辑弹窗：信用卡账户回填档案输入并显示只读额度用量与下次账单节点（ISO 具体日期）", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    await openEditOnRow(wrapper, 0);
    const form = editForm(wrapper);
    expect(form.text()).toContain("信用额度");
    expect(form.findAllComponents(NInputNumber)[0].props("value")).toBe(50_000);
    expect(form.findAllComponents(NInputNumber)[1].props("value")).toBe(5);
    expect(form.findAllComponents(NInputNumber)[2].props("value")).toBe(25);
    expect(form.text()).toContain("额度使用");
    expect(form.text(), "已用额度按账户币种格式化").toContain(formatAmount(320_000, CNY));
    expect(form.text(), "可用额度 = 额度 + 余额").toContain(formatAmount(4_680_000, CNY));
    expect(form.text()).toContain("使用率 6%");
    expect(form.text()).toContain("下次账单节点");
    const dates = form.text().match(/\d{4}-\d{2}-\d{2}/g) ?? [];
    expect(dates.length, "账单日与还款日各给一个具体日期").toBeGreaterThanOrEqual(2);
  });

  it("编辑弹窗：非信用卡账户不出现档案输入与摘要", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    await openEditOnRow(wrapper, 2);
    const form = editForm(wrapper);
    expect(form.text()).not.toContain("信用额度");
    expect(form.text()).not.toContain("额度使用");
  });

  it("提交编辑：信用卡账户的档案三字段随 PUT 落定（调用事实 + 弹窗关闭效果）", async () => {
    await wireCards();
    const wrapper = mountView();
    await flushPromises();
    await openEditOnRow(wrapper, 0);
    const inputs = editForm(wrapper).findAllComponents(NInputNumber);
    inputs[0].vm.$emit("update:value", 60_000);
    inputs[2].vm.$emit("update:value", null);
    await flushPromises();
    const save = editForm(wrapper)
      .findAll("button")
      .find((b) => b.text() === "保存")!;
    await save.trigger("click");
    await flushPromises();

    const args = lastInvokeArgs("update_account").input as Record<string, unknown>;
    expect(args.credit_limit_cents).toBe(6_000_000);
    expect(args.statement_day).toBe(5);
    expect(args.due_day, "清空的还款日以 null 落定").toBeNull();
    expect(editModal(wrapper).props("show"), "保存成功后关闭弹窗").toBe(false);
  });
});

describe("AccountsView 目标账户名随动只读（issue #1752 / ADR-0133 决策 2）", () => {
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  async function openEditOnRow(wrapper: ReturnType<typeof mount>, index: number) {
    const row = wrapper.findAll(".n-data-table-tbody .n-data-table-tr")[index];
    await row.trigger("contextmenu");
    await flushPromises();
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
  }

  function editNameInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NForm)[1].findComponent(NInput);
  }

  function editSaveButton(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NForm)[1]
      .findAll("button")
      .find((b) => b.text() === "保存")!;
  }

  it("目标绑定账户：名称输入禁用（随动只读）且提交不带 name 键", async () => {
    goalProgress = [
      makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1", account_id: "acc-1" }) }),
    ];
    await wireInvokeSeam({
      defaults: { list_account_balances: mockBalances, update_account: null },
      overrides: {
        list_accounts: mockBalances.map((b) => b.account),
        savings_goal_progress: () => Promise.resolve(goalProgress),
      },
    }).ready;
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();

    await openEditOnRow(wrapper, 0);
    expect(editNameInput(wrapper).props("disabled")).toBe(true);

    await editSaveButton(wrapper).trigger("click");
    await flushPromises();
    const args = lastInvokeArgs("update_account") as {
      id: string;
      input: Record<string, unknown>;
    };
    expect(args.id).toBe("acc-1");
    expect(args.input).not.toHaveProperty("name");
    expect(args.input).toHaveProperty("currency_code", "CNY");
  });

  it("普通账户编辑不受影响：名称输入可用（不误伤）", async () => {
    goalProgress = [
      makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1", account_id: "acc-1" }) }),
    ];
    const wrapper = mountView();
    await flushPromises();

    await openEditOnRow(wrapper, 1);
    expect(editNameInput(wrapper).props("disabled")).toBe(false);
  });
});

describe("AccountsView 攒钱分组与类型标签（issue #1755 / ADR-0133 决策 2）", () => {
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  async function openEditOnRow(wrapper: ReturnType<typeof mount>, index: number) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger("click");
    await flushPromises();
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
  }

  const CNY = { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 };

  it("在用目标账户归入「攒钱」分组：类型标签覆写、余额原值展示（读数不受分组影响）", async () => {
    goalProgress = [makeGoalPair({ id: "acc-1" }).progress];
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();
    const cards = wrapper.findAllComponents(NCard);
    const goalCard = cards.find((c) => c.props("title") === "攒钱");
    expect(goalCard, "分组卡按标题可定位（删分组拆分即找不到）").toBeDefined();
    expect(goalCard!.findAll("tbody tr")).toHaveLength(1);
    const listCard = cards.find((c) => c.props("title") === "账户列表");
    expect(listCard!.findAll("tbody tr"), "普通卡只余未绑定行").toHaveLength(1);
    expect(listCard!.text()).toContain("银行");
    const rows = bodyRows(wrapper);
    expect(rows).toHaveLength(2);
    expect(rows[0].text()).toContain("现金");
    expect(rows[0].text()).toContain("攒钱");
    expect(rows[0].text()).not.toContain("其他");
    expect(rows[0].text()).toContain(formatAmount(1000, CNY));
    expect(rows[1].text()).toContain("银行");
    expect(rows[1].text()).toContain("银行");
  });

  it("无绑定不误伤：普通 other 账户仍在原列表原位；目标组空集不渲染空卡", async () => {
    const balances: AccountBalance[] = [
      ...mockBalances,
      { account: { ...makeAccount("acc-3", "备用现金"), type: "other" }, balance_cents: 0 },
    ];
    await wireInvokeSeam({
      defaults: { list_account_balances: balances },
      overrides: {
        list_accounts: balances.map((b) => b.account),
        savings_goal_progress: () => Promise.resolve(goalProgress),
      },
      refreshReferenceStores: true,
    }).ready;
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.text()).not.toContain("攒钱");
    const rows = bodyRows(wrapper);
    expect(rows).toHaveLength(3);
    expect(rows[2].text()).toContain("备用现金");
    expect(rows[2].text()).toContain("其他");
  });

  it("编辑弹窗：目标账户的只读类型显示「攒钱」", async () => {
    goalProgress = [makeGoalPair({ id: "acc-1" }).progress];
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();
    await openEditOnRow(wrapper, 0);
    expect(wrapper.findAllComponents(NForm)[1].findAllComponents(NInput)[1].props("value")).toBe(
      "攒钱",
    );
  });
});

describe("AccountsView 账户归档（spec #1952 / ADR-0142）", () => {
  const CNY = { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 };
  const USD = { code: "USD", name: "美元", symbol: "$", decimal_places: 2 };

  let rows: AccountBalance[];

  let plans: ScheduledTransactionWithExt[];

  function seedRows(): AccountBalance[] {
    return [
      { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
      {
        account: { ...makeAccount("acc-2", "银行"), is_archived: true },
        balance_cents: -500,
      },
      {
        account: { ...makeAccount("acc-3", "旧卡"), is_archived: true },
        balance_cents: 2500,
      },
    ];
  }

  async function wireArchiveSeam(archiveReject?: unknown) {
    await wireInvokeSeam({
      defaults: { list_accounts: rows.map((row) => row.account) },
      overrides: {
        list_account_balances: () => rows,
        savings_goal_progress: () => Promise.resolve(goalProgress),
        list_scheduled_transactions: () => plans,
        list_currencies: [CNY, USD],
        archive_account: (args?: Record<string, unknown>) => {
          if (archiveReject) return Promise.reject(archiveReject);
          const { id } = args as { id: string };
          rows = rows.map((row) =>
            row.account.id === id
              ? { ...row, account: { ...row.account, is_archived: true } }
              : row,
          );
          return Promise.resolve();
        },
        unarchive_account: (args?: Record<string, unknown>) => {
          const { id } = args as { id: string };
          rows = rows.map((row) =>
            row.account.id === id
              ? { ...row, account: { ...row.account, is_archived: false } }
              : row,
          );
          return Promise.resolve();
        },
      },
      refreshReferenceStores: true,
    }).ready;
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
  }

  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  function listCard(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NCard).find((c) => c.props("title") === "账户列表");
  }
  function archivedCard(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('[data-testid="accounts-archived-card"]');
  }
  function archivedSummary(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('[data-testid="accounts-archived-summary"]').text();
  }

  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) =>
        (d.props("options") as Array<{ key?: string }>).some(
          (o) => o.key === "edit" || o.key === "restore",
        ),
      )!;
  }
  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
  }
  async function openMenuOnRowButton(wrapper: ReturnType<typeof mount>, index: number) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger("click");
    await flushPromises();
  }
  async function expandArchived(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="accounts-archived-toggle"]').trigger("click");
    await flushPromises();
  }
  function allRowTexts(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr").map((tr) => tr.text());
  }

  function dialogText(): string {
    return [...document.body.querySelectorAll(".n-dialog__content")].at(-1)?.textContent ?? "";
  }

  async function openArchiveDialog(wrapper: ReturnType<typeof mount>, index = 0) {
    await openMenuOnRowButton(wrapper, index);
    fireProp(rowMenu(wrapper), "onSelect", "archive");
    await flushPromises();
  }

  function dialogButton(text: string) {
    const buttons = [...document.body.querySelectorAll(".n-dialog__action button")];
    return buttons.filter((b) => b.textContent?.trim() === text).at(-1) as
      | HTMLButtonElement
      | undefined;
  }

  beforeEach(async () => {
    rows = seedRows();
    plans = [];
    await wireArchiveSeam();
  });

  it("已归档行移入页底「已归档」分区：默认折叠、标题显示数量与余额合计，主表只余在用账户", async () => {
    const wrapper = mountView();
    await flushPromises();

    const list = listCard(wrapper)!;
    expect(list.findAll("tbody tr")).toHaveLength(1);
    expect(list.text()).toContain("现金");
    expect(list.text()).not.toContain("银行");

    expect(archivedCard(wrapper).exists()).toBe(true);
    expect(archivedSummary(wrapper)).toContain("2 个账户");
    expect(archivedSummary(wrapper)).toContain(formatAmount(2000, CNY));
    expect(archivedCard(wrapper).findAll("tbody tr")).toHaveLength(0);
    expect(
      wrapper.find('[data-testid="accounts-archived-toggle"]').attributes("aria-expanded"),
    ).toBe("false");
  });

  it("合计按币种各算一份：单币种读法与既有逐字相同，混币种绝不跨币种相加", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(archivedSummary(wrapper)).toBe(`2 个账户 · 合计 ${formatAmount(2000, CNY)}`);
    wrapper.unmount();

    rows = [
      { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
      {
        account: { ...makeAccount("acc-2", "银行"), is_archived: true },
        balance_cents: 2500,
      },
      {
        account: { ...makeAccount("acc-3", "美元账户"), currency_code: "USD", is_archived: true },
        balance_cents: 5600,
      },
      {
        account: { ...makeAccount("acc-4", "美元旧卡"), currency_code: "USD", is_archived: true },
        balance_cents: 100,
      },
    ];
    await wireArchiveSeam();
    const mixed = mountView();
    await flushPromises();
    const summary = archivedSummary(mixed);
    expect(summary).toContain(`1 个账户 · 合计 ${formatAmount(2500, CNY)}`);
    expect(summary).toContain(`2 个账户 · 合计 ${formatAmount(5700, USD)}`);
    expect(summary).not.toContain(formatAmount(8200, CNY));
    expect(summary).toContain("；");
    await mixed.find('[data-testid="accounts-archived-toggle"]').trigger("click");
    await flushPromises();
    expect(mixed.find('[data-testid="accounts-archived-card"]').text()).toContain(
      formatAmount(2500, CNY),
    );
  });

  it("展开已归档分区：归档行出现且余额按快照原值渲染，合计不变（读数不受收纳影响）", async () => {
    const wrapper = mountView();
    await flushPromises();
    const summaryBefore = archivedSummary(wrapper);

    await expandArchived(wrapper);

    const rowsInArchive = archivedCard(wrapper).findAll("tbody tr");
    expect(rowsInArchive).toHaveLength(2);
    expect(rowsInArchive[0].text()).toContain("银行");
    expect(rowsInArchive[0].text()).toContain(formatAmount(-500, CNY));
    expect(rowsInArchive[1].text()).toContain("旧卡");
    expect(rowsInArchive[1].text()).toContain(formatAmount(2500, CNY));
    expect(archivedSummary(wrapper)).toBe(summaryBefore);
    expect(
      wrapper.find('[data-testid="accounts-archived-toggle"]').attributes("aria-expanded"),
    ).toBe("true");
  });

  it("行菜单：未归档行含「归档」，已归档行只有「恢复」（编辑 / 调整余额 / 删除都要先恢复）", async () => {
    const wrapper = mountView();
    await flushPromises();

    await openMenuOnRowButton(wrapper, 0);
    expect(menuKeys(wrapper)).toEqual([
      "edit",
      "adjust-balance",
      "archive",
      "menu-divider",
      "delete",
    ]);

    await expandArchived(wrapper);
    await openMenuOnRowButton(wrapper, 1);
    expect(menuKeys(wrapper)).toEqual(["restore"]);
    expect((rowMenu(wrapper).props("options") as Array<{ label: string }>)[0].label).toBe("恢复");
  });

  it("点击「归档」→ 二次确认 → 调用 archive_account，行移入已归档分区", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnRowButton(wrapper, 0);
    fireProp(rowMenu(wrapper), "onSelect", "archive");
    await flushPromises();
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "archive_account")).toBe(false);

    dialogButton("归档")!.click();
    await flushPromises();

    expect(lastInvokeArgs("archive_account")).toMatchObject({ id: "acc-1" });
    expect(listCard(wrapper)!.text()).not.toContain("现金");
    expect(archivedSummary(wrapper)).toContain("3 个账户");
  });

  it("点击「恢复」→ 调用 unarchive_account，行回到主表并恢复既有行为", async () => {
    const wrapper = mountView();
    await flushPromises();
    await expandArchived(wrapper);
    await openMenuOnRowButton(wrapper, 1);
    fireProp(rowMenu(wrapper), "onSelect", "restore");
    await flushPromises();

    expect(lastInvokeArgs("unarchive_account")).toMatchObject({ id: "acc-2" });
    expect(listCard(wrapper)!.text()).toContain("银行");
    expect(archivedSummary(wrapper)).toContain("1 个账户");
    await openMenuOnRowButton(wrapper, 1);
    expect(menuKeys(wrapper)).toEqual([
      "edit",
      "adjust-balance",
      "archive",
      "menu-divider",
      "delete",
    ]);
  });

  it("归档被拒（绑定储蓄目标的账户）：码化错误文案在场，账户仍在主表", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    rows = seedRows();
    await wireArchiveSeam({
      kind: "Invalid",
      code: "account.goal-bound-cannot-archive",
      message: "该账户是攒钱目标专属账户，请归档对应的攒钱目标",
      params: [],
    });
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnRowButton(wrapper, 0);
    fireProp(rowMenu(wrapper), "onSelect", "archive");
    await flushPromises();
    dialogButton("归档")!.click();
    await flushPromises();

    expect(sink.error.mock.calls.some((c) => String(c[0]).includes("请归档对应的攒钱目标"))).toBe(
      true,
    );
    expect(messageCalls().some((m) => m.method === "success")).toBe(false);
    expect(listCard(wrapper)!.text()).toContain("现金");
  });

  it("余额非零：确认框显示当前余额并声明仍计入净资产与余额合计（金额经 formatAmount）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openArchiveDialog(wrapper, 0);

    const text = dialogText();
    expect(text).toContain("现金");
    expect(text).toContain(formatAmount(1000, CNY));
    expect(text).toContain("仍计入净资产与余额合计");
  });

  it("余额为零：确认框不追加余额声明（不制造噪音）", async () => {
    rows = [{ account: makeAccount("acc-zero", "结清借贷"), balance_cents: 0 }];
    await wireArchiveSeam();
    const wrapper = mountView();
    await flushPromises();
    await openArchiveDialog(wrapper, 0);

    const text = dialogText();
    expect(text).toContain("结清借贷");
    expect(text).not.toContain("仍计入净资产与余额合计");
    expect(text).not.toContain(formatAmount(0, CNY));
  });

  it("金额隐私模式：确认框余额同样被掩码（收纳不泄露数字）", async () => {
    const wrapper = mountView();
    await flushPromises();
    const plain = formatAmount(1000, CNY);
    amountPrivacyEnabled.value = true;
    await nextTick();
    await openArchiveDialog(wrapper, 0);

    const text = dialogText();
    expect(text).toContain("••••");
    expect(text).not.toContain(plain);
    amountPrivacyEnabled.value = false;
  });

  it("被未结束计划引用：确认框提示计划数量，确认后归档成功且计划未被停用", async () => {
    plans = [
      makeSubscriptionPlan({ id: "plan-sub", account_id: "acc-1" }),
      makeTransferPlan({ id: "plan-in", account_id: "acc-9" }, "acc-1"),
      makeSubscriptionPlan({ id: "plan-paused", account_id: "acc-1", status: "paused" }),
      makeSubscriptionPlan({ id: "plan-cancelled", account_id: "acc-1", status: "cancelled" }),
      makeSubscriptionPlan({ id: "plan-completed", account_id: "acc-1", status: "completed" }),
      makeSubscriptionPlan({ id: "plan-other", account_id: "acc-9" }),
      makeSubscriptionPlan({ id: "plan-deleted", account_id: "acc-1", is_deleted: true }),
    ];
    const statusesBefore = plans.map((p) => p.core.status);
    const wrapper = mountView();
    await flushPromises();
    await openArchiveDialog(wrapper, 0);

    const text = dialogText();
    expect(text).toContain("3 个未结束的定时计划");
    expect(text).toContain("归档不会停用计划");

    dialogButton("归档")!.click();
    await flushPromises();
    expect(lastInvokeArgs("archive_account")).toMatchObject({ id: "acc-1" });
    expect(listCard(wrapper)!.text()).not.toContain("现金");
    expect(archivedSummary(wrapper)).toContain("3 个账户");

    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === "update_scheduled_transaction_status"),
    ).toBe(false);
    expect(plans.map((p) => p.core.status)).toEqual(statusesBefore);
  });

  it("归档前后余额与合计读数完全一致：行集只重排、金额一字不动", async () => {
    const wrapper = mountView();
    await flushPromises();
    await expandArchived(wrapper);
    const rowsBefore = allRowTexts(wrapper);
    const summaryBefore = archivedSummary(wrapper);
    expect(summaryBefore).toContain("2 个账户");
    expect(summaryBefore).toContain(formatAmount(2000, CNY));

    await openArchiveDialog(wrapper, 0);
    dialogButton("归档")!.click();
    await flushPromises();

    expect(allRowTexts(wrapper)).toEqual(rowsBefore);
    expect(archivedSummary(wrapper)).toContain("3 个账户");
    expect(archivedSummary(wrapper)).toContain(formatAmount(3000, CNY));
    expect(archivedCard(wrapper).text()).toContain(formatAmount(1000, CNY));
  });
});
