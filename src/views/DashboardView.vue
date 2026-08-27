<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NCard, NGi, NGrid, NGridItem, NSpace, NStatistic, NText } from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { AccountBalance } from '@/types'
import {
  formatCurrencyGroups,
  usePortfolioOverview,
} from '@/composables/usePortfolioOverview'

// 首页仅保留逐账户余额卡片与投资概览卡；快速记账已迁至交易页「记一笔」弹窗、
// 最近交易列表已移除（issue #141），为仪表盘改造（issue #140）腾位。
const reference = useReferenceStore()
const balances = ref<AccountBalance[]>([])

// 投资概览卡（issue #145）：复用持仓概览 composable 的分组求和结果，
// 无任何持仓时整卡隐藏；无行情标的不以零计入合计（sumByCurrency 跳过空值）。
const {
  rows: holdingRows,
  totalMarketValueGroups,
  totalUnrealizedPnlGroups,
} = usePortfolioOverview()

onMounted(async () => {
  balances.value = await api.listAccountBalances()
})
</script>

<template>
  <NCard
    v-if="holdingRows.length > 0"
    title="投资概览"
    size="small"
    data-testid="investment-overview-card"
  >
    <NGrid :x-gap="16" cols="1 s:2">
      <NGi>
        <NStatistic label="总市值" data-testid="dashboard-total-market-value">
          {{ formatCurrencyGroups(totalMarketValueGroups, reference.currencyMap) }}
        </NStatistic>
      </NGi>
      <NGi>
        <NStatistic label="未实现盈亏合计" data-testid="dashboard-total-unrealized-pnl">
          {{ formatCurrencyGroups(totalUnrealizedPnlGroups, reference.currencyMap) }}
        </NStatistic>
      </NGi>
    </NGrid>
  </NCard>

  <NGrid :cols="3" :x-gap="16" :y-gap="16" responsive="screen">
    <NGridItem v-for="b in balances" :key="b.account.id">
      <NCard size="small">
        <NSpace vertical :size="4">
          <NText depth="3" style="font-size: 12px">
            {{ b.account.name }}
          </NText>
          <NText strong style="font-size: 22px">
            {{ formatAmount(b.balance_cents, reference.getCurrency(b.account.currency_code)) }}
          </NText>
        </NSpace>
      </NCard>
    </NGridItem>
  </NGrid>
</template>
