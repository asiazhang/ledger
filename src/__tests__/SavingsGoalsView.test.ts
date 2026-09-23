import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { findButtonByTestId, findBodyButtonByTestId } from "@ledger/test-support/dom";
import { mount, flushPromises, DOMWrapper } from "@vue/test-utils";
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

beforeEach(() => {
  progress = [];
  wireInvokeSeam({ overrides: VIEW_OVERRIDES });
});

describe("SavingsGoalsView 储蓄目标视图（spec #1750 / issue #1751）", () => {
  it("挂载即拉取：调用进度读命令，渲染名称 / 目标额 / 已存 / 还差 / 进行中", async () => {
    progress = [
      makeSavingsGoalProgress({
        goal: makeSavingsGoal({ id: "goal-1", name: "买车基金", target_amount_cents: 5_000_000 }),
        saved_cents: 2_000_000,
        remaining_cents: 3_000_000,
        achieved: false,
      }),
    ];
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
    await flushPromises();
    expect(wrapper.find('[data-testid="savings-goal-empty-guide"]').exists()).toBe(true);
  });

  it("新建目标：调用 create_savings_goal 携带入参，列表刷出新目标、弹窗关闭", async () => {
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
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
    const wrapper = mount(SavingsGoalsView);
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
});
