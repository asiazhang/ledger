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

function formInput(testid: string) {
  const modal = bodyQuery('[data-testid="savings-goal-form-modal"]')!;
  return new DOMWrapper<HTMLInputElement>(modal.querySelector(`[data-testid="${testid}"] input`));
}

function saveButton() {
  return findBodyButtonByTestId("savings-goal-save")!;
}

let progress: SavingsGoalProgress[];

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

function deleteConfirmButton() {
  const buttons = [...document.body.querySelectorAll(".n-dialog__action button")];
  const positive = buttons.find((b) => b.textContent?.trim() === t("savingsGoals.actions.delete"));
  return positive ? new DOMWrapper(positive) : null;
}

beforeEach(() => {
  progress = [];
  wireInvokeSeam({ overrides: VIEW_OVERRIDES });
});

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

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "savings_goal_progress")).toBe(true);
    expect(wrapper.text()).toContain("买车基金");
    expect(wrapper.text()).toContain("5,0000");
    expect(wrapper.text()).toContain("2,0000");
    expect(wrapper.text()).toContain("3,0000");
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
    expect(wrapper.text()).toContain("1,8000");
    expect(wrapper.text()).toContain("8000");
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

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_savings_goal");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: { name: "教育金", target_amount_cents: 8_000_000, deadline: null },
    });
    expect(wrapper.text()).toContain("教育金");
    expect(wrapper.text()).toContain("8,0000");
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

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_savings_goal")).toBe(false);
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

    await findButtonByTestId(wrapper, "savings-goal-edit").trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="savings-goal-form-modal"]')).not.toBeNull();
    expect(formInput("savings-goal-name").element.value).toBe("买车基金");
    expect(formInput("savings-goal-planned-monthly").element.value).toBe("");

    await formInput("savings-goal-name").setValue("换车基金");
    await formInput("savings-goal-amount").setValue("40000");
    await formInput("savings-goal-planned-monthly").setValue("3000");
    await saveButton().trigger("click");
    await flushPromises();

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
    expect(wrapper.text()).toContain("换车基金");
    expect(wrapper.text()).not.toContain("买车基金");
    expect(wrapper.text()).toContain("4,0000");
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

    expect(formInput("savings-goal-planned-monthly").element.value).toBe("5000");
    await formInput("savings-goal-planned-monthly").setValue("");
    await saveButton().trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_savings_goal");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: { planned_monthly_cents: null },
    });
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

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "update_savings_goal")).toBe(false);
    expect(wrapper.findComponent(SavingsGoalFormModal).emitted("update:show")).toBeUndefined();
  });

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

  expect(wrapper.text()).toContain("买车基金");
  expect(wrapper.findAll('[data-testid="savings-goal-archive"]').length).toBe(1);
  expect(wrapper.text()).toContain("旧目标");
  expect(wrapper.text()).toContain("已归档");
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
  expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "delete_savings_goal")).toBe(false);

  await deleteConfirmButton()!.trigger("click");
  await flushPromises();

  const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "delete_savings_goal");
  expect(call).toBeTruthy();
  expect(call![1]).toMatchObject({ id: "goal-1" });
  expect(wrapper.text()).not.toContain("买车基金");
  expect(wrapper.find('[data-testid="savings-goal-archived-card"]').exists()).toBe(false);
});

it("删除被拒（余额非零）：码化错误文案在场（引导先转出），目标仍在", async () => {
  progress = [makeSavingsGoalProgress({ goal: makeSavingsGoal({ id: "goal-1" }) })];
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
