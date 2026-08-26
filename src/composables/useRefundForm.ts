import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { formatAmount } from '@/types'
import { useFormShared } from '@/composables/useFormShared'
import type { Transaction, TransactionInput } from '@/types'

export function useRefundForm(options?: { onCreated?: () => void }) {
  const { reference, accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const refundTargetId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  const transactions = ref<Transaction[]>([])

  const expenseTransactions = computed(() =>
    transactions.value.filter((t) => t.kind === 'expense'),
  )

  const refundTargetOptions = computed(() =>
    expenseTransactions.value.map((t) => {
      const cat = reference.categoryPath(t.category_id) || '-'
      const cur = reference.getCurrency(t.currency_code)
      const amt = formatAmount(t.amount_native_cents, cur)
      const noteStr = t.note ? ` · ${t.note}` : ''
      return {
        label: `${t.date}  ${amt}  ${cat}${noteStr}`,
        value: t.id,
      }
    }),
  )

  const refundTarget = computed<Transaction | null>(() =>
    refundTargetId.value == null
      ? null
      : (expenseTransactions.value.find((t) => t.id === refundTargetId.value) ?? null),
  )

  async function loadTransactions() {
    try {
      transactions.value = (await api.listTransactions()).items
    } catch {
      // 加载失败忽略，退款关联可后续重试
    }
  }

  async function submit() {
    if (!refundTargetId.value) {
      message.warning('请选择要退款的原始支出交易')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入退款金额')
      return
    }
    const input: TransactionInput = {
      kind: 'refund',
      amount_cents: Math.round(amount.value * 100),
      currency_code: currencyCode.value,
      account_id: accountId.value!,
      to_account_id: null,
      category_id: null,
      refund_of_transaction_id: refundTargetId.value,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
    }
    try {
      await api.createTransaction(input)
      message.success('已记退款')
      await loadTransactions()
      amount.value = null
      note.value = ''
      refundTargetId.value = null
      options?.onCreated?.()
    } catch (e) {
      message.error(`退款失败: ${e}`)
    }
  }

  function resetForm() {
    amount.value = null
    currencyCode.value = 'CNY'
    accountId.value = null
    refundTargetId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, refundTargetId, note, date,
    accountOptions, currencyOptions,
    expenseTransactions, refundTargetOptions, refundTarget,
    submit, resetForm,
  }
}
