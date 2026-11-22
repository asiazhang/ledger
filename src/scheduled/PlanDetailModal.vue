<script setup lang="ts">
import { computed, h, ref } from "vue";
import {
  NButton,
  NDataTable,
  NEmpty,
  NSpace,
  NSpin,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import { formatAmount } from "@ledger/money";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { occurrenceStatusLabel } from "@ledger/utils/scheduled";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import type { ScheduledTransactionDetail, ScheduledTransactionOccurrence } from "@ledger/types";

const reference = useReferenceStore();
const message = useMessage();

const emit = defineEmits<{
  (e: "changed"): void;
}>();

const show = ref(false);
const planId = ref<string | null>(null);
const loading = ref(false);
const loadFailed = ref(false);
const detail = ref<ScheduledTransactionDetail | null>(null);
const retryingId = ref<string | null>(null);
const expanding = ref(false);

async function open(id: string) {
  planId.value = id;
  show.value = true;
  await load();
}

async function load() {
  if (!planId.value) return;
  loading.value = true;
  loadFailed.value = false;
  try {
    detail.value = await api.getScheduledTransactionDetail(planId.value);
  } catch {
    detail.value = null;
    loadFailed.value = true;
  } finally {
    loading.value = false;
  }
}

const occurrenceRows = computed<ScheduledTransactionOccurrence[]>(() => {
  const d = detail.value;
  if (!d) return [];
  return [...d.occurrences].sort(
    (a, b) =>
      a.scheduled_date.localeCompare(b.scheduled_date) || a.created_at.localeCompare(b.created_at),
  );
});

const totalOccurrences = computed<number | null>(() => {
  const ext = detail.value?.extension;
  if (!ext) return null;
  return "total_occurrences" in ext ? (ext.total_occurrences ?? null) : null;
});

const canExpand = computed(() => {
  const d = detail.value;
  if (!d) return false;
  if (d.core.status !== "active") return false;
  if (totalOccurrences.value === null) return true;
  return occurrenceRows.value.length < totalOccurrences.value;
});

function canRetry(occ: ScheduledTransactionOccurrence): boolean {
  return occ.status === "failed";
}

async function retry(occ: ScheduledTransactionOccurrence) {
  if (!canRetry(occ) || retryingId.value) return;
  retryingId.value = occ.id;
  try {
    await api.executeScheduledOccurrence({ occurrence_id: occ.id });
    message.success(t("scheduled.detail.retrySuccess"));
    await load();
    emit("changed");
  } catch (e) {
    message.error(t("scheduled.detail.retryFailed", { message: errorMessage(e) }));
  } finally {
    retryingId.value = null;
  }
}

async function expand() {
  const d = detail.value;
  if (!d || !canExpand.value || expanding.value) return;
  expanding.value = true;
  try {
    const ids = await api.expandScheduledOccurrences(d.core.id);
    await load();
    message.success(
      ids.length > 0
        ? t("scheduled.detail.expandedCount", { n: ids.length })
        : t("scheduled.detail.noMore"),
    );
  } catch (e) {
    message.error(t("scheduled.detail.expandFailed", { message: errorMessage(e) }));
  } finally {
    expanding.value = false;
  }
}

const columns = computed<DataTableColumns<ScheduledTransactionOccurrence>>(() => [
  {
    title: t("scheduled.column.date"),
    key: "scheduled_date",
    render: (occ) => h("span", { "data-testid": `occ-date-${occ.id}` }, occ.scheduled_date),
  },
  {
    title: t("scheduled.column.amount"),
    key: "amount",
    render: (occ) => {
      const currency = reference.getCurrency(detail.value!.core.currency_code);
      return formatAmount(occ.amount_cents, currency);
    },
  },
  {
    title: t("scheduled.column.status"),
    key: "status",
    render: (occ) =>
      h(
        "span",
        { "data-testid": `occ-status-${occ.id}` },
        occurrenceStatusLabel(occ.status as ScheduledTransactionOccurrence["status"]),
      ),
  },
  {
    title: t("scheduled.column.actions"),
    key: "actions",
    render: (occ) =>
      canRetry(occ)
        ? h(
            NButton,
            {
              size: "tiny",
              type: "primary",
              quaternary: true,
              loading: retryingId.value === occ.id,
              "data-testid": `occ-retry-${occ.id}`,
              onClick: () => retry(occ),
            },
            () => t("scheduled.detail.retry"),
          )
        : "—",
  },
]);

defineExpose({ open });
</script>

<template>
  <AppModal
    v-model:show="show"
    :title="t('scheduled.detail.title')"
    preset="card"
    display-directive="if"
    card-size="lg"
  >
    <NSpin :show="loading">
      <div v-if="loadFailed" data-testid="occ-load-failed">
        {{ t("scheduled.detail.loadFailed") }}
      </div>
      <template v-else-if="detail">
        <NSpace vertical :size="8" class="plan-summary">
          <span data-testid="occ-plan-note">{{
            detail.core.note ?? t("scheduled.detail.noNote")
          }}</span>
          <span v-if="totalOccurrences !== null" class="plan-total">
            {{ t("scheduled.detail.totalOccurrences", { n: totalOccurrences }) }}
          </span>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="occurrenceRows"
          :bordered="false"
          size="small"
          :row-key="(occ: ScheduledTransactionOccurrence) => occ.id"
          :max-height="400"
        />
        <NEmpty
          v-if="occurrenceRows.length === 0"
          :description="t('scheduled.detail.empty')"
          data-testid="occ-empty"
        />
        <NSpace v-if="canExpand" justify="center" :size="8" style="margin-top: 12px">
          <NButton size="small" :loading="expanding" data-testid="occ-expand" @click="expand">
            {{ t("scheduled.detail.expandMore") }}
          </NButton>
        </NSpace>
      </template>
    </NSpin>
  </AppModal>
</template>

<style scoped>
.plan-summary {
  margin-bottom: 8px;
  font-size: 13px;
  color: var(--n-text-color-2, #666);
}
</style>
