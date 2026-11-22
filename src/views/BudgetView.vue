<script setup lang="ts">
import { h, computed, onMounted, ref } from "vue";
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInputNumber,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NProgress,
  NTag,
  NSpin,
  NText,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useLoadable } from "@ledger/loadable";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import {
  MOBILE_CELL_STYLE,
  MOBILE_SUB_STYLE,
  MOBILE_TOUCH_TARGET_STYLE,
} from "@/components/mobile-cells";
import { useReferenceStore } from "@/stores/reference";
import { errorMessage } from "@ledger/utils/errors";
import { yuanToCents, formatAmount, centsToYuan } from "@ledger/money";
import { todayStr } from "@ledger/utils/date";
import {
  BUDGET_PERIODS,
  type BudgetAnnualSummary,
  type BudgetInput,
  type BudgetPeriod,
  type BudgetProgress,
} from "@ledger/types";

const reference = useReferenceStore();
const message = useMessage();
const list = ref<BudgetProgress[]>([]);

const { loading, run: runList } = useLoadable(() => api.budgetProgress());

const summary = ref<BudgetAnnualSummary | null>(null);
const { run: runSummary } = useLoadable(() => api.budgetAnnualSummary());

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const MOBILE_HEAD_STYLE = "display: flex; align-items: center; gap: 6px; min-width: 0;";

const categoryId = ref<string | null>(null);
const amount = ref<number | null>(null);
const createPeriod = ref<BudgetPeriod>("monthly");

interface BudgetEditIntent {
  progress: BudgetProgress;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<BudgetEditIntent>();

const editAmount = ref<number | null>(null);
const editPeriod = ref<BudgetPeriod>("monthly");

const categoryOptions = () =>
  reference.treeCategoryOptions("expense").flatMap((root) => [
    { label: reference.categoryDisplayName(root.key, root.category.name), value: root.key },
    ...(root.children ?? []).map((child) => ({
      label: reference.categoryDisplayName(child.key, child.category.name),
      value: child.key,
    })),
  ]);

const displayCategoryName = (row: BudgetProgress) =>
  reference.categoryDisplayName(row.budget.category_id, row.category_name);

const editingCategoryName = computed(() =>
  editIntent.value
    ? reference.categoryDisplayName(
        editIntent.value.progress.budget.category_id,
        editIntent.value.progress.category_name,
      )
    : "",
);

async function refresh() {
  const [progress, annual] = await Promise.all([runList(), runSummary()]);
  if (progress !== null) list.value = progress;
  if (annual !== null) summary.value = annual;
}

async function create() {
  if (!categoryId.value || amount.value == null) {
    message.warning(t("budget.message.required"));
    return;
  }
  if (amount.value <= 0) {
    message.warning(t("budget.message.positive"));
    return;
  }
  const input: BudgetInput = {
    category_id: categoryId.value,
    period: createPeriod.value,
    amount_cents: yuanToCents(amount.value) ?? 0,
    start_date: todayStr(),
  };
  try {
    await api.createBudget(input);
    message.success(t("budget.message.created"));
    categoryId.value = null;
    amount.value = null;
    createPeriod.value = "monthly";
    await refresh();
  } catch (e) {
    message.error(t("budget.message.createFailed", { message: errorMessage(e) }));
  }
}

function openEdit(row: BudgetProgress) {
  editAmount.value = centsToYuan(row.budget.amount_cents);
  editPeriod.value = row.budget.period;
  openEditIntent({ progress: row });
}

async function saveEdit() {
  const target = editIntent.value;
  if (!target) return;
  if (editAmount.value == null || editAmount.value <= 0) {
    message.warning(t("budget.message.positive"));
    return;
  }
  try {
    await api.updateBudget(target.progress.budget.id, {
      amount_cents: yuanToCents(editAmount.value) ?? 0,
      period: editPeriod.value,
    });
    message.success(t("budget.message.updated"));
    closeEdit();
    await refresh();
  } catch (e) {
    message.error(t("budget.message.updateFailed", { message: errorMessage(e) }));
  }
}

async function remove(id: string) {
  try {
    await api.deleteBudget(id);
    message.success(t("budget.message.deleted"));
    await refresh();
  } catch (e) {
    message.error(t("budget.message.deleteFailed", { message: errorMessage(e) }));
  }
}

function progressPercentage(row: BudgetProgress): number {
  return row.budget.amount_cents > 0
    ? Math.min(100, Math.round((row.spent_cents / row.budget.amount_cents) * 100))
    : 0;
}

const summaryPercentage = computed(() => {
  const s = summary.value;
  if (s === null || s.annual_budget_total_cents <= 0) return 0;
  return Math.min(100, Math.round((s.spent_cents / s.annual_budget_total_cents) * 100));
});

const summaryStatsStyle = computed(() => ({
  display: "flex",
  alignItems: "flex-end",
  gap: isMobileTier.value ? "16px" : "32px",
  flexWrap: "wrap" as const,
}));
const summaryPrimaryStyle = computed(() => ({ fontSize: isMobileTier.value ? "19px" : "26px" }));
const summarySecondaryStyle = computed(() => ({
  fontSize: isMobileTier.value ? "17px" : "21px",
}));
const SUMMARY_STAT_LABEL_STYLE = "font-size: 12px;";

function progressStatus(row: BudgetProgress): "error" | "success" {
  return row.over_budget ? "error" : "success";
}

const columns = computed<DataTableColumns<BudgetProgress>>(() => {
  if (isMobileTier.value) {
    return [
      {
        title: t("budget.list.colCategory"),
        key: "category_name",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h("div", { style: MOBILE_HEAD_STYLE }, [
              h("span", displayCategoryName(row)),
              row.over_budget
                ? h(NTag, { type: "error" }, () => t("budget.status.over"))
                : h(NTag, { type: "success" }, () => t("budget.status.normal")),
            ]),
            h("span", { style: MOBILE_SUB_STYLE }, t(`budget.period.${row.budget.period}`)),
          ]),
      },
      {
        title: t("budget.list.colProgress"),
        key: "progress",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h(NProgress, {
              type: "line",
              percentage: progressPercentage(row),
              status: progressStatus(row),
            }),
            h(
              "span",
              { style: MOBILE_SUB_STYLE },
              t("budget.list.spentOfBudget", {
                spent: formatAmount(row.spent_cents),
                total: formatAmount(row.budget.amount_cents),
              }),
            ),
          ]),
      },
      {
        title: t("budget.list.colActions"),
        key: "actions",
        render: (row) =>
          h(NSpace, { size: 4, wrap: false }, () => [
            h(
              NButton,
              {
                size: "tiny",
                type: "primary",
                quaternary: true,
                style: MOBILE_TOUCH_TARGET_STYLE,
                onClick: () => openEdit(row),
              },
              () => t("budget.actions.edit"),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => remove(row.budget.id) },
              {
                default: () => t("budget.actions.confirmDelete"),
                trigger: () =>
                  h(
                    NButton,
                    {
                      size: "tiny",
                      type: "error",
                      quaternary: true,
                      style: MOBILE_TOUCH_TARGET_STYLE,
                    },
                    () => t("budget.actions.delete"),
                  ),
              },
            ),
          ]),
      },
    ];
  }
  return [
    {
      title: t("budget.list.colCategory"),
      key: "category_name",
      render: (row) => displayCategoryName(row),
    },
    {
      title: t("budget.list.colPeriod"),
      key: "budget.period",
      render: (row) => t(`budget.period.${row.budget.period}`),
    },
    {
      title: t("budget.list.colAmount"),
      key: "budget.amount_cents",
      render: (row) => formatAmount(row.budget.amount_cents),
    },
    {
      title: t("budget.list.colSpent"),
      key: "spent_cents",
      render: (row) => formatAmount(row.spent_cents),
    },
    {
      title: t("budget.list.colProgress"),
      key: "progress",
      render: (row) =>
        h(NProgress, {
          type: "line",
          percentage: progressPercentage(row),
          status: progressStatus(row),
        }),
    },
    {
      title: t("budget.list.colStatus"),
      key: "over_budget",
      width: 80,
      render: (row) =>
        row.over_budget
          ? h(NTag, { type: "error" }, () => t("budget.status.over"))
          : h(NTag, { type: "success" }, () => t("budget.status.normal")),
    },
    {
      title: t("budget.list.colActions"),
      key: "actions",
      width: 130,
      render: (row) =>
        h(NSpace, { size: 4, wrap: false }, () => [
          h(
            NButton,
            { size: "tiny", type: "primary", quaternary: true, onClick: () => openEdit(row) },
            () => t("budget.actions.edit"),
          ),
          h(
            AppPopconfirm,
            { onPositiveClick: () => remove(row.budget.id) },
            {
              default: () => t("budget.actions.confirmDelete"),
              trigger: () =>
                h(NButton, { size: "tiny", type: "error", quaternary: true }, () =>
                  t("budget.actions.delete"),
                ),
            },
          ),
        ]),
    },
  ];
});

onMounted(() => {
  void refresh();
});
</script>

<template>
  <NSpin :show="loading" class="viewport-table-spin">
    <div class="viewport-table-view viewport-gap-16">
      <NCard :title="t('budget.create.title')" size="small">
        <!-- 新增预算表单按窗口分级分档（issue #848）：桌面档行内横排（既有布局一字
             不动）；移动档纵排堆叠（标签上置 + 控件满宽，行内横排窄屏必溢出），
             行距取 ADR-0079 决策 4 的 12px 节奏值（同账户新增表单先例）。
             「添加」主操作热区扩至 ≥48px（视觉不变）。 -->
        <NForm
          :inline="!isMobileTier"
          :label-placement="isMobileTier ? 'top' : 'left'"
          :show-feedback="false"
          size="small"
          :style="
            isMobileTier ? { display: 'flex', flexDirection: 'column', gap: '12px' } : undefined
          "
        >
          <NFormItem :label="t('budget.create.category')">
            <PinyinSelect
              v-model:value="categoryId"
              :options="categoryOptions()"
              :placeholder="t('budget.create.categoryPlaceholder')"
              :style="isMobileTier ? { width: '100%' } : { width: '160px' }"
            />
          </NFormItem>
          <NFormItem :label="t('budget.create.period')">
            <NRadioGroup v-model:value="createPeriod" size="small" name="create-period">
              <NRadioButton v-for="p in BUDGET_PERIODS" :key="p" :value="p">
                {{ t(`budget.period.${p}`) }}
              </NRadioButton>
            </NRadioGroup>
          </NFormItem>
          <NFormItem :label="t('budget.create.amount')">
            <NInputNumber
              v-model:value="amount"
              :precision="2"
              :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
            />
          </NFormItem>
          <NButton
            type="primary"
            :class="isMobileTier ? 'touch-hit-area' : undefined"
            :style="isMobileTier ? { '--touch-hit-inset': '-10px -14px' } : undefined"
            @click="create"
            >{{ t("budget.create.add") }}</NButton
          >
        </NForm>
      </NCard>

      <!-- 取数失败（summary 仍为 null）时不渲染空卡：错误已由 Loadable 的默认 toast 呈现 -->
      <NCard
        v-if="summary !== null"
        :title="t('budget.summary.title')"
        size="small"
        data-testid="budget-annual-summary"
      >
        <template #header-extra>
          <NTag v-if="summary" :type="summary.over_budget ? 'error' : 'success'" size="small">
            {{ summary.over_budget ? t("budget.summary.over") : t("budget.summary.normal") }}
          </NTag>
        </template>
        <template v-if="summary">
          <div :style="summaryStatsStyle">
            <NSpace vertical :size="0">
              <NText depth="2" :style="SUMMARY_STAT_LABEL_STYLE">
                {{ t("budget.summary.annualTotal") }}
              </NText>
              <NText strong :style="summaryPrimaryStyle">
                {{ formatAmount(summary.annual_budget_total_cents) }}
              </NText>
            </NSpace>
            <NSpace vertical :size="0">
              <NText depth="2" :style="SUMMARY_STAT_LABEL_STYLE">
                {{ t("budget.summary.spent") }}
              </NText>
              <NText strong :style="summarySecondaryStyle">
                {{ formatAmount(summary.spent_cents) }}
              </NText>
            </NSpace>
          </div>
          <NProgress
            type="line"
            :percentage="summaryPercentage"
            :status="summary.over_budget ? 'error' : 'success'"
            :show-indicator="false"
            style="margin-top: 16px"
          />
          <NText depth="3" style="font-size: 12px; display: block; margin-top: 8px">
            {{ t("budget.summary.note") }}
          </NText>
        </template>
      </NCard>

      <NCard :title="t('budget.list.title')" size="small" class="viewport-table-card">
        <NDataTable
          :columns="columns"
          :data="list"
          :bordered="false"
          size="small"
          :flex-height="!isMobileTier"
        />
      </NCard>
    </div>

    <!-- 编辑弹窗（issue #184 金额 / issue #1946 放开周期）：金额与周期可改，
         分类只读。显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂
         清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <AppModal
      :key="editSeq"
      :show="editIntent !== null"
      :title="t('budget.edit.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('budget.edit.category')">
            <NText>{{ editingCategoryName }}</NText>
          </NFormItem>
          <NFormItem :label="t('budget.edit.period')">
            <NRadioGroup v-model:value="editPeriod" size="small" name="edit-period">
              <NRadioButton v-for="p in BUDGET_PERIODS" :key="p" :value="p">
                {{ t(`budget.period.${p}`) }}
              </NRadioButton>
            </NRadioGroup>
          </NFormItem>
          <NFormItem :label="t('budget.edit.amount')">
            <NInputNumber v-model:value="editAmount" :precision="2" style="width: 100%" />
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeEdit">{{ t("budget.edit.cancel") }}</NButton>
            <NButton type="primary" @click="saveEdit">{{ t("budget.edit.save") }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>
  </NSpin>
</template>
