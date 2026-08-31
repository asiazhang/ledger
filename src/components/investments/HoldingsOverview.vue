<script setup lang="ts">
import {
  NButton,
  NCard,
  NDataTable,
  NEmpty,
  NGi,
  NGrid,
  NSpace,
  NSpin,
  NStatistic,
  NText,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { h } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount, formatPrice, formatQuantity } from '@/types'
import { useHoldingPriceSync } from '@/composables/useHoldingPriceSync'
import { usePricesChanged } from '@/composables/usePricesChanged'
import {
  formatCurrencyGroups,
  usePortfolioOverview,
  type PortfolioRow,
} from '@/composables/usePortfolioOverview'

const reference = useReferenceStore()
const { rows, loading, totalMarketValueGroups, totalUnrealizedPnlGroups, refresh } =
  usePortfolioOverview()

// 同步按钮复用 T4 的增量同步接缝（useHoldingPriceSync），两处行为一致：
// 按钮 loading + 轻量消息反馈。同步后重拉不绑在调用方自觉里：后端实际
// 写价后 emit 价格失效信号（ADR-0031），此处订阅重拉现价/市值（含本卡
// 所在隐藏 tab 常驻挂载的场景）；失败/零更新后端不 emit，无谓重拉也不发生。
const { syncing, resultMessage, status, sync } = useHoldingPriceSync()

usePricesChanged(() => {
  void refresh()
})

function pnlColor(cents: number): string {
  return cents >= 0 ? '#18a058' : '#d03050'
}

const overviewColumns: DataTableColumn<PortfolioRow>[] = [
  { title: '标的', key: 'symbol', width: 100, render: (r) => r.symbol ?? '-' },
  { title: '名称', key: 'instrumentName', width: 160, render: (r) => r.instrumentName ?? '-' },
  { title: '账户', key: 'accountName', width: 120, render: (r) => r.accountName ?? '-' },
  { title: '数量', key: 'quantity', width: 80, render: (r) => formatQuantity(r.quantity) },
  {
    title: '成本',
    key: 'cost_basis',
    width: 110,
    render: (r) => formatAmount(r.costBasisCents, reference.currencyMap.get(r.costCurrencyCode)),
  },
  {
    title: '现价',
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
          `净值 ${r.latestNavDate}`,
        ),
      ])
    },
  },
  {
    title: '市值',
    key: 'market_value',
    width: 110,
    render: (r) =>
      r.marketValueCents === null
        ? '-'
        : formatAmount(r.marketValueCents, reference.currencyMap.get(r.valueCurrencyCode)),
  },
  {
    title: '未实现盈亏',
    key: 'unrealized_pnl',
    width: 130,
    render: (r) => {
      if (r.unrealizedPnlCents === null) return '-'
      return h(
        'span',
        { style: { color: pnlColor(r.unrealizedPnlCents) } },
        formatAmount(r.unrealizedPnlCents, reference.currencyMap.get(r.valueCurrencyCode)),
      )
    },
  },
]
</script>

<template>
  <NCard title="当前持仓" size="small">
    <template #header-extra>
      <NButton
        type="primary"
        size="small"
        :loading="syncing"
        data-testid="sync-holding-prices"
        @click="sync"
      >
        同步持仓价格
      </NButton>
    </template>

    <NSpin :show="loading">
      <NSpace vertical :size="12">
        <!-- 与标的页一致的轻量反馈：成功/失败着色 -->
        <NText v-if="resultMessage" :type="status === 'error' ? 'error' : 'info'">
          {{ resultMessage }}
        </NText>

        <NEmpty v-if="rows.length === 0 && !loading" description="暂无持仓" />
        <template v-else-if="rows.length > 0">
          <NGrid :x-gap="16" cols="1 s:2">
            <NGi>
              <NStatistic label="总市值" data-testid="total-market-value">
                {{ formatCurrencyGroups(totalMarketValueGroups, reference.currencyMap) }}
              </NStatistic>
            </NGi>
            <NGi>
              <NStatistic label="未实现盈亏合计" data-testid="total-unrealized-pnl">
                {{ formatCurrencyGroups(totalUnrealizedPnlGroups, reference.currencyMap) }}
              </NStatistic>
            </NGi>
          </NGrid>

          <NDataTable
            :columns="overviewColumns"
            :data="rows"
            :bordered="false"
            size="small"
            :row-key="(r: PortfolioRow) => r.holdingId"
          />
        </template>
      </NSpace>
    </NSpin>
  </NCard>
</template>
