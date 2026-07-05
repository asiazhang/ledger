<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NCard, NGrid, NGridItem, NSpace, NText, NEmpty } from 'naive-ui'
import TransactionForm from '@/components/TransactionForm.vue'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount, TRANSACTION_KIND_LABELS } from '@/types'
import type { AccountBalance, Transaction } from '@/types'

const store = useAppStore()
const balances = ref<AccountBalance[]>([])
const recent = ref<Transaction[]>([])

async function refresh() {
  balances.value = await api.listAccountBalances()
  recent.value = await api.listTransactions(10)
}

onMounted(async () => {
  await store.loadAll()
  await refresh()
})
</script>

<template>
  <NSpace vertical :size="20">
    <NGrid :cols="3" :x-gap="16" :y-gap="16" responsive="screen">
      <NGridItem v-for="b in balances" :key="b.account.id">
        <NCard size="small">
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">
              {{ b.account.name }}
            </NText>
            <NText strong style="font-size: 22px">
              {{ formatAmount(b.balance_cents, store.getCurrency(b.account.currency_code)) }}
            </NText>
          </NSpace>
        </NCard>
      </NGridItem>
    </NGrid>

    <NGrid :cols="2" :x-gap="16" :y-gap="16" responsive="screen">
      <NGridItem>
        <NCard title="快速记账" size="small">
          <TransactionForm @created="refresh" />
        </NCard>
      </NGridItem>
      <NGridItem>
        <NCard title="最近交易" size="small">
          <NEmpty v-if="recent.length === 0" description="暂无交易" />
          <NSpace v-else vertical :size="8">
            <div v-for="t in recent" :key="t.id" style="display: flex; justify-content: space-between">
              <NSpace :size="8" align="center">
                <NText>{{ TRANSACTION_KIND_LABELS[t.kind as keyof typeof TRANSACTION_KIND_LABELS] }}</NText>
                <NText depth="3" style="font-size: 12px">{{ t.date }}</NText>
                <NText depth="2">{{ t.note }}</NText>
              </NSpace>
              <NText :type="t.kind === 'income' ? 'success' : t.kind === 'expense' ? 'error' : t.kind === 'refund' ? 'info' : 'default'">
                {{ formatAmount(t.amount_native_cents, store.getCurrency(t.currency_code)) }}
              </NText>
            </div>
          </NSpace>
        </NCard>
      </NGridItem>
    </NGrid>
  </NSpace>
</template>
