<script setup lang="ts">
import { computed } from 'vue'
import type { CreateTransactionKind, Transaction } from '@/types'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

// 类型选择由「记一笔」分裂按钮入口单点表达，弹窗内不再提供切换（issue #150）。
// 编辑模式（issue #178）：传入 editing 时 kind 由既有交易锁死（按 editing.kind
// 分派表单，不可切换；refund/buy/sell 本期不开放编辑，父层不会以编辑模式传入）。
const props = defineProps<{ kind?: CreateTransactionKind; editing?: Transaction | null }>()

const emit = defineEmits<{ created: []; saved: [] }>()

const effectiveKind = computed<CreateTransactionKind | null>(() =>
  props.editing ? (props.editing.kind as CreateTransactionKind) : (props.kind ?? null),
)
</script>

<template>
  <div>
    <CategoryForm
      v-if="effectiveKind === 'expense'"
      kind="expense"
      submit-label="记支出"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <CategoryForm
      v-if="effectiveKind === 'income'"
      kind="income"
      submit-label="记收入"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <TransferForm
      v-if="effectiveKind === 'transfer'"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <InvestmentForm
      v-if="effectiveKind === 'buy'"
      kind="buy"
      submit-label="记买入"
      @created="emit('created')"
    />
    <InvestmentForm
      v-if="effectiveKind === 'sell'"
      kind="sell"
      submit-label="记卖出"
      @created="emit('created')"
    />
  </div>
</template>
