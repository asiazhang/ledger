<script setup lang="ts">
import { computed, h, ref, nextTick, watch } from "vue";
import { t } from "@ledger/i18n";
import { NButton, NDataTable, NSpace, NTag, useMessage, type DataTableColumns } from "naive-ui";
import { formatAmount } from "@ledger/money";
import { errorMessage } from "@ledger/utils/errors";
import { scheduledStatusLabel } from "@ledger/utils/scheduled";
import { scheduledRecurrenceLabel } from "@ledger/scheduled-plan-list";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import type { Policy, ScheduledTransactionWithExt } from "@ledger/types";
import PolicyAgreementFields from "@/policy/PolicyAgreementFields.vue";

const props = defineProps<{ policy: Policy }>();

const message = useMessage();
const reference = useReferenceStore();
const fieldsRef = ref<InstanceType<typeof PolicyAgreementFields> | null>(null);

const segments = ref<ScheduledTransactionWithExt[]>([]);
const loading = ref(false);

const activeSegment = computed(
  () => segments.value.find((s) => s.core.status === "active") ?? null,
);

async function load() {
  loading.value = true;
  try {
    const plans = await api.listScheduledTransactions();
    segments.value = plans
      .filter((p) => p.policy_id === props.policy.id)
      .sort(
        (a, b) =>
          a.core.created_at.localeCompare(b.core.created_at) || a.core.id.localeCompare(b.core.id),
      );
  } catch (e) {
    message.error(t("policies.agreement.msg.loadFailed", { msg: errorMessage(e) }));
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.policy,
  () => void load(),
  { immediate: true },
);

type FormMode = "add" | "rebuild";
const mode = ref<FormMode | null>(null);
const rebuildFrom = ref<ScheduledTransactionWithExt | null>(null);

async function openAdd() {
  mode.value = "add";
  rebuildFrom.value = null;
  await nextTick();
  fieldsRef.value?.reset({ startDate: props.policy.start_date });
}

async function nextChargeDate(planId: string, fallback: string): Promise<string> {
  try {
    const detail = await api.getScheduledTransactionDetail(planId);
    const pending = detail.occurrences
      .filter((o) => o.status === "pending")
      .map((o) => o.scheduled_date)
      .sort();
    return pending[0] ?? fallback;
  } catch {
    return fallback;
  }
}

async function openRebuild() {
  const old = activeSegment.value;
  if (!old) return;
  mode.value = "rebuild";
  rebuildFrom.value = old;
  await nextTick();
  fieldsRef.value?.reset({
    currencyCode: old.core.currency_code,
    recurrenceType: old.core.recurrence_type,
    recurrenceInterval: old.core.recurrence_interval,
    accountId: old.core.account_id,
    startDate: await nextChargeDate(old.core.id, old.core.start_date),
  });
}

function closeForm() {
  mode.value = null;
  rebuildFrom.value = null;
}

async function submit() {
  const fields = fieldsRef.value;
  if (!fields) return;
  const err = fields.validate();
  if (err) {
    message.warning(err);
    return;
  }
  const input = fields.build(props.policy.id, props.policy.product_name);
  try {
    if (mode.value === "rebuild" && rebuildFrom.value) {
      const oldId = rebuildFrom.value.core.id;
      await api.updateScheduledTransactionStatus({ id: oldId, new_status: "cancelled" });
      await api.createScheduledTransaction(input);
      message.success(t("policies.agreement.msg.rebuilt"));
    } else {
      await api.createScheduledTransaction(input);
      message.success(t("policies.agreement.msg.created"));
    }
    closeForm();
    await load();
  } catch (e) {
    message.error(t("policies.agreement.msg.failed", { msg: errorMessage(e) }));
  }
}

const segmentColumns = computed<DataTableColumns<ScheduledTransactionWithExt>>(() => [
  {
    title: t("policies.agreement.column.amount"),
    key: "amount",
    render: (s) => formatAmount(s.core.amount_cents, reference.getCurrency(s.core.currency_code)),
  },
  {
    title: t("policies.agreement.column.recurrence"),
    key: "recurrence",
    render: (s) => scheduledRecurrenceLabel(s.core.recurrence_type, s.core.recurrence_interval),
  },
  {
    title: t("policies.agreement.column.startDate"),
    key: "start_date",
    render: (s) => s.core.start_date,
  },
  {
    title: t("policies.agreement.column.status"),
    key: "status",
    render: (s) =>
      s.core.status === "active"
        ? scheduledStatusLabel(s.core.status)
        : h(
            NTag,
            {
              size: "small",
              bordered: false,
              type: s.core.status === "cancelled" ? "default" : "warning",
            },
            () => scheduledStatusLabel(s.core.status),
          ),
  },
]);
</script>

<template>
  <NSpace vertical :size="12">
    <NDataTable
      :columns="segmentColumns"
      :data="segments"
      :loading="loading"
      :bordered="false"
      size="small"
      :row-key="(s: ScheduledTransactionWithExt) => s.core.id"
      data-testid="policy-agreement-segments"
    />

    <!-- 操作行：无活跃段 → 添加；有活跃段 → 改价（沿用订阅价变语义） -->
    <NSpace v-if="mode === null" :size="8">
      <NButton
        v-if="!activeSegment"
        size="small"
        data-testid="policy-agreement-add"
        @click="openAdd"
      >
        {{ t("policies.agreement.add") }}
      </NButton>
      <NButton v-else size="small" data-testid="policy-agreement-rebuild-open" @click="openRebuild">
        {{ t("policies.agreement.rebuild") }}
      </NButton>
    </NSpace>

    <!-- 添加/改价表单（同一字段组实例，reset 预填切换） -->
    <NSpace v-else vertical :size="12" data-testid="policy-agreement-form">
      <div v-if="mode === 'rebuild'" style="opacity: 0.7; font-size: 12px">
        {{ t("policies.agreement.rebuildHint") }}
      </div>
      <PolicyAgreementFields ref="fieldsRef" />
      <NSpace justify="end">
        <NButton size="small" data-testid="policy-agreement-form-cancel" @click="closeForm">
          {{ t("policies.agreement.cancel") }}
        </NButton>
        <NButton type="primary" size="small" data-testid="policy-agreement-submit" @click="submit">
          {{
            mode === "rebuild"
              ? t("policies.agreement.rebuildConfirm")
              : t("policies.agreement.create")
          }}
        </NButton>
      </NSpace>
    </NSpace>
  </NSpace>
</template>
