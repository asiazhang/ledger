import { ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { useFormShared } from '@/composables/useFormShared'
import type { TransactionInput } from '@/types'

export function useTransferForm(options?: { onCreated?: () => void }) {
  const { accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const toAccountId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  async function submit() {
    if (!accountId.value) {
      message.warning('请选择转出账户')
      return
    }
    if (!toAccountId.value) {
      message.warning('请选择转入账户')
      return
    }
    if (accountId.value === toAccountId.value) {
      message.warning('转出账户和转入账户不能相同')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入金额')
      return
    }
    const input: TransactionInput = {
      kind: 'transfer',
      amount_cents: Math.round(amount.value * 100),
      currency_code: currencyCode.value,
      account_id: accountId.value,
      to_account_id: toAccountId.value,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
    }
    try {
      await api.createTransaction(input)
      message.success('已记转账')
      amount.value = null
      note.value = ''
      options?.onCreated?.()
    } catch (e) {
      message.error(`记账失败: ${e}`)
    }
  }

  function resetForm() {
    amount.value = null
    currencyCode.value = 'CNY'
    accountId.value = null
    toAccountId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, toAccountId, note, date,
    accountOptions, currencyOptions,
    submit, resetForm,
  }
}
