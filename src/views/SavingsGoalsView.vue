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
const savingsGoalsStore = useSavingsGoalsStore();
const reference = useReferenceStore();
const message = useMessage();
const dialog = useAppDialog();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

function currencyOf(row: SavingsGoalProgress) {
  return reference.getCurrency(row.currency_code);
}

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

const editingGoal = computed(() =>
  formIntent.value?.mode === "edit" ? formIntent.value.goal : null,
);

function openCreate() {
  openFormIntent({ mode: "create" });
}

function openEdit(goal: SavingsGoal) {
  openFormIntent({ mode: "edit", goal });
}

const actionTarget = ref<SavingsGoal | null>(null);

const archiveRun = useLoadable(async () => {
  const goal = actionTarget.value;
  if (goal) await savingsGoalsStore.archive(goal.id);
});

const unarchiveRun = useLoadable(async () => {
  const goal = actionTarget.value;
  if (goal) await savingsGoalsStore.unarchive(goal.id);
});

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
    title: () => t("savingsGoals.columns.remaining"),
    key: "remaining_cents",
    width: 130,
    render: (row) => formatAmount(row.remaining_cents, currencyOf(row)),
  },
  {
    title: () => t("savingsGoals.columns.projection"),
    key: "projection",
    width: 240,
    render: (row) => {
      const lines: string[] = [];
      if (!row.achieved) {
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
          if (row.eta_months !== null) {
            lines.push(t("savingsGoals.projection.etaMonths", { n: row.eta_months }));
            if (row.eta_month !== null) {
              lines.push(t("savingsGoals.projection.etaMonth", { ym: row.eta_month }));
            }
          } else {
            lines.push(t("savingsGoals.projection.setupGuide"));
          }
        } else if (row.required_monthly_cents !== null) {
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
          lines.push(t("savingsGoals.projection.deadlinePassed"));
        }
      }
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

const tableScrollX = sumFixedColumnWidths(columns);
const listTitle = computed(() => t("savingsGoals.listTitle"));

onMounted(() => {
  void savingsGoalsStore.refresh().catch(() => {});
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
