<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import {
  NButton,
  NCard,
  NDataTable,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { formatAmount } from "@ledger/money";
import { t } from "@ledger/i18n";
import { useLoadable } from "@ledger/loadable";
import SavingsGoalFormModal from "@/savings-goal/SavingsGoalFormModal.vue";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { useAppDialog } from "@/composables/useAppDialog";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import { useReferenceStore } from "@/stores/reference";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import type { SavingsGoal, SavingsGoalProgress } from "@ledger/types";
/**
 * 储蓄目标视图（spec #1750 / issue #1751 建档、#1752 编辑、#1754 生命周期守卫 /
 * ADR-0133）：目标清单（进行中）+ 蓄水进度（已存 / 还差 / 达成态与关联计划执行
 * 提示）+ 归档列表（取消归档 / 删除）+ 新建与编辑入口。
 *
 * 进度全部来自后端读数（`useSavingsGoalsStore`，self-init + `ledger:changed`
 * 静默重拉）——已存 = 专属账户余额（余额缓存口径），本组件零业务逻辑、不自算
 * 进度；达成行的「还差」照显带符号差值（词汇表「蓄水进度」：只输出金额差值，
 * 无百分比口径），达成态由状态列表达。
 * 金额展示统一走 `formatAmount`（数字分组与隐私掩码随行生效）。
 */
const savingsGoalsStore = useSavingsGoalsStore();
const reference = useReferenceStore();
const message = useMessage();
const dialog = useAppDialog();

// 移动档（先例实物资产视图）：固定列宽总和由横向滚动吸收，桌面档不挂。
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

/** 目标币种（= 专属账户币种）的字典查得：进度行随行携带币种码。 */
function currencyOf(row: SavingsGoalProgress) {
  return reference.getCurrency(row.currency_code);
}

// —— 新建 / 编辑弹窗（ModalIntent 单成员意图闭集扩展，ADR-0072；编辑随
//    issue #1752 接入）——
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：显示由「意图非空」
// 派生（无独立 show 布尔），序号随开启递增驱动表单重建（:key=formSeq）。
interface SavingsGoalFormIntent {
  mode: "create";
}

interface SavingsGoalEditIntent {
  mode: "edit";
  goal: SavingsGoal;
}

const {
  intent: formIntent,
  seq: formSeq,
  open: openFormIntent,
  close: closeForm,
} = useModalIntent<SavingsGoalFormIntent | SavingsGoalEditIntent>();

/** 待编辑目标（null = 新建模式）：四字段回填与全量替换由弹窗承载。 */
const editingGoal = computed(() =>
  formIntent.value?.mode === "edit" ? formIntent.value.goal : null,
);

function openCreate() {
  openFormIntent({ mode: "create" });
}

function openEdit(goal: SavingsGoal) {
  openFormIntent({ mode: "edit", goal });
}

// —— 归档 / 取消归档 / 删除（issue #1754：生命周期守卫，守卫归后端域）——
// 异步失败统一走 Loadable：错误捕获、文案归一（码化错误经 errorMessage 按码
// 本地化）与 toast 策略内化在 Loadable 单点（ADR-0040），视图不手搓 catch toast。
/** 动作目标（闭包内自读的响应式参数——useLoadable 任务以 0 元闭包声明）。 */
const actionTarget = ref<SavingsGoal | null>(null);

/** 归档：退出默认列表进归档列表，账户 / 流水 / 关联计划原样保留。 */
const archiveRun = useLoadable(async () => {
  const goal = actionTarget.value;
  if (goal) await savingsGoalsStore.archive(goal.id);
});

/** 取消归档：恢复进行中回默认列表。 */
const unarchiveRun = useLoadable(async () => {
  const goal = actionTarget.value;
  if (goal) await savingsGoalsStore.unarchive(goal.id);
});

/** 删除：余额非零被后端码化拒绝（toast 引导先转出），余额为零后端级联软删
 *  专属账户、流水保留。 */
const deleteRun = useLoadable(async () => {
  const goal = actionTarget.value;
  if (goal) await savingsGoalsStore.remove(goal.id);
});

async function archiveGoal(goal: SavingsGoal) {
  actionTarget.value = goal;
  if ((await archiveRun.run()) !== null) message.success(t("savingsGoals.msg.archived"));
}

async function unarchiveGoal(goal: SavingsGoal) {
  actionTarget.value = goal;
  if ((await unarchiveRun.run()) !== null) message.success(t("savingsGoals.msg.unarchived"));
}

/** 删除：useAppDialog 二次确认（与账户删除同语义）。 */
function confirmDelete(goal: SavingsGoal) {
  dialog.warning({
    title: t("savingsGoals.msg.deleteConfirmTitle"),
    content: t("savingsGoals.msg.deleteConfirmContent", { name: goal.name }),
    positiveText: t("savingsGoals.actions.delete"),
    negativeText: t("savingsGoals.form.cancel"),
    maskClosable: false,
    onPositiveClick: async () => {
      actionTarget.value = goal;
      if ((await deleteRun.run()) !== null) message.success(t("savingsGoals.msg.deleted"));
    },
  });
}
const columns: DataTableColumns<SavingsGoalProgress> = [
  {
    title: () => t("savingsGoals.columns.name"),
    key: "goal",
    width: 170,
    render: (row) => row.goal.name,
  },
  {
    title: () => t("savingsGoals.columns.target"),
    key: "target_amount_cents",
    width: 130,
    render: (row) => formatAmount(row.goal.target_amount_cents, currencyOf(row)),
  },
  {
    title: () => t("savingsGoals.columns.saved"),
    key: "saved_cents",
    width: 130,
    render: (row) => formatAmount(row.saved_cents, currencyOf(row)),
  },
  {
    // 还差 = 目标额 − 已存的带符号差值恒显（词汇表「蓄水进度」：只输出金额差值、
    // 无百分比口径；超额存入后为负，达成态由 status 列表达）。
    title: () => t("savingsGoals.columns.remaining"),
    key: "remaining_cents",
    width: 130,
    render: (row) => formatAmount(row.remaining_cents, currencyOf(row)),
  },
  {
    // 双向推算（issue #1753）：无截止正推 ETA（还差 N 个月 / 预计年月）、有截止
    // 反推所需月存 + 落后 / 超前差值；节奏来源闭集二值随行首可见（计划 / 手填）；
    // 节奏为零给设置引导而非虚构时点（词汇表 ETA / 所需月存——纯展示态由后端
    // 读数派生，本组件零业务逻辑、不自算）。
    title: () => t("savingsGoals.columns.projection"),
    key: "projection",
    width: 240,
    render: (row) => {
      const lines: string[] = [];
      // 达成 = 读时派生纯展示态：时点已无意义，推算整体退场（状态列表达达成）。
      if (!row.achieved) {
        // 节奏行：来源闭集二值随行可见（按计划 / 手填），节奏为零不给本行。
        if (row.pace_monthly_cents !== null) {
          lines.push(
            t(
              row.pace_source === "plan"
                ? "savingsGoals.projection.planPace"
                : "savingsGoals.projection.manualPace",
              { amount: formatAmount(row.pace_monthly_cents, currencyOf(row)) },
            ),
          );
        }
        if (row.goal.deadline === null) {
          // 无截止正推：还差 N 个月 / 预计年月；节奏为零 → 设置引导（不虚构时点）。
          if (row.eta_months !== null) {
            lines.push(t("savingsGoals.projection.etaMonths", { n: row.eta_months }));
            if (row.eta_month !== null) {
              lines.push(t("savingsGoals.projection.etaMonth", { ym: row.eta_month }));
            }
          } else {
            lines.push(t("savingsGoals.projection.setupGuide"));
          }
        } else if (row.required_monthly_cents !== null) {
          // 有截止反推：每月需存 + 落后 / 超前差值（差值缺席给对比引导）。
          lines.push(
            t("savingsGoals.projection.requiredMonthly", {
              amount: formatAmount(row.required_monthly_cents, currencyOf(row)),
            }),
          );
          if (row.pace_delta_cents !== null) {
            const amount = formatAmount(Math.abs(row.pace_delta_cents), currencyOf(row));
            lines.push(
              row.pace_delta_cents >= 0
                ? t("savingsGoals.projection.ahead", { amount })
                : t("savingsGoals.projection.behind", { amount }),
            );
          } else {
            lines.push(t("savingsGoals.projection.requiredGuide"));
          }
        } else {
          // 截止日已过：不反推所需月存、不虚构时点。
          lines.push(t("savingsGoals.projection.deadlinePassed"));
        }
      }
      // 逐行渲染：节奏 / 方向 / 引导各自成行，不拼成连续一行。
      return h(
        "div",
        { class: "savings-goal-projection", "data-testid": "savings-goal-projection" },
        lines.map((line) => h("div", line)),
      );
    },
  },
  {
    title: () => t("savingsGoals.columns.deadline"),
    key: "deadline",
    width: 110,
    render: (row) => row.goal.deadline ?? "—",
  },
  {
    // 达成态 = 读时派生的纯展示态（余额 ≥ 目标额），不触发任何自动动作；
    // 关联计划仍在执行（节奏来源 = 计划 = 在用计划在场）时提示用户处理，
    // 计划状态不由目标域改动（两域边界，词汇表「达成与归档」）。
    title: () => t("savingsGoals.columns.status"),
    key: "status",
    width: 150,
    render: (row) => {
      const children = [
        h(
          NTag,
          {
            size: "small",
            type: row.achieved ? "success" : "default",
            bordered: false,
            "data-testid": "savings-goal-status",
          },
          () =>
            row.achieved ? t("savingsGoals.status.achieved") : t("savingsGoals.status.inProgress"),
        ),
      ];
      if (row.achieved && row.pace_source === "plan") {
        children.push(
          h(
            "div",
            {
              class: "savings-goal-plan-hint",
              "data-testid": "savings-goal-plan-hint",
            },
            t("savingsGoals.achievedPlanHint"),
          ),
        );
      }
      return h("div", children);
    },
  },
  {
    // 操作列（issue #1752 / #1754）：编辑 + 归档——新建走卡片按钮，删除经
    // 二次确认（余额非零由后端守卫码化拒绝）。
    title: () => t("savingsGoals.columns.actions"),
    key: "actions",
    width: 130,
    render: (row) =>
      h(NSpace, { size: 2 }, () => [
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "primary",
            "data-testid": "savings-goal-edit",
            onClick: () => openEdit(row.goal),
          },
          () => t("savingsGoals.actions.edit"),
        ),
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "default",
            "data-testid": "savings-goal-archive",
            onClick: () => archiveGoal(row.goal),
          },
          () => t("savingsGoals.actions.archive"),
        ),
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "error",
            "data-testid": "savings-goal-delete",
            onClick: () => confirmDelete(row.goal),
          },
          () => t("savingsGoals.actions.delete"),
        ),
      ]),
  },
];

// —— 归档列表（issue #1754）：归档目标可查（历史与流水保留）——
const archivedColumns: DataTableColumns<SavingsGoalProgress> = [
  {
    title: () => t("savingsGoals.columns.name"),
    key: "goal",
    width: 170,
    render: (row) => row.goal.name,
  },
  {
    title: () => t("savingsGoals.columns.target"),
    key: "target_amount_cents",
    width: 130,
    render: (row) => formatAmount(row.goal.target_amount_cents, currencyOf(row)),
  },
  {
    title: () => t("savingsGoals.columns.saved"),
    key: "saved_cents",
    width: 130,
    render: (row) => formatAmount(row.saved_cents, currencyOf(row)),
  },
  {
    title: () => t("savingsGoals.columns.actions"),
    key: "archived-actions",
    width: 150,
    render: (row) =>
      h(NSpace, { size: 2 }, () => [
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "primary",
            "data-testid": "savings-goal-unarchive",
            onClick: () => unarchiveGoal(row.goal),
          },
          () => t("savingsGoals.actions.unarchive"),
        ),
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "error",
            "data-testid": "savings-goal-delete",
            onClick: () => confirmDelete(row.goal),
          },
          () => t("savingsGoals.actions.delete"),
        ),
      ]),
  },
];
const archivedTableScrollX = sumFixedColumnWidths(archivedColumns);

/** 横向滚动下限 = 固定列宽总和（列定义之后单点派生，桌面档不消费）。 */
const tableScrollX = sumFixedColumnWidths(columns);
const listTitle = computed(() => t("savingsGoals.listTitle"));

onMounted(() => {
  // store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void savingsGoalsStore.refresh().catch(() => {
    /* 失败信号已由 status 承载 */
  });
});
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="listTitle" size="small">
      <template #header-extra>
        <NButton type="primary" data-testid="savings-goal-new" @click="openCreate">
          {{ t("savingsGoals.newButton") }}
        </NButton>
      </template>
      <NDataTable
        :columns="columns"
        :data="savingsGoalsStore.activeGoals"
        :bordered="false"
        size="small"
        :scroll-x="isMobileTier ? tableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="savings-goal-empty-guide">{{ t("savingsGoals.emptyGuide") }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 归档列表（issue #1754）：归档可查——历史与流水保留；取消归档恢复，
         删除受余额守卫（非零拒绝引导先转出，为零级联软删专属账户）。 -->
    <NCard
      v-if="savingsGoalsStore.archivedGoals.length > 0"
      :title="t('savingsGoals.archivedTitle')"
      size="small"
    >
      <NDataTable
        :columns="archivedColumns"
        :data="savingsGoalsStore.archivedGoals"
        :bordered="false"
        size="small"
        :scroll-x="isMobileTier ? archivedTableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="savings-goal-archived-empty">{{
            t("savingsGoals.archivedTitle")
          }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建弹窗：显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC /
         取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <SavingsGoalFormModal
      :key="formSeq"
      :show="formIntent !== null"
      :editing="editingGoal"
      @update:show="(v: boolean) => (v ? undefined : closeForm())"
    />
  </NSpace>
</template>
