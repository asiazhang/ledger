<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import type { CreateTransactionKind, Transaction, TransactionTrade } from '@/types'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'

// 类型选择由「记一笔」分裂按钮入口单点表达，弹窗内不再提供切换（issue #150）。
// 编辑模式（issue #178/#180）：传入 editing 时 kind 由既有交易锁死（按 editing.kind
// 分派表单，不可切换；refund 本期不开放编辑，父层不会以编辑模式传入）。
// buy/sell 编辑另需 trade（买卖明细，扩展表投影）回填标的/数量/价格/费用（issue #180）。
const props = defineProps<{
  kind?: CreateTransactionKind
  editing?: Transaction | null
  trade?: TransactionTrade | null
}>()

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
      :submit-label="t('transactions.form.submitExpense')"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <CategoryForm
      v-if="effectiveKind === 'income'"
      kind="income"
      :submit-label="t('transactions.form.submitIncome')"
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
      :submit-label="t('transactions.form.submitBuy')"
      :editing="editing ?? null"
      :trade="trade ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <InvestmentForm
      v-if="effectiveKind === 'sell'"
      kind="sell"
      :submit-label="t('transactions.form.submitSell')"
      :editing="editing ?? null"
      :trade="trade ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
  </div>
</template>
