<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { t } from "@ledger/i18n";
import {
  NCard,
  NButton,
  NButtonGroup,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSpace,
  NProgress,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import AppTreeSelect from "@ledger/ui-kit/AppTreeSelect.vue";
import { formatAmount, yuanToCents } from "@ledger/money";
import { installmentSchedule } from "@ledger/utils/installment";
import { useReferenceStore } from "@/stores/reference";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { useScheduledPlanForm } from "@/scheduled/useScheduledPlanForm";
import {
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from "@ledger/scheduled-plan-list";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import PlanRowActions from "@/scheduled/PlanRowActions.vue";
import PlanDetailModal from "@/scheduled/PlanDetailModal.vue";
import { MOBILE_CELL_STYLE, MOBILE_SUB_STYLE, mobileSubLine } from "@/components/mobile-cells";
import { usePlanFocusLanding } from "@/scheduled/usePlanFocusLanding";
import { scheduledStatusLabel } from "@ledger/utils/scheduled";

const reference = useReferenceStore();
const message = useMessage();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

interface InstallmentExt {
  completedCount: number;
  completedAmountCents: number;
}
type InstallmentRow = ScheduledPlanRow<InstallmentExt>;

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null);

const list = useScheduledPlanList<InstallmentExt>({
  kind: "installment",
  expandDetail: (_plan, detail) => ({
    completedCount: detail?.completed_occurrences ?? 0,
    completedAmountCents: detail?.completed_amount_cents ?? 0,
  }),
  cancelConfirmText: () => t("scheduled.pane.installmentCancelConfirm"),
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
});
const { loading, statusFilter, statusFilterOptions, filteredRows } = list;

async function onDetailChanged() {
  await list.load();
}

interface InstallmentCreateIntent {
  type: "create";
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<InstallmentCreateIntent>();

const totalYuan = ref("");
const periods = ref<number | null>(null);

const form = useScheduledPlanForm({
  onSubmitted: () => {
    closeCreateIntent();
    totalYuan.value = "";
    periods.value = null;
    void list.load();
  },
});
const {
  note,
  accountId,
  categoryId,
  merchantRef,
  currencyCode,
  recurrenceType,
  recurrenceInterval,
  startDate,
  accountOptions,
  currencyOptions,
  categoryTreeOptions,
  merchantOptions,
} = form;

const recurrenceOptions = computed(scheduledRecurrenceOptions);

const schedule = computed(() => {
  const totalCents = yuanToCents(totalYuan.value);
  if (totalCents === null || periods.value === null || periods.value < 1) return null;
  try {
    return installmentSchedule(totalCents, periods.value);
  } catch {
    return null;
  }
});

const previewText = computed(() => {
  const s = schedule.value;
  if (!s) return "";
  const currency = reference.getCurrency(currencyCode.value);
  const per = formatAmount(s.perPeriodCents, currency);
  if (s.lastPeriodCents === s.perPeriodCents)
    return t("scheduled.preview.perPeriod", { amount: per });
  return t("scheduled.preview.withLast", {
    per,
    last: formatAmount(s.lastPeriodCents, currency),
  });
});

async function create() {
  if (!accountId.value) {
    message.warning(t("scheduled.form.selectAccount"));
    return;
  }
  const totalCents = yuanToCents(totalYuan.value);
  if (totalCents === null || totalCents <= 0) {
    message.warning(t("scheduled.form.totalPositive"));
    return;
  }
  const totalOccurrences = periods.value;
  if (totalOccurrences === null || totalOccurrences < 1) {
    message.warning(t("scheduled.form.periodsMin"));
    return;
  }
  if (totalCents < totalOccurrences) {
    message.warning(t("scheduled.form.totalBelowPeriods"));
    return;
  }
  const s = installmentSchedule(totalCents, totalOccurrences);
  await form.submitCreate({
    kind: "installment",
    amountCents: s.perPeriodCents,
    specific: { total_amount_cents: totalCents, total_occurrences: totalOccurrences },
  });
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status);
}

function merchantName(row: InstallmentRow): string {
  const m = row.plan.merchant_id ? reference.merchantMap.get(row.plan.merchant_id) : undefined;
  return m?.name ?? "—";
}

function accountName(row: InstallmentRow): string {
  return reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id;
}

function progressText(row: InstallmentRow): string {
  const currency = reference.getCurrency(row.plan.core.currency_code);
  return row.detailFailed
    ? t("scheduled.list.loadFailed")
    : t("scheduled.progress.repaid", {
        paid: formatAmount(row.ext.completedAmountCents, currency),
        total: formatAmount(row.plan.total_amount_cents ?? 0, currency),
        count: row.ext.completedCount,
        occurrences: row.plan.total_occurrences ?? 0,
      });
}

function progressPercentage(row: InstallmentRow): number {
  const total = row.plan.total_occurrences ?? 0;
  if (total <= 0) return 0;
  return Math.min(100, (row.ext.completedCount / total) * 100);
}

const columns = computed<DataTableColumns<InstallmentRow>>(() => {
  if (isMobileTier.value) {
    return [
      {
        title: t("scheduled.column.note"),
        key: "note",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h("span", null, row.plan.core.note ?? "—"),
            h(
              "span",
              { style: MOBILE_SUB_STYLE },
              mobileSubLine(
                statusLabel(row.plan.core.status),
                scheduledRecurrenceLabel(
                  row.plan.core.recurrence_type,
                  row.plan.core.recurrence_interval,
                ),
                row.plan.core.start_date,
              ),
            ),
            h(
              "span",
              { style: MOBILE_SUB_STYLE },
              mobileSubLine(
                merchantName(row),
                reference.categoryPath(row.plan.core.category_id),
                accountName(row),
              ),
            ),
          ]),
      },
      {
        title: t("scheduled.column.totalAmount"),
        key: "total",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h(
              "span",
              null,
              formatAmount(
                row.plan.total_amount_cents ?? 0,
                reference.getCurrency(row.plan.core.currency_code),
              ),
            ),
            h(
              "div",
              { style: MOBILE_SUB_STYLE, "data-testid": `inst-progress-${row.plan.core.id}` },
              [
                row.detailFailed
                  ? null
                  : h(NProgress, {
                      type: "line",
                      percentage: progressPercentage(row),
                      showIndicator: false,
                    }),
                h("span", progressText(row)),
              ],
            ),
          ]),
      },
      {
        title: t("scheduled.column.actions"),
        key: "actions",
        render: (row) =>
          h(PlanRowActions, {
            actions: list.rowActions(row),
            rowId: row.plan.core.id,
            mobile: true,
          }),
      },
    ];
  }
  return [
    {
      title: t("scheduled.column.note"),
      key: "note",
      render: (row) => row.plan.core.note ?? "—",
    },
    {
      title: t("scheduled.column.merchant"),
      key: "merchant",
      render: (row) => merchantName(row),
    },
    {
      title: t("scheduled.column.category"),
      key: "category",
      render: (row) => reference.categoryPath(row.plan.core.category_id) || "—",
    },
    {
      title: t("scheduled.column.account"),
      key: "account",
      render: (row) => accountName(row),
    },
    {
      title: t("scheduled.column.totalAmount"),
      key: "total",
      render: (row) =>
        formatAmount(
          row.plan.total_amount_cents ?? 0,
          reference.getCurrency(row.plan.core.currency_code),
        ),
    },
    {
      title: t("scheduled.column.progress"),
      key: "progress",
      render: (row) =>
        h("div", { "data-testid": `inst-progress-${row.plan.core.id}` }, [
          row.detailFailed
            ? null
            : h(NProgress, {
                type: "line",
                percentage: progressPercentage(row),
                showIndicator: false,
                style: "max-width: 160px",
              }),
          h("span", progressText(row)),
        ]),
    },

    {
      title: t("scheduled.column.recurrence"),
      key: "recurrence",
      render: (row) =>
        scheduledRecurrenceLabel(row.plan.core.recurrence_type, row.plan.core.recurrence_interval),
    },
    {
      title: t("scheduled.column.startDate"),
      key: "start_date",
      render: (row) => row.plan.core.start_date,
    },
    {
      title: t("scheduled.column.status"),
      key: "status",
      render: (row) => statusLabel(row.plan.core.status),
    },
    {
      title: t("scheduled.column.actions"),
      key: "actions",
      render: (row) =>
        h(PlanRowActions, {
          actions: list.rowActions(row),
          rowId: row.plan.core.id,
        }),
    },
  ];
});

const props = defineProps<{ focusPlanId?: string | null }>();

const emit = defineEmits<{ (e: "focusConsumed"): void }>();

usePlanFocusLanding({
  focusPlanId: () => props.focusPlanId,
  openDetail: (id) => void planDetailRef.value?.open(id),
  onConsumed: () => emit("focusConsumed"),
});

onMounted(() => {
  void list.load();
});
</script>

<template>
  <div class="viewport-table-view viewport-gap-16">
    <NCard :title="t('scheduled.pane.installmentList')" size="small" class="viewport-table-card">
      <template #header-extra>
        <NSpace :size="12">
          <NButtonGroup size="small">
            <NButton
              v-for="f in statusFilterOptions"
              :key="f.key"
              :type="statusFilter === f.key ? 'primary' : 'default'"
              :data-testid="`filter-${f.key}`"
              @click="list.setStatusFilter(f.key)"
            >
              {{ f.label }}
            </NButton>
          </NButtonGroup>
          <NButton
            type="primary"
            size="small"
            data-testid="inst-create-open"
            @click="openCreateIntent({ type: 'create' })"
          >
            {{ t("scheduled.pane.createInstallment") }}
          </NButton>
        </NSpace>
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :flex-height="!isMobileTier"
        :row-key="(row: InstallmentRow) => row.plan.core.id"
      />
    </NCard>

    <!-- 新建分期弹窗：总金额 + 期数实时预览；其余字段与订阅表单同款；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createInstallment')"
      preset="card"
      display-directive="if"
      card-size="md"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.totalAmount')">
            <NInput
              v-model:value="totalYuan"
              data-testid="inst-total"
              :placeholder="t('scheduled.form.totalAmountPlaceholder')"
              style="width: 160px"
            />
            <AppSelect
              :value="currencyCode"
              :disabled="true"
              :options="currencyOptions"
              style="width: 130px; margin-left: 8px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.periods')">
            <NInputNumber
              v-model:value="periods"
              data-testid="inst-periods"
              :min="1"
              :precision="0"
              :placeholder="t('scheduled.form.periodsPlaceholder')"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.perPeriod')">
            <span data-testid="inst-preview">{{ previewText }}</span>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="inst-note"
              :placeholder="t('scheduled.form.installmentNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
              data-testid="inst-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.category')">
            <AppTreeSelect
              v-model:value="categoryId"
              :options="categoryTreeOptions"
              :placeholder="t('scheduled.form.categoryPlaceholder')"
              filterable
              clearable
              :consistent-menu-width="false"
              style="width: 220px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.merchant')">
            <PinyinSelect
              v-model:value="merchantRef"
              :options="merchantOptions"
              tag
              clearable
              :placeholder="t('scheduled.form.merchantPlaceholder')"
              style="width: 220px"
              data-testid="inst-merchant"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.recurrence')">
            <NSpace :size="8" align="center" :wrap="false">
              <span>{{ t("scheduled.form.every") }}</span>
              <NInputNumber
                v-model:value="recurrenceInterval"
                :min="1"
                :precision="0"
                style="width: 90px"
              />
              <AppSelect
                v-model:value="recurrenceType"
                :options="recurrenceOptions"
                style="width: 100px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.startDate')">
            <AppDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 200px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="inst-create-cancel" @click="closeCreateIntent">{{
              t("scheduled.form.cancel")
            }}</NButton>
            <NButton type="primary" data-testid="inst-create" @click="create">{{
              t("scheduled.pane.createInstallmentSubmit")
            }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </div>
</template>
