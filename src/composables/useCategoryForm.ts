import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import type { TreeSelectOption } from 'naive-ui'
import { api } from '@/api'
import { useFormShared } from '@/composables/useFormShared'
import type { TransactionInput } from '@/types'

export function useCategoryForm(kind: 'expense' | 'income', options?: { onCreated?: () => void }) {
  const { store, accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const categoryId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  const treeOptions = computed<TreeSelectOption[]>(() => store.treeCategoryOptions(kind) as unknown as TreeSelectOption[])

  async function submit() {
    if (!accountId.value) {
      message.warning('请选择账户')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入金额')
      return
    }
    const input: TransactionInput = {
      kind,
      amount_cents: Math.round(amount.value * 100),
      currency_code: currencyCode.value,
      account_id: accountId.value,
      category_id: categoryId.value,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
    }
    try {
      await api.createTransaction(input)
      message.success(kind === 'expense' ? '已记支出' : '已记收入')
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
    categoryId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, categoryId, note, date,
    accountOptions, currencyOptions, treeOptions,
    submit, resetForm,
  }
}
