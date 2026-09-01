<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import type { CreateFormKind, Transaction, TransactionTrade } from '@/types'
import CategoryForm from '@/components/CategoryForm.vue'
import TransferForm from '@/components/TransferForm.vue'
import LendingForm from '@/components/LendingForm.vue'
import InvestmentForm from '@/components/InvestmentForm.vue'
import { isLendingEntryKind, resolveLendingDirection } from '@/domain/lending'

// 类型选择由「记一笔」分裂按钮入口单点表达，弹窗内不再提供切换（issue #150）。
// 编辑模式（issue #178/#180）：传入 editing 时 kind 由既有交易锁死（按 editing.kind
// 分派表单，不可切换；refund 本期不开放编辑，父层不会以编辑模式传入）。
// buy/sell 编辑另需 trade（买卖明细，扩展表投影）回填标的/数量/价格/费用（issue #180）。
// 借贷入口（issue #374）：lend/borrow 是转账的借贷变体（不新增交易 kind），经 LendingForm
// 呈现；编辑形态识别同理——既有转账两端账户类型构成借贷（receivable/debt）时以借贷
// 变体回填（方向由账户类型派生），普通转账仍走转账表单。
const props = defineProps<{
  kind?: CreateFormKind
  editing?: Transaction | null
  trade?: TransactionTrade | null
}>()

const emit = defineEmits<{ created: []; saved: [] }>()

const reference = useReferenceStore()

const effectiveKind = computed<CreateFormKind | null>(() =>
  props.editing ? (props.editing.kind as CreateFormKind) : (props.kind ?? null),
)

// 编辑形态识别（issue #374）：与借贷表单的方向回填消费同一派生函数；
// 非 transfer / 普通转账 / 账户类型缺失 → null（按普通转账呈现）。
const editingLendingDirection = computed(() =>
  props.editing
    ? resolveLendingDirection(props.editing, (id) => reference.accountMap.get(id)?.type)
    : null,
)

/** 模板用：当前形态是否借贷变体入口（lend/borrow，非交易 kind；类型谓词供分支内收窄） */
function isLendingEntry(kind: CreateFormKind | null): kind is 'lend' | 'borrow' {
  return kind != null && isLendingEntryKind(kind)
}
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
      v-if="effectiveKind === 'transfer' && !editingLendingDirection"
      :editing="editing ?? null"
      @created="emit('created')"
      @saved="emit('saved')"
    />
    <LendingForm
      v-else-if="isLendingEntry(effectiveKind)"
      :initial-direction="effectiveKind"
      @created="emit('created')"
    />
    <LendingForm
      v-else-if="effectiveKind === 'transfer' && editingLendingDirection"
      :editing="editing ?? null"
      :initial-direction="editingLendingDirection"
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
