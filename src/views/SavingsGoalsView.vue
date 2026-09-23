<script setup lang="ts">
import { computed, h, onMounted } from "vue";
import { NButton, NCard, NDataTable, NSpace, NTag, type DataTableColumns } from "naive-ui";
import { formatAmount } from "@ledger/money";
import { t } from "@ledger/i18n";
import SavingsGoalFormModal from "@/savings-goal/SavingsGoalFormModal.vue";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import { useReferenceStore } from "@/stores/reference";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import type { SavingsGoal, SavingsGoalProgress } from "@ledger/types";
/**
 * 储蓄目标视图（spec #1750 / issue #1751 建档、#1752 编辑 / ADR-0133）：目标
 * 清单 + 蓄水进度（已存 / 还差 / 达成态）+ 新建与编辑入口。
 *
 * 进度全部来自后端读数（`useSavingsGoalsStore`，self-init + `ledger:changed`
 * 静默重拉）——已存 = 专属账户余额（余额缓存口径），本组件零业务逻辑、不自算
 * 进度；达成行的「还差」照显带符号差值（词汇表「蓄水进度」：只输出金额差值，
 * 无百分比口径），达成态由状态列表达。
 * 金额展示统一走 `formatAmount`（数字分组与隐私掩码随行生效）。
 */
const savingsGoalsStore = useSavingsGoalsStore();
const reference = useReferenceStore();

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
      return h(
        "div",
        { class: "savings-goal-projection", "data-testid": "savings-goal-projection" },
        lines,
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
    // 达成态 = 读时派生的纯展示态（余额 ≥ 目标额），不触发任何自动动作。
    title: () => t("savingsGoals.columns.status"),
    key: "status",
    width: 96,
    render: (row) =>
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
  },
  {
    // 操作列（issue #1752）：编辑入口——新建走卡片按钮，行内只承载编辑。
    title: () => t("savingsGoals.columns.actions"),
    key: "actions",
    width: 80,
    render: (row) =>
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
  },
];

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
        :data="savingsGoalsStore.goals"
        :bordered="false"
        size="small"
        :scroll-x="isMobileTier ? tableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="savings-goal-empty-guide">{{ t("savingsGoals.emptyGuide") }}</span>
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
