<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NAlert, NCard, NGrid, NGridItem, NSpace, NSpin, NText } from 'naive-ui'
import { api } from '@/api'
import { useDashboardOverview } from '@/composables/useDashboardOverview'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { AccountBalance } from '@/types'

// 首页顶部净资产总览卡（issue #143）：多币种折算与合计全部在后端
// `dashboard_overview` 完成，前端只做装配渲染；缺汇率等报错显示提示文案。
// 下方保留逐账户余额卡片；快速记账已迁至交易页「记一笔」弹窗、
// 最近交易列表已移除（issue #141），为仪表盘改造（issue #140）腾位。
const reference = useReferenceStore()
const { overview, loading, error } = useDashboardOverview()
const balances = ref<AccountBalance[]>([])

onMounted(async () => {
  balances.value = await api.listAccountBalances()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard size="small" data-testid="net-worth-card">
      <NSpin :show="loading">
        <NSpace vertical :size="4">
          <NText depth="3" style="font-size: 12px">净资产</NText>
          <template v-if="overview">
            <NText strong style="font-size: 28px">
              {{
                formatAmount(
                  overview.net_worth_cents,
                  reference.getCurrency(overview.native_currency),
                )
              }}
            </NText>
          </template>
          <NAlert v-else-if="error" type="warning" :bordered="false">
            {{ error }}
          </NAlert>
        </NSpace>
      </NSpin>
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
  </NSpace>
</template>
