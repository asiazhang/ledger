import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { useFormShared } from '@/composables/useFormShared'
import type { Instrument, InstrumentType, TransactionInput } from '@/types'

export function useInvestmentForm(kind: 'buy' | 'sell', options?: { onCreated?: () => void }) {
  const { store, currencyOptions } = useFormShared()
  const message = useMessage()

  const accountId = ref<string | null>(null)
  const instrumentId = ref<string | null>(null)
  const quantity = ref<number | null>(null)
  const price = ref<number | null>(null)
  const fee = ref<number | null>(null)
  const note = ref('')
  const date = ref(Date.now())
  const currencyCode = ref('CNY')

  const instruments = ref<Instrument[]>([])
  const showNewInstrument = ref(false)
  const newInstrumentSymbol = ref('')
  const newInstrumentName = ref('')
  const newInstrumentType = ref<InstrumentType>('stock')

  const investmentAccountOptions = computed(() =>
    store.accounts
      .filter((a) => a.type === 'investment')
      .map((a) => ({ label: a.name, value: a.id })),
  )

  const instrumentOptions = computed(() =>
    instruments.value.map((i) => ({
      label: i.name ? `${i.symbol} · ${i.name}` : i.symbol,
      value: i.id,
    })),
  )

  const investmentAmount = computed(() => {
    if (quantity.value == null || price.value == null) return 0
    const feeValue = fee.value ?? 0
    const raw = kind === 'buy'
      ? quantity.value * price.value + feeValue
      : quantity.value * price.value - feeValue
    return Math.round(raw * 100) / 100
  })

  async function createNewInstrument() {
    if (!newInstrumentSymbol.value.trim()) {
      message.warning('请输入标的代码')
      return
    }
    try {
      const id = await api.createInstrument({
        symbol: newInstrumentSymbol.value.trim(),
        type: newInstrumentType.value,
        name: newInstrumentName.value.trim() || null,
        currency_code: currencyCode.value,
      })
      message.success('已新增标的')
      await loadInstruments()
      instrumentId.value = id
      showNewInstrument.value = false
      newInstrumentSymbol.value = ''
      newInstrumentName.value = ''
    } catch (e) {
      message.error(`新增标的失败: ${e}`)
    }
  }

  async function loadInstruments() {
    try {
      instruments.value = await api.listInstruments()
    } catch {
      // 加载失败忽略，可后续重试
    }
  }

  async function submit() {
    if (!accountId.value) {
      message.warning('请选择投资账户')
      return
    }
    if (!instrumentId.value) {
      message.warning('请选择标的')
      return
    }
    if (quantity.value == null || quantity.value <= 0) {
      message.warning(kind === 'buy' ? '请输入买入数量' : '请输入卖出数量')
      return
    }
    if (price.value == null || price.value <= 0) {
      message.warning(kind === 'buy' ? '请输入买入单价' : '请输入卖出单价')
      return
    }

    const input: TransactionInput = {
      kind,
      amount_cents: 0,
      currency_code: currencyCode.value,
      account_id: accountId.value,
      to_account_id: null,
      category_id: null,
      refund_of_transaction_id: null,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
      instrument_id: instrumentId.value,
      quantity: quantity.value,
      price_cents: Math.round(price.value * 100),
      fee_cents: fee.value ? Math.round(fee.value * 100) : null,
    }
    try {
      await api.createTransaction(input)
      message.success(kind === 'buy' ? '已记买入' : '已记卖出')
      instrumentId.value = null
      quantity.value = null
      price.value = null
      fee.value = null
      note.value = ''
      options?.onCreated?.()
    } catch (e) {
      message.error(`记账失败: ${e}`)
    }
  }

  function resetForm() {
    accountId.value = null
    instrumentId.value = null
    quantity.value = null
    price.value = null
    fee.value = null
    note.value = ''
    date.value = Date.now()
    currencyCode.value = 'CNY'
    showNewInstrument.value = false
  }

  return {
    accountId, instrumentId, quantity, price, fee, note, date, currencyCode,
    investmentAmount, investmentAccountOptions, instrumentOptions, currencyOptions,
    showNewInstrument, newInstrumentSymbol, newInstrumentName, newInstrumentType,
    submit, createNewInstrument, resetForm,
  }
}
