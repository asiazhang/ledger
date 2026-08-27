<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NCard, NGrid, NGridItem, NSpace, NText } from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { AccountBalance } from '@/types'

// 首页仅保留逐账户余额卡片；快速记账已迁至交易页「记一笔」弹窗、
// 最近交易列表已移除（issue #141），为仪表盘改造（issue #140）腾位。
const reference = useReferenceStore()
const balances = ref<AccountBalance[]>([])

onMounted(async () => {
  balances.value = await api.listAccountBalances()
})
</script>

<template>
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
