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

/**
 * 所属订单区（issue #1862 / ADR-0138 决策 9）：按行上来源订单号取同单汇总的
 * 只读呈现面——行数 · 合计 · 按账户聚合的出资构成徽标（单账户订单 1 枚、多账户
 * 订单 ≥2 枚）+ 各行明细（日期升序，对照回单核对）。
 *
 * 数据自取：经订单汇总只读命令（IPC，无 HTTP 端点），加载三态内化在组件——
 * 打开弹窗的编排不做「先取再开窗」（dividend / funding 同款「仅作渲染面判别」，
 * 区块有加载态，失败不拦开窗）。行尾静态订单徽章（列表侧）与订单区共用
 * source_order_no 锚点；无订单号的行不渲染本区（调用方 v-if 保证）。
 *
 * 金额口径：合计与构成金额为行原始币种金额（币种唯一时按其格式化，混合币种
 * 回落展示币种偏好——后端不静默混算，见 TransactionOrderSummary 契约）；明细行
 * 金额与列表同源 `displayAmountText`（本位币口径 + 金额隐私模式收口在格式化层）。
 */
const props = defineProps<{
  /** 行上来源订单号（同单聚合锚点） */
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

/** 未知账户回退占位（与列表账户列同口径，不抛错）。 */
function accountName(accountId: string): string {
  return reference.accountMap.get(accountId)?.name ?? "—";
}

/** 展示币种字典：同单币种唯一按其解析；混合币种（后端 None）回落展示币种偏好。 */
const summaryCurrency = computed(() =>
  summary.value?.currency_code
    ? reference.getCurrency(summary.value.currency_code)
    : reference.getCurrency(app.defaultCurrency),
);

/** 合计（原始币种直和）。 */
const totalText = computed(() =>
  summary.value ? formatAmount(summary.value.total_amount_cents, summaryCurrency.value) : "",
);

/** 构成金额（聚合口径与合计同币种）。 */
function contributionText(amountCents: number): string {
  return formatAmount(amountCents, summaryCurrency.value);
}

/** 明细行金额：与列表金额列同口径单点（本位币 + 隐私模式）。 */
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
