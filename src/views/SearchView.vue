<script setup lang="ts">
import { t } from "@ledger/i18n";
import { computed, ref, shallowRef, watch } from "vue";
import {
  NButton,
  NDataTable,
  NEmpty,
  NInput,
  NPagination,
  NSpace,
  NSpin,
  NText,
  useMessage,
} from "naive-ui";
import type { DataTableColumn } from "naive-ui";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import TransactionCardList from "@/transaction/TransactionCardList.vue";
import { useTransactionTableCellActions } from "@/transaction/table-cell-actions";
import { useInputMode } from "@/composables/useInputMode";
import { useWindowTier } from "@ledger/window-tier";
import { useLoadable } from "@ledger/loadable";
import { api } from "@ledger/api";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import {
  buildTransactionColumns,
  precomputeTransactionDisplayRows,
} from "@/transaction/transaction-columns";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { type Transaction, type TransactionSearchFilter } from "@ledger/types";
import type { NullableDateRange } from "@ledger/utils/time-period";
import { yuanToCents, formatAmount } from "@ledger/money";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";

const store = useAppStore();
const reference = useReferenceStore();
const inputMode = useInputMode();
const message = useMessage();
const tier = useWindowTier();
const isMobile = computed(() => tier.value === "mobile");

const keyword = ref("");
const amountMinYuan = ref("");
const amountMaxYuan = ref("");
const dateFrom = ref<string | null>(null);
const dateTo = ref<string | null>(null);
const results = shallowRef<Transaction[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = 20;
const searched = ref(false);

let debounceTimer: ReturnType<typeof setTimeout> | undefined;

const {
  loading,
  run: runSearchTask,
  invalidate,
} = useLoadable(() =>
  api.searchTransactions(keyword.value.trim(), page.value, pageSize, buildFilter()),
);

const amountMinCents = computed(() => yuanToCents(amountMinYuan.value));
const amountMaxCents = computed(() => yuanToCents(amountMaxYuan.value));

const quickRange = computed<NullableDateRange>({
  get: () => ({ from: dateFrom.value, to: dateTo.value }),
  set: (range) => {
    dateFrom.value = range.from;
    dateTo.value = range.to;
  },
});

const filtersActive = computed(
  () =>
    amountMinCents.value !== null ||
    amountMaxCents.value !== null ||
    !!dateFrom.value ||
    !!dateTo.value,
);

const hasQuery = computed(() => keyword.value.trim() !== "" || filtersActive.value);

const activeFilterDescriptions = computed(() => {
  const parts: string[] = [];
  const currency = reference.getCurrency(store.defaultCurrency);
  const min = amountMinCents.value;
  const max = amountMaxCents.value;
  if (min !== null && max !== null) {
    parts.push(
      t("search.filter.amountRange", {
        min: formatAmount(min, currency),
        max: formatAmount(max, currency),
      }),
    );
  } else if (min !== null) {
    parts.push(t("search.filter.amountMin", { amount: formatAmount(min, currency) }));
  } else if (max !== null) {
    parts.push(t("search.filter.amountMax", { amount: formatAmount(max, currency) }));
  }
  if (dateFrom.value) parts.push(t("search.filter.dateFrom", { date: dateFrom.value }));
  if (dateTo.value) parts.push(t("search.filter.dateTo", { date: dateTo.value }));
  return parts;
});

function buildFilter(): TransactionSearchFilter {
  return {
    amountMinCents: amountMinCents.value,
    amountMaxCents: amountMaxCents.value,
    dateFrom: dateFrom.value || null,
    dateTo: dateTo.value || null,
  };
}

async function runSearch() {
  const res = await runSearchTask();
  if (res === null) return;
  results.value = res.items;
  total.value = res.total;
  searched.value = true;
}

function scheduleSearch() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    page.value = 1;
    runSearch();
  }, SEARCH_DEBOUNCE_MS);
}

function resetResults() {
  clearTimeout(debounceTimer);
  invalidate();
  results.value = [];
  total.value = 0;
  page.value = 1;
  searched.value = false;
}

watch([keyword, amountMinYuan, amountMaxYuan, dateFrom, dateTo], () => {
  if (!hasQuery.value) {
    resetResults();
    return;
  }
  scheduleSearch();
});

function onEnter() {
  if (!hasQuery.value) return;
  clearTimeout(debounceTimer);
  page.value = 1;
  runSearch();
}

function clearFilters() {
  amountMinYuan.value = "";
  amountMaxYuan.value = "";
  dateFrom.value = null;
  dateTo.value = null;
}

const columns = computed<DataTableColumn<Transaction>[]>(() =>
  buildTransactionColumns(reference, { expandPurchases: true, inputMode }),
);

const displayRows = computed(() =>
  precomputeTransactionDisplayRows(reference, results.value, { expandPurchases: true }),
);

const scrollX = computed(() => sumFixedColumnWidths(columns.value));

function onPageChange(p: number): void {
  page.value = p;
  void runSearch();
}

const onTableCellClick = useTransactionTableCellActions({ message });

const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: onPageChange,
}));
</script>

<template>
  <div class="viewport-table-view viewport-gap-12">
    <NInput
      v-model:value="keyword"
      :placeholder="t('search.keywordPlaceholder')"
      clearable
      @keyup.enter="onEnter"
    />
    <!-- 时间范围快捷选择行（issue #526 / ADR-0070）：搜索页唯一时间控件——
         芯片「全部 | 当月 | 当季 | 当年 | 去年」＋期间步进器＋期间直达面板整行由
         QuickTimeRange 渲染；快照区间 v-model 进出（唯一事实源是本地日期条件），
         防抖自动搜索与清除筛选回「全部」由既有链路继承（交易页时间维度行同构）。 -->
    <QuickTimeRange v-model="quickRange" />
    <NSpace :size="8" align="center" :wrap="true">
      <NInput
        v-model:value="amountMinYuan"
        :placeholder="t('search.amountMinPlaceholder')"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <NInput
        v-model:value="amountMaxYuan"
        :placeholder="t('search.amountMaxPlaceholder')"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <template v-if="filtersActive">
        <NText depth="3">{{
          t("search.appliedFilters", {
            filters: activeFilterDescriptions.join(t("search.filterSeparator")),
          })
        }}</NText>
        <NButton size="tiny" quaternary type="primary" @click="clearFilters">
          {{ t("search.clearFilters") }}
        </NButton>
      </template>
    </NSpace>
    <template v-if="searched">
      <NText depth="3">{{ t("search.hitCount", { n: total }) }}</NText>
      <NEmpty v-if="total === 0" :description="t('search.noResults')" />
      <!-- 移动档（issue #846）：同构复用交易卡片列表（只读：不传行菜单/整卡回调），
           分页语义不变（翻页重新搜索） -->
      <template v-else-if="isMobile">
        <NSpin :show="loading">
          <TransactionCardList :rows="results" />
        </NSpin>
        <NPagination
          :page="page"
          :page-size="pageSize"
          :item-count="total"
          @update:page="onPageChange"
        />
      </template>
      <!-- 备注列为弹性列，表格铺满容器；窄窗口时备注先收缩，scroll-x（固定列宽总和）作为横向滚动下限。
           表格容器承载单元格动作的表级事件委托（issue #2189，见 onTableCellClick） -->
      <div v-else class="viewport-table-slot" @click="onTableCellClick">
        <NDataTable
          :columns="columns"
          :data="displayRows"
          :loading="loading"
          :bordered="false"
          size="small"
          remote
          :flex-height="!isMobile"
          :scroll-x="scrollX"
          :pagination="pagination"
        />
      </div>
    </template>
    <NEmpty v-else :description="t('search.emptyPlaceholder')" />
  </div>
</template>
