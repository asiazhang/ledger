import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { findButtonByTestId, findBodyButtonByTestId } from "@ledger/test-support/dom";
import { t } from "@ledger/i18n";
import { mount, flushPromises, DOMWrapper } from "@vue/test-utils";
import { NDialogProvider, NMessageProvider } from "naive-ui";
import { h } from "vue";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink } from "./factories";
import SavingsGoalsView from "@/views/SavingsGoalsView.vue";
import SavingsGoalFormModal from "@/savings-goal/SavingsGoalFormModal.vue";
import { makeSavingsGoal, makeSavingsGoalProgress } from "./factories";
import type { SavingsGoalProgress } from "@ledger/types";

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector);
}

/** 弹窗内输入框：NInput 外层带 data-testid，真 input 在内部（先例 PhysicalAssetsView）。 */
function formInput(testid: string) {
  const modal = bodyQuery('[data-testid="savings-goal-form-modal"]')!;
  return new DOMWrapper<HTMLInputElement>(modal.querySelector(`[data-testid="${testid}"] input`));
}

function saveButton() {
  return findBodyButtonByTestId("savings-goal-save")!;
}

let progress: SavingsGoalProgress[];

/**
 * 视图布线：进度读数是动态行为（创建后重拉回新清单），归 overrides——
 * 求值序 defaults → 参考预热兜底（ADR-0085），先例 PhysicalAssetsView。
 */
const VIEW_OVERRIDES = {
  savings_goal_progress: () => Promise.resolve(progress),
  create_savings_goal: (args?: Record<string, unknown>) => {
    const { input } = args as {
      input: { name: string; target_amount_cents: number; deadline: string | null };
    };
    const id = `goal-new-${progress.length + 1}`;
    progress = [
      ...progress,
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({
          id,
          name: input.name,
          target_amount_cents: input.target_amount_cents,
          deadline: input.deadline,
        }),
      }),
    ];
    return Promise.resolve(id);
  },
  update_savings_goal: (args?: Record<string, unknown>) => {
    const { id, input } = args as {
      id: string;
      input: {
        name: string;
        target_amount_cents: number;
        deadline: string | null;
        planned_monthly_cents: number | null;
      };
    };
    // 镜像后端语义：目标行四字段替换，剩余与达成由读数派生（issue #1752）
    progress = progress.map((row) =>
      row.goal.id === id
        ? {
            ...row,
            goal: {
              ...row.goal,
              name: input.name,
              target_amount_cents: input.target_amount_cents,
              deadline: input.deadline,
              planned_monthly_cents: input.planned_monthly_cents,
            },
            remaining_cents: input.target_amount_cents - row.saved_cents,
            achieved: row.saved_cents >= input.target_amount_cents,
          }
        : row,
    );
    return Promise.resolve();
  },
};

/** 删除确认弹层的确认按钮（teleport 到 body；按正向按钮文案定位）。 */
function deleteConfirmButton() {
  const buttons = [...document.body.querySelectorAll(".n-dialog__action button")];
  const positive = buttons.find((b) => b.textContent?.trim() === t("savingsGoals.actions.delete"));
  return positive ? new DOMWrapper(positive) : null;
}

beforeEach(() => {
  progress = [];
  wireInvokeSeam({ overrides: VIEW_OVERRIDES });
});

/** 视图顶层调用 useMessage 与 useDialog（删除二次确认），与 App.vue 同构需
 *  Provider 包裹（AccountsView 测试同款挂法）。 */
function mountView() {
  return mount(NDialogProvider, {
    slots: { default: () => h(NMessageProvider, () => h(SavingsGoalsView)) },
  });
}

describe("SavingsGoalsView 储蓄目标视图（spec #1750 / issue #1751 建档编辑 / #1753 双向推算）", () => {
  it("挂载即拉取：调用进度读命令，渲染名称 / 目标额 / 已存 / 还差 / 进行中", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", name: "买车基金", target_amount_cents: 5_000_000 }),
        saved_cents: 2_000_000,
        remaining_cents: 3_000_000,
        achieved: false,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    // 调用事实：进度读命令在挂载时发出
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "savings_goal_progress")).toBe(true);
    // 渲染效果：进度读数可见（金额 = 分 → 元，中文四位分组）
    expect(wrapper.text()).toContain("买车基金");
    expect(wrapper.text()).toContain("5,0000"); // 目标额 50,000 元
    expect(wrapper.text()).toContain("2,0000"); // 已存 20,000 元
    expect(wrapper.text()).toContain("3,0000"); // 还差 30,000 元
    expect(wrapper.text()).toContain("进行中");
  });

  it("达成态渲染：已达成标签在场，还差恒显带符号差值", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-2", name: "旅行基金", target_amount_cents: 1_000_000 }),
        saved_cents: 1_800_000,
        remaining_cents: -800_000,
        achieved: true,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const statusTags = wrapper.findAll('[data-testid="savings-goal-status"]');
    expect(statusTags.length).toBe(1);
    expect(statusTags[0].text()).toContain("已达成");
    expect(wrapper.text()).toContain("1,8000"); // 已存 18,000 元仍可见
    expect(wrapper.text()).toContain("8000"); // 还差 -8,000 元恒显带符号差值（不出「—」）
    expect(wrapper.text()).not.toContain("—");
    expect(wrapper.text()).not.toContain("进行中");
  });

  it("空列表显示空态引导", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="savings-goal-empty-guide"]').exists()).toBe(true);
  });

  it("新建目标：调用 create_savings_goal 携带入参，列表刷出新目标、弹窗关闭", async () => {
    const wrapper = mountView();
    await flushPromises();
    await findButtonByTestId(wrapper, "savings-goal-new").trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="savings-goal-form-modal"]')).not.toBeNull();
    await formInput("savings-goal-name").setValue("教育金");
    await formInput("savings-goal-amount").setValue("80000");
    await saveButton().trigger("click");
    await flushPromises();

    // 调用事实：创建命令携名称、目标金额（元 → 分）与空截止日
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_savings_goal");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: { name: "教育金", target_amount_cents: 8_000_000, deadline: null },
    });
    // 渲染效果：store 重拉后新目标进列表（专属账户读数 = 初始进度）
    expect(wrapper.text()).toContain("教育金");
    expect(wrapper.text()).toContain("8,0000"); // 目标额 80,000 元 = 还差
    // 弹窗关闭（update:show = false）
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toContainEqual([
      false,
    ]);
  });

  it("目标金额非正数：客户端拦截，不调用 create_savings_goal、弹窗不关", async () => {
    const wrapper = mountView();
    await flushPromises();
    await findButtonByTestId(wrapper, "savings-goal-new").trigger("click");
    await flushPromises();
    await formInput("savings-goal-name").setValue("坏目标");
    await formInput("savings-goal-amount").setValue("0");
    await saveButton().trigger("click");
    await flushPromises();

    // 调用事实：零次创建调用
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_savings_goal")).toBe(false);
    // 效果：弹窗未关闭（内容保留可改）
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toBeUndefined();
  });

  it("编辑目标：弹窗回填并提交 update_savings_goal，列表刷出新名新额、弹窗关闭", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({
          id: "goal-1",
          name: "买车基金",
          target_amount_cents: 5_000_000,
          deadline: "2027-06-30",
          planned_monthly_cents: null,
        }),
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    // 编辑入口（操作列）在场并打开编辑弹窗
    await findButtonByTestId(wrapper, "savings-goal-edit").trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="savings-goal-form-modal"]')).not.toBeNull();
    // 回填：名称预填既有目标名；计划月存字段结构性只在编辑模式出现
    expect(formInput("savings-goal-name").element.value).toBe("买车基金");
    expect(formInput("savings-goal-planned-monthly").element.value).toBe("");

    await formInput("savings-goal-name").setValue("换车基金");
    await formInput("savings-goal-amount").setValue("40000");
    await formInput("savings-goal-planned-monthly").setValue("3000");
    await saveButton().trigger("click");
    await flushPromises();

    // 调用事实：update_savings_goal 携 id 与四字段全量载荷（元 → 分）
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_savings_goal");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      id: "goal-1",
      input: {
        name: "换车基金",
        target_amount_cents: 4_000_000,
        deadline: "2027-06-30",
        planned_monthly_cents: 300_000,
      },
    });
    // 渲染效果：列表刷出新名与新目标额（重拉后的读数）
    expect(wrapper.text()).toContain("换车基金");
    expect(wrapper.text()).not.toContain("买车基金");
    expect(wrapper.text()).toContain("4,0000"); // 目标额 40,000 元
    // 弹窗关闭
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toContainEqual([
      false,
    ]);
  });

  it("编辑目标：清空计划月存提交 null（可空列）", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", planned_monthly_cents: 500_000 }),
      }),
    ];
    const wrapper = mountView();
    await flushPromises();
    await findButtonByTestId(wrapper, "savings-goal-edit").trigger("click");
    await flushPromises();

    // 回填：计划月存 500000 分 → 5000 元
    expect(formInput("savings-goal-planned-monthly").element.value).toBe("5000");
    await formInput("savings-goal-planned-monthly").setValue("");
    await saveButton().trigger("click");
    await flushPromises();

    // 调用事实：清除以 null 落定（可空列）
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_savings_goal");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: { planned_monthly_cents: null },
    });
    // 效果：保存成功弹窗关闭
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toContainEqual([
      false,
    ]);
  });

  it("计划月存非正数：客户端拦截，不调用 update_savings_goal、弹窗不关", async () => {
    progress = [makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1" }) })];
    const wrapper = mountView();
    await flushPromises();
    await findButtonByTestId(wrapper, "savings-goal-edit").trigger("click");
    await flushPromises();
    await formInput("savings-goal-planned-monthly").setValue("0");
    await saveButton().trigger("click");
    await flushPromises();

    // 调用事实：零次编辑调用
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "update_savings_goal")).toBe(false);
    // 效果：弹窗未关闭（内容保留可改）
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toBeUndefined();
  });

  // —— 双向推算展示（issue #1753）：推算读数全部来自后端，组件零自算 ——

  it("无截止 + 计划节奏：推算列显示节奏来源、还差 N 个月与预计年月", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", deadline: null }),
        saved_cents: 0,
        remaining_cents: 1_200_000,
        pace_monthly_cents: 100_000,
        pace_source: "plan",
        eta_months: 12,
        eta_month: "2027-09",
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const cell = wrapper.find('[data-testid="savings-goal-projection"]');
    expect(cell.exists()).toBe(true);
    // 渲染效果：节奏来源闭集二值（按计划）、ETA 月数与预计年月（金额 = 分 → 元）
    expect(cell.text()).toContain("按计划 ¥1000/月");
    expect(cell.text()).toContain("还差 12 个月");
    expect(cell.text()).toContain("预计 2027-09 攒够");
    expect(cell.text()).not.toContain("每月需存");
  });

  it("有截止 + 节奏：显示每月需存与落后 / 超前差值", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", deadline: "2027-06-30" }),
        saved_cents: 300_000,
        remaining_cents: 900_000,
        pace_monthly_cents: 50_000,
        pace_source: "manual",
        required_monthly_cents: 100_000,
        pace_delta_cents: -50_000,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const cell = wrapper.find('[data-testid="savings-goal-projection"]');
    expect(cell.text()).toContain("手填 ¥500/月");
    expect(cell.text()).toContain("每月需存 ¥1000");
    expect(cell.text()).toContain("落后 ¥500/月");
    expect(cell.text()).not.toContain("超前");
  });

  it("有截止 + 节奏充足：差值显示超前", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", deadline: "2027-06-30" }),
        pace_monthly_cents: 200_000,
        pace_source: "plan",
        required_monthly_cents: 100_000,
        pace_delta_cents: 100_000,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const cell = wrapper.find('[data-testid="savings-goal-projection"]');
    expect(cell.text()).toContain("超前 ¥1000/月");
    expect(cell.text()).not.toContain("落后");
  });

  it("节奏为零：推算列给设置引导而非虚构时点（双断言）", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", deadline: null }),
        pace_monthly_cents: null,
        pace_source: null,
        eta_months: null,
        eta_month: null,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const cell = wrapper.find('[data-testid="savings-goal-projection"]');
    // 双断言：设置引导在场，虚构时点不在场
    expect(cell.text()).toContain("设置计划月存或挂一条自动转账计划");
    expect(cell.text()).not.toContain("预计");
    expect(cell.text()).not.toContain("还差");
  });

  it("达成态：推算整体退场（不虚构时点与月存）", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", deadline: "2027-06-30" }),
        achieved: true,
        pace_monthly_cents: 100_000,
        pace_source: "plan",
        required_monthly_cents: null,
        pace_delta_cents: null,
      }),
    ];
    const wrapper = mountView();
    await flushPromises();

    const cell = wrapper.find('[data-testid="savings-goal-projection"]');
    expect(cell.text()).toBe("");
    expect(cell.text()).not.toContain("每月需存");
    expect(cell.text()).not.toContain("还差");
  });
});

// ---------------------------------------------------------------------------
// 生命周期守卫（issue #1754）：默认列表只留进行中、归档列表可查（取消归档恢复）、
// 删除守卫（余额非零拒绝引导先转出）与达成后关联计划执行提示。
// ---------------------------------------------------------------------------

/** 镜像后端语义的归档覆写：状态转 archived（分组随读数派生，视图不自算）。 */
const ARCHIVE_OVERRIDES = {
  archive_savings_goal: (args?: Record<string, unknown>) => {
    const { id } = args as { id: string };
    progress = progress.map((row) =>
      row.goal.id === id ? { ...row, goal: { ...row.goal, status: "archived" as const } } : row,
    );
    return Promise.resolve();
  },
  unarchive_savings_goal: (args?: Record<string, unknown>) => {
    const { id } = args as { id: string };
    progress = progress.map((row) =>
      row.goal.id === id ? { ...row, goal: { ...row.goal, status: "active" as const } } : row,
    );
    return Promise.resolve();
  },
};

it("默认列表只显示进行中目标，归档目标只在归档卡片（归档退出默认列表）", async () => {
  progress = [
    makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-live", name: "买车基金" }) }),
    makeSavingsGoalProgress({
      goal: makeSavingsGoal({ id: "goal-arch", name: "旧目标", status: "archived" }),
      saved_cents: 100_000,
    }),
  ];
  const wrapper = mountView();
  await flushPromises();

  // 双断言：进行中在默认列表、归档目标不在默认列表（仅归档卡片）
  expect(wrapper.text()).toContain("买车基金");
  expect(wrapper.findAll('[data-testid="savings-goal-archive"]').length).toBe(1);
  expect(wrapper.text()).toContain("旧目标");
  expect(wrapper.text()).toContain("已归档");
  // 归档卡片行不带归档按钮（只有取消归档 / 删除）
  expect(wrapper.findAll('[data-testid="savings-goal-unarchive"]').length).toBe(1);
});

it("归档目标：调用 archive_savings_goal，读数刷新后移入归档卡片", async () => {
  progress = [makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1" }) })];
  wireInvokeSeam({ overrides: { ...VIEW_OVERRIDES, ...ARCHIVE_OVERRIDES } });
  const wrapper = mountView();
  await flushPromises();

  await findButtonByTestId(wrapper, "savings-goal-archive").trigger("click");
  await flushPromises();

  const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "archive_savings_goal");
  expect(call).toBeTruthy();
  expect(call![1]).toMatchObject({ id: "goal-1" });
  // 渲染效果：退出默认列表、进归档卡片
  expect(wrapper.findAll('[data-testid="savings-goal-archive"]').length).toBe(0);
  expect(wrapper.findAll('[data-testid="savings-goal-unarchive"]').length).toBe(1);
});

it("取消归档：归档卡片调用 unarchive_savings_goal，读数刷新回默认列表", async () => {
  progress = [
    makeSavingsGoalProgress({
      goal: makeSavingsGoal({ id: "goal-1", status: "archived" }),
      saved_cents: 100_000,
    }),
  ];
  wireInvokeSeam({ overrides: { ...VIEW_OVERRIDES, ...ARCHIVE_OVERRIDES } });
  const wrapper = mountView();
  await flushPromises();

  await findButtonByTestId(wrapper, "savings-goal-unarchive").trigger("click");
  await flushPromises();

  const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "unarchive_savings_goal");
  expect(call).toBeTruthy();
  expect(call![1]).toMatchObject({ id: "goal-1" });
  expect(wrapper.findAll('[data-testid="savings-goal-archive"]').length).toBe(1);
});

it("删除目标：确认弹层后调用 delete_savings_goal，目标从列表消失", async () => {
  progress = [
    makeSavingsGoalProgress({
      goal: makeSavingsGoal({ id: "goal-1", status: "archived" }),
      saved_cents: 0,
    }),
  ];
  // 镜像后端语义：余额为零删除成功，读数刷新后目标退出快照
  wireInvokeSeam({
    overrides: {
      ...VIEW_OVERRIDES,
      delete_savings_goal: (args?: Record<string, unknown>) => {
        const { id } = args as { id: string };
        progress = progress.filter((row) => row.goal.id !== id);
        return Promise.resolve();
      },
    },
  });
  const wrapper = mountView();
  await flushPromises();

  await findButtonByTestId(wrapper, "savings-goal-delete").trigger("click");
  await flushPromises();
  // 二次确认：确认前零次删除调用
  expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "delete_savings_goal")).toBe(false);

  await deleteConfirmButton()!.trigger("click");
  await flushPromises();

  const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "delete_savings_goal");
  expect(call).toBeTruthy();
  expect(call![1]).toMatchObject({ id: "goal-1" });
  // 渲染效果：目标与归档卡片一并消失
  expect(wrapper.text()).not.toContain("买车基金");
  expect(wrapper.find('[data-testid="savings-goal-archived-card"]').exists()).toBe(false);
});

it("删除被拒（余额非零）：码化错误文案在场（引导先转出），目标仍在", async () => {
  progress = [makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1" }) })];
  // 错误 toast 经 Loadable 默认策略走模块级 sink（断言只看 sink 面，BudgetView 同款）
  const sink = makeFakeSink();
  registerToastSink(sink);
  wireInvokeSeam({
    overrides: {
      ...VIEW_OVERRIDES,
      delete_savings_goal: () =>
        Promise.reject({
          kind: "Invalid",
          code: "savings-goal.delete-balance-nonzero",
          message: "目标账户仍有余额，请先转出后再删除",
          params: [],
        }),
    },
  });
  const wrapper = mountView();
  await flushPromises();

  await findButtonByTestId(wrapper, "savings-goal-delete").trigger("click");
  await flushPromises();
  await deleteConfirmButton()!.trigger("click");
  await flushPromises();

  // 双断言：引导文案经 sink 提示在场、目标行未消失
  expect(sink.error.mock.calls.some((c) => String(c[0]).includes("请先转出后再删除"))).toBe(true);
  expect(wrapper.findAll('[data-testid="savings-goal-edit"]').length).toBe(1);
});

it("达成 + 关联计划仍在执行：提示在场（计划状态不由目标域改动）", async () => {
  progress = [
    makeSavingsGoalProgress({
      goal: makeSavingsGoal({ id: "goal-1" }),
      achieved: true,
      pace_monthly_cents: 100_000,
      pace_source: "plan",
    }),
  ];
  const wrapper = mountView();
  await flushPromises();

  const hint = wrapper.find('[data-testid="savings-goal-plan-hint"]');
  expect(hint.exists()).toBe(true);
  expect(hint.text()).toContain("关联计划仍在执行");
  // 计划节奏照常回显（计划仍在执行的可观察面）
  expect(wrapper.find('[data-testid="savings-goal-projection"]').exists()).toBe(true);
});

it("达成但无在用计划（手填节奏）：不显示关联计划提示", async () => {
  progress = [
    makeSavingsGoalProgress({
      goal: makeSavingsGoal({ id: "goal-1" }),
      achieved: true,
      pace_monthly_cents: 100_000,
      pace_source: "manual",
    }),
  ];
  const wrapper = mountView();
  await flushPromises();

  expect(wrapper.find('[data-testid="savings-goal-plan-hint"]').exists()).toBe(false);
});
