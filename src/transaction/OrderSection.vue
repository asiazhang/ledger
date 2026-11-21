<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { NSpin } from "naive-ui";
import { formatAmount } from "@ledger/money";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { displayAmountText } from "@/transaction/transaction-columns";
import type { TransactionOrderSummary } from "@ledger/types";
import {
  ORDER_SECTION_CLASS,
  ORDER_SECTION_TITLE_CLASS,
  ORDER_NO_CLASS,
  ORDER_METRIC_CLASS,
  ORDER_METRIC_LABEL_CLASS,
  ORDER_METRIC_VALUE_CLASS,
  ORDER_ACCOUNTS_CLASS,
  ORDER_ACCOUNT_TAG_CLASS,
  ORDER_ITEMS_CLASS,
  ORDER_ITEM_ROW_CLASS,
  ORDER_ITEM_ACCOUNT_CLASS,
  ORDER_ITEM_MUTED_CLASS,
  ORDER_ERROR_CLASS,
} from "./order-section.css.ts";

const props = defineProps<{
  sourceOrderNo: string;
}>();

const reference = useReferenceStore();
const app = useAppStore();
const summary = ref<TransactionOrderSummary | null>(null);
const loadError = ref<string | null>(null);
const loading = ref(true);

onMounted(async () => {
  try {
    summary.value = await api.getTransactionOrderSummary(props.sourceOrderNo);
  } catch (e) {
    loadError.value = errorMessage(e);
  } finally {
    loading.value = false;
  }
});

function accountName(accountId: string): string {
  return reference.accountMap.get(accountId)?.name ?? "—";
}

const summaryCurrency = computed(() =>
  summary.value?.currency_code
    ? reference.getCurrency(summary.value.currency_code)
    : reference.getCurrency(app.defaultCurrency),
);

const totalText = computed(() =>
  summary.value ? formatAmount(summary.value.total_amount_cents, summaryCurrency.value) : "",
);

function contributionText(amountCents: number): string {
  return formatAmount(amountCents, summaryCurrency.value);
}

function itemAmount(row: TransactionOrderSummary["items"][number]): string {
  return displayAmountText(reference, row);
}
</script>

<template>
  <div :class="ORDER_SECTION_CLASS">
    <div :class="ORDER_SECTION_TITLE_CLASS">{{ t("transactions.order.section") }}</div>
    <NSpin v-if="loading" size="small" />
    <template v-else-if="summary">
      <div :class="ORDER_NO_CLASS">
        <span :class="ORDER_METRIC_LABEL_CLASS">{{ t("transactions.order.no") }}</span>
        {{ summary.source_order_no }}
      </div>
      <div :class="ORDER_METRIC_CLASS">
        <span>
          <span :class="ORDER_METRIC_LABEL_CLASS">{{ t("transactions.order.items") }}</span>
          <span :class="ORDER_METRIC_VALUE_CLASS">{{
            t("transactions.order.rowCount", { n: summary.row_count })
          }}</span>
        </span>
        <span>
          <span :class="ORDER_METRIC_LABEL_CLASS">{{ t("transactions.order.total") }}</span>
          <span :class="ORDER_METRIC_VALUE_CLASS">{{ totalText }}</span>
        </span>
      </div>
      <div :class="ORDER_ACCOUNTS_CLASS">
        <span :class="ORDER_METRIC_LABEL_CLASS">{{ t("transactions.order.accounts") }}</span>
        <span v-for="a in summary.accounts" :key="a.account_id" :class="ORDER_ACCOUNT_TAG_CLASS">
          <span>{{ accountName(a.account_id) }}</span>
          <span>{{ contributionText(a.amount_cents) }}</span>
        </span>
      </div>
      <div :class="ORDER_ITEMS_CLASS">
        <div v-for="row in summary.items" :key="row.id" :class="ORDER_ITEM_ROW_CLASS">
          <span :class="ORDER_ITEM_MUTED_CLASS">{{ row.date }}</span>
          <span :class="ORDER_ITEM_ACCOUNT_CLASS">{{ accountName(row.account_id ?? "") }}</span>
          <span :class="ORDER_METRIC_VALUE_CLASS">{{ itemAmount(row) }}</span>
        </div>
      </div>
    </template>
    <div v-else :class="ORDER_ERROR_CLASS">
      {{ t("transactions.order.loadFailed", { msg: loadError ?? "" }) }}
    </div>
  </div>
</template>
