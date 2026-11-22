<script setup lang="ts">
import { NButton, NCard, NDataTable, NEmpty, NInput, NSpace, NSpin, NText } from "naive-ui";
import type { DataTableColumn } from "naive-ui";
import { computed, h, ref } from "vue";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { t } from "@ledger/i18n";
import { formatAmount, formatPrice, formatQuantity } from "@ledger/money";
import { useInstrumentInfoSync } from "@/investment/useInstrumentInfoSync";
import { usePricesChanged } from "@/investment/usePricesChanged";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import SyncProgressBar from "@/investment/SyncProgressBar.vue";
import SyncDegradedNotice from "@/investment/SyncDegradedNotice.vue";
import InstrumentLink from "@/investment/InstrumentLink.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import ManualPriceModal from "@/investment/ManualPriceModal.vue";
import PortfolioStatsCards from "@/investment/PortfolioStatsCards.vue";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { usePortfolioOverview, type PortfolioRow } from "@/investment/usePortfolioOverview";
import { renderMwrRateCell, useMoneyWeightedReturn } from "@/investment/useMoneyWeightedReturn";
import {
  useHoldingsFilter,
  HOLDINGS_PAGE_SIZE,
  type HoldingsSortColumn,
} from "@/investment/useHoldingsFilter";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { useWindowTier } from "@ledger/window-tier";

const reference = useReferenceStore();
const appStore = useAppStore();
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const { rows, loading, refresh, cumulativePnl, nativeCurrency } = usePortfolioOverview();
const { instrumentMwr } = useMoneyWeightedReturn();
const {
  searchInput,
  setSearch,
  accountId,
  setAccount,
  sorter,
  setSorter,
  filteredRows,
  page,
  statCards,
  accountOptions,
} = useHoldingsFilter(rows);

const { syncing, resultMessage, status, progress, degraded, sync } = useInstrumentInfoSync();

usePricesChanged(() => {
  void refresh();
});

const quoteTarget = ref<{ id: string; symbol: string } | null>(null);
const quoteOpen = ref(false);
const quoteMessage = ref<string | null>(null);

function openQuote(row: PortfolioRow) {
  quoteTarget.value = { id: row.instrumentId, symbol: row.symbol ?? row.instrumentName ?? "" };
  quoteOpen.value = true;
}

function onQuoted(message: string) {
  quoteMessage.value = message;
}

function priceGuideRowKey(row: PortfolioRow) {
  return `${row.accountId}-${row.instrumentId}`;
}

function missingPriceAction(row: PortfolioRow) {
  const testidBase = priceGuideRowKey(row);
  if (row.priceChannel === "manual") {
    return h(
      NButton,
      {
        text: true,
        type: "primary",
        size: "tiny",
        "data-testid": `missing-price-quote-${testidBase}`,
        onClick: () => openQuote(row),
      },
      { default: () => t("investments.holdings.missingPrice.quote") },
    );
  }
  if (row.priceChannel === "quote" || row.priceChannel === "fund_nav") {
    return h(
      NButton,
      {
        text: true,
        type: "primary",
        size: "tiny",
        "data-testid": `missing-price-sync-${testidBase}`,
        onClick: () => {
          void sync();
        },
      },
      { default: () => t("investments.holdings.missingPrice.sync") },
    );
  }
  return null;
}

function columnSortOrder(key: HoldingsSortColumn) {
  return sorter.value?.columnKey === key ? sorter.value.order : false;
}

const pagination = computed(() => ({
  page: page.value,
  pageSize: HOLDINGS_PAGE_SIZE,
  onChange: (next: number) => {
    page.value = next;
  },
}));

const scrollX = computed(() => sumFixedColumnWidths(overviewColumns.value));

const overviewColumns = computed<DataTableColumn<PortfolioRow>[]>(() => [
  {
    title: t("investments.holdings.columns.symbol"),
    key: "symbol",
    width: 100,
    render: (r) =>
      h(InstrumentLink, {
        instrumentId: r.instrumentId,
        accountId: r.accountId,
        label: r.symbol ?? null,
      }),
  },
  {
    title: t("investments.holdings.columns.name"),
    key: "instrumentName",
    minWidth: 160,
    ellipsis: { tooltip: true },
    render: (r) => r.instrumentName ?? "-",
  },
  {
    title: t("investments.holdings.columns.account"),
    key: "accountName",
    width: 80,
    ellipsis: { tooltip: true },
    render: (r) => r.accountName ?? "-",
  },
  {
    title: t("investments.holdings.columns.quantity"),
    key: "quantity",
    width: 110,
    align: "right",
    className: "tabular-nums",
    render: (r) => formatQuantity(r.quantity),
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.holdings.columns.cost"),
        concept: "cost",
        testId: "holdings-cost",
      }),
    key: "cost_basis",
    width: 120,
    align: "right",
    className: "tabular-nums",
    render: (r) => formatAmount(r.costBasisCents, reference.currencyMap.get(r.costCurrencyCode)),
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.holdings.columns.price"),
        concept: "price",
        testId: "holdings-price",
      }),
    key: "latest_price",
    width: 110,
    align: "right",
    className: "tabular-nums",
    render: (r) => {
      if (r.latestPriceCents !== null) {
        return formatPrice(
          r.latestPriceCents,
          reference.currencyMap.get(r.latestPriceCurrencyCode ?? ""),
        );
      }
      const action = missingPriceAction(r);
      if (action === null) return "-";
      return h("span", { style: { display: "inline-flex", alignItems: "center", gap: "4px" } }, [
        "-",
        action,
      ]);
    },
  },
  {
    title: t("investments.holdings.columns.navDate"),
    key: "nav_date",
    width: 100,
    align: "right",
    className: "tabular-nums",
    render: (r) => r.latestNavDate ?? "-",
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.holdings.columns.marketValue"),
        concept: "marketValue",
        testId: "holdings-market-value",
      }),
    key: "market_value",
    width: 120,
    align: "right",
    className: "tabular-nums",
    sorter: true,
    sortOrder: columnSortOrder("market_value"),
    render: (r) =>
      r.marketValueCents === null
        ? "-"
        : formatAmount(r.marketValueCents, reference.currencyMap.get(r.valueCurrencyCode)),
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.holdings.columns.unrealizedPnl"),
        concept: "unrealizedPnl",
        testId: "holdings-unrealized-pnl",
      }),
    key: "unrealized_pnl",
    width: 130,
    align: "right",
    className: "tabular-nums",
    sorter: true,
    sortOrder: columnSortOrder("unrealized_pnl"),
    render: (r) => {
      if (r.unrealizedPnlCents === null) return "-";
      return h(
        "span",
        { style: { color: pnlSemanticColor(r.unrealizedPnlCents, appStore.theme) } },
        formatAmount(r.unrealizedPnlCents, reference.currencyMap.get(r.valueCurrencyCode)),
      );
    },
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.holdings.columns.mwr"),
        concept: "mwr",
        testId: "holdings-mwr",
      }),
    key: "mwr",
    width: 150,
    align: "right",
    className: "tabular-nums",
    render: (r) => {
      const mwr = instrumentMwr(r.accountId, r.instrumentId);
      return renderMwrRateCell(mwr?.rate, appStore.theme, mwr?.basis);
    },
  },
]);
</script>

<template>
  <NCard
    :title="t('investments.holdings.title')"
    size="small"
    class="viewport-table-view viewport-table-card"
  >
    <template #header-extra>
      <NButton
        type="primary"
        size="small"
        :loading="syncing"
        data-testid="sync-instrument-info"
        @click="sync"
      >
        {{ t("investments.holdings.sync") }}
      </NButton>
    </template>

    <NSpin :show="loading" class="viewport-table-spin">
      <div class="viewport-table-card-body">
        <!-- 同步确定进度条（issue #897）：卡顶就近反馈，与标的页同一展示组件 -->
        <SyncProgressBar :progress="progress" />

        <!-- 与标的页一致的轻量反馈：成功/失败着色 -->
        <NText v-if="resultMessage" :type="status === 'error' ? 'error' : 'info'">
          {{ resultMessage }}
        </NText>

        <!-- 降级可见（issue #1376 / ADR-0121 决策 4）：回退逐标的通道时明示本次较慢 -->
        <SyncDegradedNotice :degraded="degraded" />

        <!-- 缺价行「录价」引导的页面级回执（issue #1193）；行内数值刷新由价格失效信号驱动 -->
        <NText v-if="quoteMessage" type="success" data-testid="manual-quote-result">
          {{ quoteMessage }}
        </NText>

        <NEmpty
          v-if="rows.length === 0 && !loading"
          :description="t('investments.holdings.empty')"
        />
        <template v-else-if="rows.length > 0">
          <!-- 三维过滤（issue #902）：搜索（300ms 防抖在 composable 内）+ 账户单选
               （与盈亏页账户下拉同源，clearable 即「全部」默认态）；无持仓时不渲染 -->
          <NSpace align="center" :size="12">
            <NInput
              :value="searchInput"
              clearable
              :placeholder="t('investments.holdings.searchPlaceholder')"
              data-testid="holdings-search"
              style="width: 240px"
              @update:value="setSearch"
            />
            <PinyinSelect
              :value="accountId"
              :options="accountOptions"
              clearable
              :placeholder="t('investments.holdings.filterAccount')"
              data-testid="holdings-account-filter"
              style="width: 180px"
              @update:value="setAccount"
            />
          </NSpace>

          <!-- 合计三卡（issue #902 / #1077；issue #1797 起折本位币单值）：形态与口径归
               PortfolioStatsCards（首页投资概览卡同一组件），合计随过滤子集更新（排序
               不影响）、累计收益为全账本口径（不受搜索/账户过滤收窄），见
               usePortfolioOverview 注解；缺料警告态的重试 = 重发同一条读命令。 -->
          <PortfolioStatsCards
            test-id-prefix="total-"
            scope="filtered"
            :market-value="statCards.marketValue"
            :unrealized-pnl="statCards.unrealizedPnl"
            :cumulative-pnl="cumulativePnl"
            :native-currency="nativeCurrency"
            :retry="refresh"
          />

          <!-- 「没有持仓」（上方）与「筛选条件下无匹配」（此处）两种空态可区分 -->
          <NEmpty
            v-if="filteredRows.length === 0"
            :description="t('investments.holdings.filterNoMatch')"
            data-testid="holdings-no-match"
          />
          <div v-else class="viewport-table-slot">
            <NDataTable
              :columns="overviewColumns"
              :data="filteredRows"
              :bordered="false"
              size="small"
              :flex-height="!isMobileTier"
              :row-key="(r: PortfolioRow) => r.holdingId"
              :scroll-x="scrollX"
              :pagination="pagination"
              :paginate-single-page="false"
              @update:sorter="setSorter"
            />
          </div>
        </template>
      </div>
    </NSpin>

    <!-- 缺价行「录价」引导打开既有录价弹窗（issue #1193）：与标的页行内「录价」
         同一弹窗、同一命令，不新增第二套入口；提交回执转页面级展示 -->
    <ManualPriceModal v-model:show="quoteOpen" :instrument="quoteTarget" @quoted="onQuoted" />
  </NCard>
</template>
