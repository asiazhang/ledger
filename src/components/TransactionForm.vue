<script setup lang="ts">
import { NRadioGroup, NRadio } from 'naive-ui'
import { useTransactionForm } from '@/composables/useTransactionForm'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import RefundForm from '@/components/RefundForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

const emit = defineEmits<{ created: [] }>()
defineProps<{ onCreated?: () => void }>()

const ctx = useTransactionForm({ onCreated: () => emit('created') })
</script>

<template>
  <div>
    <NRadioGroup v-model:value="ctx.kind.value" style="margin-bottom: 12px">
      <NRadio value="expense">支出</NRadio>
      <NRadio value="income">收入</NRadio>
      <NRadio value="transfer">转账</NRadio>
      <NRadio value="refund">退款</NRadio>
      <NRadio value="buy">买入</NRadio>
      <NRadio value="sell">卖出</NRadio>
    </NRadioGroup>

    <CategoryForm
      v-if="ctx.kind.value === 'expense'"
      :ctx="ctx"
      :kind="'expense'"
      submit-label="记支出"
    />
    <CategoryForm
      v-if="ctx.kind.value === 'income'"
      :ctx="ctx"
      :kind="'income'"
      submit-label="记收入"
    />
    <TransferForm
      v-if="ctx.kind.value === 'transfer'"
      :ctx="ctx"
    />
    <RefundForm
      v-if="ctx.kind.value === 'refund'"
      :ctx="ctx"
    />
    <InvestmentForm
      v-if="ctx.kind.value === 'buy'"
      :ctx="ctx"
      :kind="'buy'"
      submit-label="记买入"
    />
    <InvestmentForm
      v-if="ctx.kind.value === 'sell'"
      :ctx="ctx"
      :kind="'sell'"
      submit-label="记卖出"
    />
  </div>
</template>
