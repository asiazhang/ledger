<script setup lang="ts">
import type { CreateTransactionKind } from '@/types'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

// 类型选择由「记一笔」分裂按钮入口单点表达，弹窗内不再提供切换（issue #150）。
defineProps<{ kind: CreateTransactionKind }>()

const emit = defineEmits<{ created: [] }>()
</script>

<template>
  <div>
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
