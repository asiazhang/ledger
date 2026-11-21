<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from "vue";
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
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import PlanRowActions from "@/scheduled/PlanRowActions.vue";
import { formatAmount, yuanToCents } from "@ledger/money";
import type { ScheduledTransactionOccurrence } from "@ledger/types";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from "@ledger/scheduled-plan-list";
import { useModalIntent } from "@ledger/modal-intent";
import { useScheduledPlanForm } from "@/scheduled/useScheduledPlanForm";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import PlanDetailModal from "@/scheduled/PlanDetailModal.vue";
import { MOBILE_CELL_STYLE, MOBILE_SUB_STYLE, mobileSubLine } from "@/components/mobile-cells";
import { usePlanFocusLanding } from "@/scheduled/usePlanFocusLanding";
import { scheduledStatusLabel } from "@ledger/utils/scheduled";

const reference = useReferenceStore();
const message = useMessage();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

interface TransferExt {
  next: ScheduledTransactionOccurrence | null;
}
type TransferRow = ScheduledPlanRow<TransferExt>;

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null);

const list = useScheduledPlanList<TransferExt>({
  kind: "scheduled_transfer",
  expandDetail: (_plan, detail) => ({
    next: detail ? earliestPendingOccurrence(detail) : null,
  }),
  cancelConfirmText: () => t("scheduled.pane.transferCancelConfirm"),
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
});
const { loading, statusFilter, statusFilterOptions, filteredRows } = list;

async function onDetailChanged() {
  await list.load();
}

interface TransferCreateIntent {
  type: "create";
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<TransferCreateIntent>();

const toAccountId = ref<string | null>(null);
const amountYuan = ref("");
const totalOccurrences = ref<number | null>(null);

const form = useScheduledPlanForm({
  onSubmitted: () => {
    closeCreateIntent();
    toAccountId.value = null;
    amountYuan.value = "";
    totalOccurrences.value = null;
    void list.load();
  },
});
const {
  note,
  accountId: fromAccountId,
  currencyCode,
  recurrenceType,
  recurrenceInterval,
  startDate,
  accountOptions,
  currencyOptions,
} = form;

const toAccountOptions = computed(() => {
  const from = fromAccountId.value ? reference.accountMap.get(fromAccountId.value) : undefined;
  if (!from) return accountOptions.value;
  return accountOptions.value.filter((o) => {
    const acc = reference.accountMap.get(o.value);
    return acc && o.value !== from.id && acc.currency_code === from.currency_code;
  });
});

watch(fromAccountId, (id) => {
  const from = id ? reference.accountMap.get(id) : undefined;
  if (
    from &&
    toAccountId.value &&
    reference.accountMap.get(toAccountId.value)?.currency_code !== from.currency_code
  ) {
    toAccountId.value = null;
  }
});

async function create() {
  if (!fromAccountId.value) {
    message.warning(t("scheduled.form.selectFromAccount"));
    return;
  }
  if (!toAccountId.value) {
    message.warning(t("scheduled.form.selectToAccount"));
    return;
  }
  const amountCents = yuanToCents(amountYuan.value);
  if (amountCents === null || amountCents <= 0) {
    message.warning(t("scheduled.form.amountPositive"));
    return;
  }
  await form.submitCreate({
    kind: "scheduled_transfer",
    amountCents,
    specific: { to_account_id: toAccountId.value, total_occurrences: totalOccurrences.value },
  });
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status);
}

function fromAccountName(row: TransferRow): string {
  return reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id;
}

function toAccountName(row: TransferRow): string {
  const id = row.plan.to_account_id;
  return id ? (reference.accountMap.get(id)?.name ?? id) : "—";
}

function amountText(row: TransferRow): string {
  return `${formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code))}${row.plan.total_occurrences != null ? ` ${t("scheduled.column.occurrencesSuffix", { n: row.plan.total_occurrences })}` : ""}`;
}

function nextTransferText(row: TransferRow): string {
  const currency = reference.getCurrency(row.plan.core.currency_code);
  if (row.ext.next) {
    return `${row.ext.next.scheduled_date} · ${formatAmount(row.ext.next.amount_cents, currency)}`;
  }
  return row.detailFailed ? t("scheduled.list.loadFailed") : "—";
}

const recurrenceOptions = computed(scheduledRecurrenceOptions);

const columns = computed<DataTableColumns<TransferRow>>(() => {
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
              mobileSubLine(fromAccountName(row), "→", toAccountName(row)),
            ),
          ]),
      },
      {
        title: t("scheduled.column.amount"),
        key: "amount",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h("span", null, amountText(row)),
            h(
              "span",
              { style: MOBILE_SUB_STYLE, "data-testid": `next-transfer-${row.plan.core.id}` },
              nextTransferText(row),
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
      title: t("scheduled.column.fromAccount"),
      key: "from",
      render: (row) => fromAccountName(row),
    },
    {
      title: t("scheduled.column.toAccount"),
      key: "to",
      render: (row) => toAccountName(row),
    },
    {
      title: t("scheduled.column.amount"),
      key: "amount",
      render: (row) => amountText(row),
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
      title: t("scheduled.column.nextTransfer"),
      key: "next",
      render: (row) =>
        h("span", { "data-testid": `next-transfer-${row.plan.core.id}` }, nextTransferText(row)),
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
  <NSpace vertical :size="16">
    <NCard :title="t('scheduled.pane.transferList')" size="small">
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
            data-testid="transfer-create-open"
            @click="openCreateIntent({ type: 'create' })"
          >
            {{ t("scheduled.pane.createTransfer") }}
          </NButton>
        </NSpace>
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :row-key="(row: TransferRow) => row.plan.core.id"
      />
    </NCard>

    <!-- 新建定时转账弹窗：转入候选按转出账户币种过滤，无商户字段（issue #203）；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createTransferModal')"
      preset="card"
      display-directive="if"
      card-size="md"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="transfer-note"
              :placeholder="t('scheduled.form.transferNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.fromAccount')">
            <PinyinSelect
              v-model:value="fromAccountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.fromAccountPlaceholder')"
              style="width: 200px"
              data-testid="transfer-from-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.toAccount')">
            <PinyinSelect
              v-model:value="toAccountId"
              :options="toAccountOptions"
              :placeholder="t('scheduled.form.toAccountPlaceholder')"
              style="width: 200px"
              data-testid="transfer-to-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.amount')">
            <NInput
              v-model:value="amountYuan"
              data-testid="transfer-amount"
              :placeholder="t('scheduled.form.amountPerPeriodPlaceholder')"
              style="width: 160px"
            />
            <!-- 币种随转出账户推导并锁定（issue #1770 / ADR-0134 决策 5） -->
            <AppSelect
              :value="currencyCode"
              :options="currencyOptions"
              :disabled="true"
              data-testid="transfer-currency"
              style="width: 130px; margin-left: 8px"
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
                data-testid="transfer-recurrence"
                style="width: 90px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.totalOccurrences')">
            <NInputNumber
              v-model:value="totalOccurrences"
              :min="1"
              :precision="0"
              :placeholder="t('scheduled.form.totalOccurrencesPlaceholder')"
              data-testid="transfer-total-occurrences"
              style="width: 200px"
            />
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
            <NButton data-testid="transfer-create-cancel" @click="closeCreateIntent">
              {{ t("scheduled.form.cancel") }}
            </NButton>
            <NButton type="primary" data-testid="transfer-create" @click="create">
              {{ t("scheduled.pane.createTransferSubmit") }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
