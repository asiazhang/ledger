import { describe, it, expect, vi, beforeEach } from "vitest";
import { lastInvokeArgs, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { fireProp } from "@ledger/test-support/component-vm";
import { mount, flushPromises } from "@vue/test-utils";
import {
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
import { makeGoalPair, makeSavingsGoal, makeSavingsGoalProgress } from "./factories";
import { amountPrivacyEnabled, formatAmount } from "@ledger/money";
import type { Account, AccountBalance, SavingsGoalProgress } from "@ledger/types";

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
  };
}

const mockBalances: AccountBalance[] = [
  { account: makeAccount("acc-1", "现金"), balance_cents: 1000 },
  { account: makeAccount("acc-2", "银行"), balance_cents: -500 },
];

/** 目标绑定派生夹具（issue #1752）：储蓄目标 store 快照——账户是否目标专属
 *  由 account_id 集合派生（ADR-0133 决策 2），每测复位。 */
let goalProgress: SavingsGoalProgress[] = [];

beforeEach(async () => {
  pushMock.mockReset();
  goalProgress = [];
  // list_accounts 参考命令本场景需自定义值（acc-2「银行」，overrides 优先于参考兜底）；
  // 参考 store 预载走接缝 opt-in 参数。
  await wireInvokeSeam({
    defaults: { list_account_balances: mockBalances },
    overrides: {
      list_accounts: mockBalances.map((b) => b.account),
      savings_goal_progress: () => Promise.resolve(goalProgress),
    },
    refreshReferenceStores: true,
  }).ready;
  // 账户页分组消费绑定集（issue #1755）：goals store self-init 快照与 goalProgress
  // 变量同源——每测复位后显式 refresh 落位，绑定派生（分组 / 名随动只读 / 类型标签
  // 覆写）不消费上一用例的残留。
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

  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }
});

describe("AccountsView 行菜单冒烟（issue #551：右键 + 「⋯」双入口）", () => {
  /** 表格数据行。 */
  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  /** 行菜单：视图内唯一 NDropdown（按 options 含 edit key 识别）。 */
  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
  }

  /** 右键指定行打开菜单。 */
  async function openMenuOnRow(wrapper: ReturnType<typeof mount>, index = 0) {
    await bodyRows(wrapper)[index].trigger("contextmenu");
    await flushPromises();
  }

  /** 点击指定行操作列「⋯」按钮打开菜单（第二入口，aria-label 随界面语言）。 */
  async function openMenuOnMoreButton(wrapper: ReturnType<typeof mount>, index = 0) {
    await wrapper.findAll('button[aria-label="更多操作"]')[index].trigger("click");
    await flushPromises();
  }

  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
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
    expect(menuKeys(wrapper)).toEqual(["edit", "adjust-balance", "menu-divider", "delete"]);
  });

  it("操作列「⋯」按钮弹出行菜单（第二入口，与右键共用同一菜单）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnMoreButton(wrapper, 1);
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual(["edit", "adjust-balance", "menu-divider", "delete"]);
  });

  it("菜单选中「编辑」分派到编辑弹窗并回填目标行（第二行「银行」）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openMenuOnRow(wrapper, 1);
    // NDropdown onSelect 装配缝（fireProp 单点窄化）：分派到编辑弹窗
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
    const editModal = wrapper
      .findAllComponents(NModal)
      .find((m) => m.props("title") === "编辑账户")!;
    expect(editModal.props("show")).toBe(true);
    // 回填目标行：编辑弹窗表单（全局第 2 个 NForm，第 1 个为顶部新增表单）内
    // 首个 NInput 即名称字段，值为右键目标行的账户名——分派到的确是收起菜单
    // 瞬间的目标行
    const editForm = wrapper.findAllComponents(NForm)[1];
    expect(editForm.findComponent(NInput).props("value")).toBe("银行");
  });
});

describe("AccountsView 移动档（issue #847 / ADR-0088 决策 11 票⑦，词汇表「窗口分级」）", () => {
  /** 视图顶层调用 useDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  /** 表格数据行。 */
  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  /** 行菜单：视图内唯一 NDropdown（按 options 含 edit key 识别）。 */
  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  function menuKeys(wrapper: ReturnType<typeof mount>) {
    return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
  }

  /** 行「⋯」按钮（aria-label 随界面语言）。 */
  function moreButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button[aria-label="更多操作"]');
  }

  /** 本 describe 专用夹具：第二行为 bank 类型，副行「银行卡 · CNY」与名称
   * 「银行」可区分（验证类型/币种确实并入副行而非丢失）。 */
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
    // 类型与币种不丢：并入名称副行（第二行「银行」→ 银行卡 · CNY）
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
    const expected = ["edit", "adjust-balance", "menu-divider", "delete"];

    // 触控轴无右键手势可达性要求，但两轴行为一致是验收硬条件：右键入口结果…
    await bodyRows(wrapper)[0].trigger("contextmenu");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(menuKeys(wrapper)).toEqual(expected);
    // …点外部关闭（非模态家族既有通道）后，「⋯」入口得同一闭集
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
    // 副行不是金额：类型与币种照常可读
    expect(bodyRows(wrapper)[1].text()).toContain("银行卡 · CNY");
    amountPrivacyEnabled.value = false;
    await nextTick();
    expect(wrapper.text()).toContain(visible);
  });
});

// ---------------------------------------------------------------------------
// 信用卡档案字段（spec #1327 / ADR-0119）：列表使用率小字、新增表单条件字段、
// 编辑弹窗档案输入与只读摘要、提交三字段。
describe("AccountsView 信用卡档案（spec #1327 / ADR-0119）", () => {
  /** 本 describe 专用夹具：两张信用卡（6% 与 95% 使用率）、一张已还清卡、
   * 一张未设额度卡、一笔现金账户（验证非信用卡行不受影响）。 */
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
      // update_account 是保存路径的命令面：无桩即「未命中报错」让提交失败（弹窗不关）。
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

  /** 行菜单（选项含 edit key）与打开某行编辑弹窗。 */
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
    // 额度以元回填（5,000,000 分 = 50000 元）
    expect(form.findAllComponents(NInputNumber)[0].props("value")).toBe(50_000);
    expect(form.findAllComponents(NInputNumber)[1].props("value")).toBe(5);
    expect(form.findAllComponents(NInputNumber)[2].props("value")).toBe(25);
    // 只读摘要：额度用量按派生口径 + 下次账单节点是两个具体日期
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
  /** 视图顶层调用 useDialog，与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  /** 行菜单：视图内唯一 NDropdown（按 options 含 edit key 识别）。 */
  function rowMenu(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NDropdown)
      .find((d) => (d.props("options") as Array<{ key?: string }>).some((o) => o.key === "edit"))!;
  }

  /** 打开指定行的编辑弹窗（右键 + onSelect 装配缝）。 */
  async function openEditOnRow(wrapper: ReturnType<typeof mount>, index: number) {
    const row = wrapper.findAll(".n-data-table-tbody .n-data-table-tr")[index];
    await row.trigger("contextmenu");
    await flushPromises();
    fireProp(rowMenu(wrapper), "onSelect", "edit");
    await flushPromises();
  }

  /** 编辑弹窗名称输入（全局第 2 个 NForm 内首个 NInput）。 */
  function editNameInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAllComponents(NForm)[1].findComponent(NInput);
  }

  /** 编辑弹窗保存按钮（NForm 内主按钮，先例信用卡 describe）。 */
  function editSaveButton(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NForm)[1]
      .findAll("button")
      .find((b) => b.text() === "保存")!;
  }

  it("目标绑定账户：名称输入禁用（随动只读）且提交不带 name 键", async () => {
    // 账户身份由绑定派生：acc-1「现金」被储蓄目标绑定（改名唯一入口是目标编辑）
    goalProgress = [
      makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1", account_id: "acc-1" }) }),
    ];
    // 绑定快照落位后挂载视图：分组派生后 acc-1 在储蓄目标组（第一条数据行 = 目标
    // 账户行）；goals store 不在参考预热五命令内，本用例重布线（含保存命令面）
    // 后显式 refresh 落位，不消费上一用例残留。
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
    // 渲染效果：目标账户名称输入禁用（账户侧无独立改名入口）
    expect(editNameInput(wrapper).props("disabled")).toBe(true);

    await editSaveButton(wrapper).trigger("click");
    await flushPromises();
    // 调用事实：update_account 载荷不含 name 键（缺席 = 不改，不回写陈旧名）
    const args = lastInvokeArgs("update_account") as {
      id: string;
      input: Record<string, unknown>;
    };
    expect(args.id).toBe("acc-1");
    expect(args.input).not.toHaveProperty("name");
    expect(args.input).toHaveProperty("currency_code", "CNY"); // 载荷确实在发（非空调用）
  });

  it("普通账户编辑不受影响：名称输入可用（不误伤）", async () => {
    goalProgress = [
      makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1", account_id: "acc-1" }) }),
    ];
    const wrapper = mountView();
    await flushPromises();

    // 第二行「银行」未被任何目标绑定
    await openEditOnRow(wrapper, 1);
    expect(editNameInput(wrapper).props("disabled")).toBe(false);
  });
});

describe("AccountsView 储蓄目标分组与类型标签（issue #1755 / ADR-0133 决策 2）", () => {
  /** 视图顶层调用 useDialog，与 App.vue 同构需 NDialogProvider 包裹。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(AccountsView) },
    });
  }

  /** 数据表格行：分组卡与普通列表卡各一张表，DOM 顺序 = 分组卡在前。 */
  function bodyRows(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
  }

  /** 行菜单（选项含 edit key）与打开某行编辑弹窗（同 #1752 describe 帮手）。 */
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

  it("在用目标账户归入「储蓄目标」分组：类型标签覆写、余额原值展示（读数不受分组影响）", async () => {
    goalProgress = [makeGoalPair({ id: "acc-1" }).progress];
    // 绑定快照落位后挂载视图：beforeEach 的 store 快照是空集（goalProgress 在其后赋值）
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();
    const rows = bodyRows(wrapper);
    // 分组卡列于普通列表之前：第一条数据行 = 目标账户行
    expect(rows).toHaveLength(2);
    expect(rows[0].text()).toContain("现金");
    // 类型标签覆写为「储蓄目标」，不显示 other 类型的「其他」（特殊性不靠类型值表达）
    expect(rows[0].text()).toContain("储蓄目标");
    expect(rows[0].text()).not.toContain("其他");
    // 余额原值展示：同一 balances 快照拆分、不做任何加减，读数不受分组影响
    expect(rows[0].text()).toContain(formatAmount(1000, CNY));
    // 未绑定行留在普通列表（第二条数据行 = acc-2「银行」）
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
    // 无绑定：分组标题（储蓄目标）不出现——目标组空集不渲染空卡
    expect(wrapper.text()).not.toContain("储蓄目标");
    // 普通 other 账户不被误伤：仍在原列表原位、类型标签显示「其他」
    const rows = bodyRows(wrapper);
    expect(rows).toHaveLength(3);
    expect(rows[2].text()).toContain("备用现金");
    expect(rows[2].text()).toContain("其他");
  });

  it("编辑弹窗：目标账户的只读类型显示「储蓄目标」", async () => {
    goalProgress = [makeGoalPair({ id: "acc-1" }).progress];
    // 绑定快照落位后挂载视图（同上：beforeEach 快照为空集）
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    const wrapper = mountView();
    await flushPromises();
    await openEditOnRow(wrapper, 0);
    // 只读类型输入框（NForm 内第 2 个 NInput）：值覆写为「储蓄目标」（NInput 的
    // value 落在 input 元素上，不进 text()，按 props 断言）
    expect(wrapper.findAllComponents(NForm)[1].findAllComponents(NInput)[1].props("value")).toBe(
      "储蓄目标",
    );
  });
});
