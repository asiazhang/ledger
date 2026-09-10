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
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import { formatAmount, formatPrice, formatQuantity } from '@/types'
import { useInstrumentInfoSync } from '@/composables/useInstrumentInfoSync'
import { usePricesChanged } from '@/composables/usePricesChanged'
import { pnlSemanticColor } from '@/theme/semantic-colors'
import SyncProgressBar from '@/components/investments/SyncProgressBar.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import {
  formatCurrencyGroups,
  usePortfolioOverview,
  type PortfolioRow,
} from '@/composables/usePortfolioOverview'
import {
  useHoldingsFilter,
  HOLDINGS_PAGE_SIZE,
  type HoldingsSortColumn,
} from '@/composables/useHoldingsFilter'
import { sumFixedColumnWidths } from '@/utils/table'

const reference = useReferenceStore()
const appStore = useAppStore()

// 数据拉取（list_holdings + 持仓标的字典拼装）归 usePortfolioOverview——与首页
// 投资概览卡共享同一拼装接缝（issue #901/#902 契约不动）；过滤/排序/合计派生
// 与页码归 useHoldingsFilter，全在前端内存完成，实例随页签挂载而生、
// 卸载而灭（页签与筛选/排序/页码状态全瞬态，进入投资视图一律回默认）。
const { rows, loading, refresh } = usePortfolioOverview()
const {
  searchInput,
  setSearch,
  accountId,
  setAccount,
  sorter,
  setSorter,
  filteredRows,
  page,
  setPage,
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

// 盈亏数字着色：红涨绿跌（A股/基金语境，词汇表「盈亏涨跌色」），随主题取亮/暗变体

// 市值/未实现盈亏两列列头排序为受控形态（sorter: true + 受控 sortOrder）：
// 排序状态与行集合产出都归 useHoldingsFilter，表头只回传点击意图。
function columnSortOrder(key: HoldingsSortColumn) {
  return sorter.value?.columnKey === key ? sorter.value.order : false
}

// 分页（issue #912）：页码状态归 useHoldingsFilter（三维任一变化即翻页归零，
// 卸载重挂回默认），切片由表格内置分页完成（客户端模式按页码内存切片，
// itemCount 缺省取行集长度）；页大小固定 20 不设选择器；单页时收起分页条
// （paginate-single-page=false，≤20 行全量直显不出翻页噪声）。合计/空态在
// 切片前判定（派生自 filteredRows），与可见页无关。
const pagination = computed(() => ({
  page: page.value,
  pageSize: HOLDINGS_PAGE_SIZE,
  onChange: setPage,
}))

// 横向滚动下限 = 各固定列宽总和（全仓单一收口）：名称列是唯一弹性列（minWidth
// 不计入），窄窗口由横向滚动吸收（本表不设窗口分级分支，#898 边界维持；
// 标的页签表格的移动档 scroll-x 分支见 InstrumentBrowser，issue #849）。
const scrollX = computed(() => sumFixedColumnWidths(overviewColumns.value))

// 列形态遵循词汇表「表格列形态」约定：数值列右对齐 + 等宽数字（className 单点
// 挂全局工具类），长名称列弹性 + 单行 ellipsis 悬停全名，短内容列按内容定宽。
const overviewColumns = computed<DataTableColumn<PortfolioRow>[]>(() => [
  { title: t('investments.holdings.columns.symbol'), key: 'symbol', width: 100, render: (r) => r.symbol ?? '-' },
  {
    title: t('investments.holdings.columns.name'),
    key: 'instrumentName',
    // 唯一弹性列：不设固定宽，独吃窗口剩余宽度；minWidth 保窄窗口下限
    minWidth: 160,
    ellipsis: { tooltip: true },
    render: (r) => r.instrumentName ?? '-',
  },
  {
    title: t('investments.holdings.columns.account'),
    key: 'accountName',
    width: 80,
    ellipsis: { tooltip: true },
    render: (r) => r.accountName ?? '-',
  },
  {
    title: t('investments.holdings.columns.quantity'),
    key: 'quantity',
    width: 110,
    align: 'right',
    className: 'tabular-nums',
    render: (r) => formatQuantity(r.quantity),
  },
  {
    title: t('investments.holdings.columns.cost'),
    key: 'cost_basis',
    width: 120,
    align: 'right',
    className: 'tabular-nums',
    render: (r) => formatAmount(r.costBasisCents, reference.currencyMap.get(r.costCurrencyCode)),
  },
  {
    title: t('investments.holdings.columns.price'),
    key: 'latest_price',
    width: 110,
    align: 'right',
    className: 'tabular-nums',
    // 现价为价格列（万分之一元刻度，ADR-0038），用 formatPrice 展示；
    // 净值日期已独立成列（issue #912），本列恢复单行渲染。
    render: (r) =>
      r.latestPriceCents === null
        ? '-'
        : formatPrice(r.latestPriceCents, reference.currencyMap.get(r.latestPriceCurrencyCode ?? '')),
  },
  {
    title: t('investments.holdings.columns.navDate'),
    key: 'nav_date',
    width: 100,
    align: 'right',
    className: 'tabular-nums',
    // 净值日期独立成列（#303 形态修订，issue #912）：仅基金行携带（现价 =
    // 最新公布单位净值），其余行显示「-」；不可排序。
    render: (r) => r.latestNavDate ?? '-',
  },
  {
    title: t('investments.holdings.columns.marketValue'),
    key: 'market_value',
    width: 120,
    align: 'right',
    className: 'tabular-nums',
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
    align: 'right',
    className: 'tabular-nums',
    sorter: true,
    sortOrder: columnSortOrder('unrealized_pnl'),
    render: (r) => {
      if (r.unrealizedPnlCents === null) return '-'
      return h(
        'span',
        { style: { color: pnlSemanticColor(r.unrealizedPnlCents, appStore.theme) } },
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
            :scroll-x="scrollX"
            :pagination="pagination"
            :paginate-single-page="false"
            @update:sorter="setSorter"
          />
        </template>
      </NSpace>
    </NSpin>
  </NCard>
</template>
