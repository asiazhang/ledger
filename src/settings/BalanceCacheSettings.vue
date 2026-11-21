<script setup lang="ts">
import { computed, ref } from "vue";
import { NAlert, NButton, NCard, NDataTable, NSpace, NText, useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import { useLoadable } from "@ledger/loadable";
import type { BalanceCacheAudit } from "@ledger/types";

const message = useMessage();
const reference = useReferenceStore();

const report = ref<BalanceCacheAudit | null>(null);

function currencyOf(accountId: string) {
  const code = reference.accountMap.get(accountId)?.currency_code;
  return code ? reference.getCurrency(code) : undefined;
}

function cachedText(cents: number | null) {
  return cents === null ? t("settings.data.balanceCache.report.missing") : formatAmount(cents);
}

const columns = computed(() => [
  { title: () => t("settings.data.balanceCache.columns.account"), key: "account_name" },
  {
    title: () => t("settings.data.balanceCache.columns.cached"),
    key: "cached_cents",
    render: (row: BalanceCacheAudit["drifts"][number]) => cachedText(row.cached_cents),
  },
  {
    title: () => t("settings.data.balanceCache.columns.actual"),
    key: "actual_cents",
    render: (row: BalanceCacheAudit["drifts"][number]) =>
      formatAmount(row.actual_cents, currencyOf(row.account_id)),
  },
]);

const reportView = computed(() => {
  const r = report.value;
  if (!r) return null;
  return r.drifts.length > 0
    ? {
        type: "success" as const,
        title: t("settings.data.balanceCache.report.doneTitle", { n: r.drifts.length }),
        body: t("settings.data.balanceCache.report.doneBody"),
      }
    : {
        type: "success" as const,
        title: t("settings.data.balanceCache.report.noopTitle"),
        body: t("settings.data.balanceCache.report.noopBody", { n: r.accounts_checked }),
      };
});

const repairAction = useLoadable(() => api.auditBalanceCache());
const repairing = repairAction.loading;

async function repair() {
  const r = await repairAction.run();
  if (r !== null) {
    report.value = r;
    message.success(reportView.value!.title);
  }
}
</script>

<template>
  <NCard :title="t('settings.data.balanceCache.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t("settings.data.balanceCache.hint") }}
      </NText>

      <NAlert v-if="reportView" :type="reportView.type" :show-icon="true" :title="reportView.title">
        {{ reportView.body }}
      </NAlert>

      <NDataTable
        v-if="report && report.drifts.length > 0"
        :columns="columns"
        :data="report.drifts"
        :row-key="(row: BalanceCacheAudit['drifts'][number]) => row.account_id"
        :pagination="false"
        size="small"
        data-testid="balance-cache-drifts"
      />

      <NSpace align="center" :size="12">
        <NButton size="small" type="primary" :loading="repairing" @click="repair">
          {{ t("settings.data.balanceCache.repair") }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
