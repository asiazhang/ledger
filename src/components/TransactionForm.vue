<script setup lang="ts">
import { ref } from 'vue'
import { NRadioGroup, NRadio } from 'naive-ui'
import type { TransactionKind } from '@/types'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import RefundForm from '@/components/RefundForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

const emit = defineEmits<{ created: [] }>()
defineProps<{ onCreated?: () => void }>()

const kind = ref<TransactionKind>('expense')
</script>

<template>
  <div>
    <NRadioGroup v-model:value="kind" style="margin-bottom: 12px">
      <NRadio value="expense">支出</NRadio>
      <NRadio value="income">收入</NRadio>
      <NRadio value="transfer">转账</NRadio>
      <NRadio value="refund">退款</NRadio>
      <NRadio value="buy">买入</NRadio>
      <NRadio value="sell">卖出</NRadio>
    </NRadioGroup>

    <CategoryForm
      v-if="kind === 'expense'"
      kind="expense"
      submit-label="记支出"
      @created="emit('created')"
    />
    <CategoryForm
      v-if="kind === 'income'"
      kind="income"
      submit-label="记收入"
      @created="emit('created')"
    />
    <TransferForm
      v-if="kind === 'transfer'"
      @created="emit('created')"
    />
    <RefundForm
      v-if="kind === 'refund'"
      @created="emit('created')"
    />
    <InvestmentForm
      v-if="kind === 'buy'"
      kind="buy"
      submit-label="记买入"
      @created="emit('created')"
    />
    <InvestmentForm
      v-if="kind === 'sell'"
      kind="sell"
      submit-label="记卖出"
      @created="emit('created')"
    />
  </div>
</template>
