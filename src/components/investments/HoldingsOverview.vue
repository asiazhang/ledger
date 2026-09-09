<script setup lang="ts">
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NGi,
  NGrid,
  NInput,
  NSpace,
  NSpin,
  NStatistic,
  NText,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { computed, h } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import { formatAmount, formatPrice, formatQuantity } from '@/types'
import { useInstrumentInfoSync } from '@/composables/useInstrumentInfoSync'
import { usePricesChanged } from '@/composables/usePricesChanged'
import SyncProgressBar from '@/components/investments/SyncProgressBar.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import {
  formatCurrencyGroups,
  usePortfolioOverview,
  type PortfolioRow,
} from '@/composables/usePortfolioOverview'
import {
  useHoldingsFilter,
  type HoldingsSortColumn,
} from '@/composables/useHoldingsFilter'

const reference = useReferenceStore()

// 数据拉取（list_holdings + 持仓标的字典拼装）归 usePortfolioOverview——与首页
// 投资概览卡共享同一拼装接缝（issue #901/#902 契约不动）；过滤/排序/合计派生
// 归 useHoldingsFilter 三维深模块，全在前端内存完成，实例随页签挂载而生、
// 卸载而灭（页签与筛选/排序状态全瞬态，进入投资视图一律回默认）。
const { rows, loading, refresh } = usePortfolioOverview()
const {
  searchInput,
  setSearch,
  accountId,
  setAccount,
  sorter,
  setSorter,
  filteredRows,
  totalMarketValueGroups,
  totalUnrealizedPnlGroups,
  accountOptions,
} = useHoldingsFilter(rows)

// 同步按钮复用 T4 的同步接缝（useInstrumentInfoSync），两处行为一致：
// 按钮 loading + 轻量消息反馈 + 确定进度条（issue #897，与标的页同一份
// SyncProgressBar 展示组件、同一份共享进度状态）。同步后重拉不绑在调用方自觉里：后端实际
// 写价后 emit 价格失效信号（ADR-0031），此处订阅重拉现价/市值（含本卡
// 所在隐藏 tab 常驻挂载的场景）；失败/零更新后端不 emit，无谓重拉也不发生。
const { syncing, resultMessage, status, progress, sync } = useInstrumentInfoSync()

usePricesChanged(() => {
  void refresh()
})

function pnlColor(cents: number): string {
  return cents >= 0 ? '#18a058' : '#d03050'
}

// 市值/未实现盈亏两列列头排序为受控形态（sorter: true + 受控 sortOrder）：
// 排序状态与行集合产出都归 useHoldingsFilter，表头只回传点击意图。
function columnSortOrder(key: HoldingsSortColumn) {
  return sorter.value?.columnKey === key ? sorter.value.order : false
}

const overviewColumns = computed<DataTableColumn<PortfolioRow>[]>(() => [
  { title: t('investments.holdings.columns.symbol'), key: 'symbol', width: 100, render: (r) => r.symbol ?? '-' },
  { title: t('investments.holdings.columns.name'), key: 'instrumentName', width: 160, render: (r) => r.instrumentName ?? '-' },
  { title: t('investments.holdings.columns.account'), key: 'accountName', width: 120, render: (r) => r.accountName ?? '-' },
  { title: t('investments.holdings.columns.quantity'), key: 'quantity', width: 80, render: (r) => formatQuantity(r.quantity) },
  {
    title: t('investments.holdings.columns.cost'),
    key: 'cost_basis',
    width: 110,
    render: (r) => formatAmount(r.costBasisCents, reference.currencyMap.get(r.costCurrencyCode)),
  },
  {
    title: t('investments.holdings.columns.price'),
    key: 'latest_price',
    width: 130,
    // 现价为价格列（万分之一元刻度，ADR-0038），用 formatPrice 展示；
    // 基金现价 = 最新公布单位净值，下方小字展示净值日期——现价对应哪天的
    // 净值一眼可辨（#303），股票无净值日期不渲染该行。
    render: (r) => {
      if (r.latestPriceCents === null) return '-'
      const price = formatPrice(
        r.latestPriceCents,
        reference.currencyMap.get(r.latestPriceCurrencyCode ?? ''),
      )
      if (r.latestNavDate === null) return price
      return h('div', [
        price,
        h(
          'div',
          { style: 'font-size:12px;opacity:.65;line-height:1.4' },
          t('investments.holdings.navDate', { date: r.latestNavDate }),
        ),
      ])
    },
  },
  {
    title: t('investments.holdings.columns.marketValue'),
    key: 'market_value',
    width: 110,
    sorter: true,
    sortOrder: columnSortOrder('market_value'),
    render: (r) =>
      r.marketValueCents === null
        ? '-'
        : formatAmount(r.marketValueCents, reference.currencyMap.get(r.valueCurrencyCode)),
  },
  {
    title: t('investments.holdings.columns.unrealizedPnl'),
    key: 'unrealized_pnl',
    width: 130,
    sorter: true,
    sortOrder: columnSortOrder('unrealized_pnl'),
    render: (r) => {
      if (r.unrealizedPnlCents === null) return '-'
      return h(
        'span',
        { style: { color: pnlColor(r.unrealizedPnlCents) } },
        formatAmount(r.unrealizedPnlCents, reference.currencyMap.get(r.valueCurrencyCode)),
      )
    },
  },
])
</script>

<template>
  <NCard :title="t('investments.holdings.title')" size="small">
    <template #header-extra>
      <NButton
        type="primary"
        size="small"
        :loading="syncing"
        data-testid="sync-instrument-info"
        @click="sync"
      >
        {{ t('investments.holdings.sync') }}
      </NButton>
    </template>

    <NSpin :show="loading">
      <NSpace vertical :size="12">
        <!-- 同步确定进度条（issue #897）：卡顶就近反馈，与标的页同一展示组件 -->
        <SyncProgressBar :progress="progress" />

        <!-- 与标的页一致的轻量反馈：成功/失败着色 -->
        <NText v-if="resultMessage" :type="status === 'error' ? 'error' : 'info'">
          {{ resultMessage }}
        </NText>

        <NEmpty v-if="rows.length === 0 && !loading" :description="t('investments.holdings.empty')" />
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

          <!-- 合计随过滤子集更新（排序不影响）；排序只是重排行，不换口径 -->
          <NGrid :x-gap="16" cols="1 s:2">
            <NGi>
              <NStatistic :label="t('investments.holdings.totalMarketValue')" data-testid="total-market-value">
                {{ formatCurrencyGroups(totalMarketValueGroups, reference.currencyMap) }}
              </NStatistic>
            </NGi>
            <NGi>
              <NStatistic :label="t('investments.holdings.totalUnrealizedPnl')" data-testid="total-unrealized-pnl">
                {{ formatCurrencyGroups(totalUnrealizedPnlGroups, reference.currencyMap) }}
              </NStatistic>
            </NGi>
          </NGrid>

          <!-- 「没有持仓」（上方）与「筛选条件下无匹配」（此处）两种空态可区分 -->
          <NEmpty
            v-if="filteredRows.length === 0"
            :description="t('investments.holdings.filterNoMatch')"
            data-testid="holdings-no-match"
          />
          <NDataTable
            v-else
            :columns="overviewColumns"
            :data="filteredRows"
            :bordered="false"
            size="small"
            :row-key="(r: PortfolioRow) => r.holdingId"
            @update:sorter="setSorter"
          />
        </template>
      </NSpace>
    </NSpin>
  </NCard>
</template>
