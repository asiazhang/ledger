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
import { formatAmount, formatQuantity } from '@/types'
import { useHoldingPriceSync } from '@/composables/useHoldingPriceSync'
import {
  usePortfolioOverview,
  type CurrencyAmountGroup,
  type PortfolioRow,
} from '@/composables/usePortfolioOverview'

const reference = useReferenceStore()
const { rows, loading, totalMarketValueGroups, totalUnrealizedPnlGroups, refresh } =
  usePortfolioOverview()

// 同步按钮复用 T4 的增量同步接缝（useHoldingPriceSync），两处行为一致：
// 按钮 loading + 轻量消息反馈。同步成功后价格已更新，重拉持仓刷新现价/市值。
const { syncing, resultMessage, status, sync } = useHoldingPriceSync()

async function onSync() {
  await sync()
  if (status.value === 'success') void refresh()
}

/** 多币种账户的市值/盈亏无法合并成单一数字，按币种逐组格式化 */
function groupsText(groups: CurrencyAmountGroup[]): string {
  if (groups.length === 0) return '-'
  return groups
    .map((g) => formatAmount(g.cents, reference.currencyMap.get(g.currencyCode)))
    .join(' / ')
}

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
    width: 110,
    render: (r) =>
      r.latestPriceCents === null
        ? '-'
        : formatAmount(r.latestPriceCents, reference.currencyMap.get(r.latestPriceCurrencyCode ?? '')),
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
        @click="onSync"
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
                {{ groupsText(totalMarketValueGroups) }}
              </NStatistic>
            </NGi>
            <NGi>
              <NStatistic label="未实现盈亏合计" data-testid="total-unrealized-pnl">
                {{ groupsText(totalUnrealizedPnlGroups) }}
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
