import { type ComputedRef, computed, onMounted, type Ref, ref, watch } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount } from '@/types'
import type { Instrument, InstrumentType, Transaction, TransactionInput, TransactionKind } from '@/types'
import type { CategoryTreeNode } from '@/stores/app'

export interface TransactionFormContext {
  kind: Ref<TransactionKind>
  amount: Ref<number | null>
  currencyCode: Ref<string>
  accountId: Ref<string | null>
  toAccountId: Ref<string | null>
  categoryId: Ref<string | null>
  refundTargetId: Ref<string | null>
  note: Ref<string>
  date: Ref<number>
  instrumentId: Ref<string | null>
  quantity: Ref<number | null>
  price: Ref<number | null>
  fee: Ref<number | null>
  instruments: Ref<Instrument[]>
  showNewInstrument: Ref<boolean>
  newInstrumentSymbol: Ref<string>
  newInstrumentName: Ref<string>
  newInstrumentType: Ref<InstrumentType>
  transactions: Ref<Transaction[]>
  accountOptions: ComputedRef<{ label: string; value: string }[]>
  investmentAccountOptions: ComputedRef<{ label: string; value: string }[]>
  instrumentOptions: ComputedRef<{ label: string; value: string }[]>
  currencyOptions: ComputedRef<{ label: string; value: string }[]>
  treeOptions: ComputedRef<CategoryTreeNode[]>
  expenseTransactions: ComputedRef<Transaction[]>
  refundTargetOptions: ComputedRef<{ label: string; value: string }[]>
  refundTarget: ComputedRef<Transaction | null>
  isInvestmentTransaction: ComputedRef<boolean>
  investmentAmount: ComputedRef<number>
  submit: () => Promise<void>
  createNewInstrument: () => Promise<void>
  loadInstruments: () => Promise<void>
  loadTransactions: () => Promise<void>
  resetForm: () => void
}

export function useTransactionForm(options?: { onCreated?: () => void }): TransactionFormContext {
  const store = useAppStore()
  const message = useMessage()

  const kind = ref<TransactionKind>('expense')
  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const toAccountId = ref<string | null>(null)
  const categoryId = ref<string | null>(null)
  const refundTargetId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  const instrumentId = ref<string | null>(null)
  const quantity = ref<number | null>(null)
  const price = ref<number | null>(null)
  const fee = ref<number | null>(null)
  const instruments = ref<Instrument[]>([])
  const showNewInstrument = ref(false)
  const newInstrumentSymbol = ref('')
  const newInstrumentName = ref('')
  const newInstrumentType = ref<InstrumentType>('stock')

  const transactions = ref<Transaction[]>([])

  const accountOptions = computed(() =>
    store.accounts.map((a) => ({ label: a.name, value: a.id })),
  )
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
  const currencyOptions = computed(() =>
    store.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
  )
  const treeOptions = computed(() => {
    if (kind.value === 'expense') return store.treeCategoryOptions('expense')
    if (kind.value === 'income') return store.treeCategoryOptions('income')
    return []
  })

  const expenseTransactions = computed(() =>
    transactions.value.filter((t) => t.kind === 'expense'),
  )
  const refundTargetOptions = computed(() =>
    expenseTransactions.value.map((t) => {
      const cat = store.categoryPath(t.category_id) || '-'
      const cur = store.getCurrency(t.currency_code)
      const amt = formatAmount(t.amount_native_cents, cur)
      const noteStr = t.note ? ` · ${t.note}` : ''
      return {
        label: `${t.date}  ${amt}  ${cat}${noteStr}`,
        value: t.id,
      }
    }),
  )
  const refundTarget = computed(() =>
    refundTargetId.value == null
      ? null
      : (expenseTransactions.value.find((t) => t.id === refundTargetId.value) ?? null),
  )

  const isInvestmentTransaction = computed(() => kind.value === 'buy' || kind.value === 'sell')

  const investmentAmount = computed(() => {
    if (quantity.value == null || price.value == null) return 0
    const feeValue = fee.value ?? 0
    const raw = kind.value === 'buy'
      ? quantity.value * price.value + feeValue
      : quantity.value * price.value - feeValue
    return Math.round(raw * 100) / 100
  })

  watch(kind, () => {
    categoryId.value = null
    toAccountId.value = null
    refundTargetId.value = null
    instrumentId.value = null
    quantity.value = null
    price.value = null
    fee.value = null
    showNewInstrument.value = false
    if (kind.value === 'refund') accountId.value = null
  })

  watch(accountId, () => {
    if (isInvestmentTransaction.value && accountId.value) {
      const account = store.accountMap.get(accountId.value)
      if (account) currencyCode.value = account.currency_code
    }
  })

  watch(refundTargetId, () => {
    if (refundTarget.value) {
      accountId.value = refundTarget.value.account_id
      currencyCode.value = refundTarget.value.currency_code
    }
  })

  function resetForm() {
    amount.value = null
    note.value = ''
    kind.value = 'expense'
    currencyCode.value = 'CNY'
    accountId.value = null
    toAccountId.value = null
    categoryId.value = null
    refundTargetId.value = null
    instrumentId.value = null
    quantity.value = null
    price.value = null
    fee.value = null
    date.value = Date.now()
    showNewInstrument.value = false
  }

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

  async function loadTransactions() {
    try {
      transactions.value = await api.listTransactions()
    } catch {
      // 加载失败忽略，退款关联可后续重试
    }
  }

  async function submit() {
    if (kind.value === 'refund') {
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
      return
    }

    if (kind.value === 'buy') {
      if (!accountId.value) {
        message.warning('请选择投资账户')
        return
      }
      if (!instrumentId.value) {
        message.warning('请选择标的')
        return
      }
      if (quantity.value == null || quantity.value <= 0) {
        message.warning('请输入买入数量')
        return
      }
      if (price.value == null || price.value <= 0) {
        message.warning('请输入买入单价')
        return
      }
      const input: TransactionInput = {
        kind: 'buy',
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
        message.success('已记买入')
        instrumentId.value = null
        quantity.value = null
        price.value = null
        fee.value = null
        note.value = ''
        options?.onCreated?.()
      } catch (e) {
        message.error(`买入失败: ${e}`)
      }
      return
    }

    if (kind.value === 'sell') {
      if (!accountId.value) {
        message.warning('请选择投资账户')
        return
      }
      if (!instrumentId.value) {
        message.warning('请选择标的')
        return
      }
      if (quantity.value == null || quantity.value <= 0) {
        message.warning('请输入卖出数量')
        return
      }
      if (price.value == null || price.value <= 0) {
        message.warning('请输入卖出单价')
        return
      }
      const input: TransactionInput = {
        kind: 'sell',
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
        message.success('已记卖出')
        instrumentId.value = null
        quantity.value = null
        price.value = null
        fee.value = null
        note.value = ''
        options?.onCreated?.()
      } catch (e) {
        message.error(`卖出失败: ${e}`)
      }
      return
    }

    if (!accountId.value) {
      message.warning('请选择账户')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入金额')
      return
    }
    if (kind.value === 'transfer' && !toAccountId.value) {
      message.warning('转账需选择目标账户')
      return
    }
    const input: TransactionInput = {
      kind: kind.value,
      amount_cents: Math.round(amount.value * 100),
      currency_code: currencyCode.value,
      account_id: accountId.value,
      to_account_id: kind.value === 'transfer' ? toAccountId.value : null,
      category_id: kind.value === 'transfer' ? null : categoryId.value,
      refund_of_transaction_id: null,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
    }
    try {
      await api.createTransaction(input)
      message.success('已记账')
      amount.value = null
      note.value = ''
      options?.onCreated?.()
    } catch (e) {
      message.error(`记账失败: ${e}`)
    }
  }

  onMounted(async () => {
    await store.loadAll()
    await Promise.all([loadTransactions(), loadInstruments()])
  })

  return {
    kind,
    amount,
    currencyCode,
    accountId,
    toAccountId,
    categoryId,
    refundTargetId,
    note,
    date,
    instrumentId,
    quantity,
    price,
    fee,
    instruments,
    showNewInstrument,
    newInstrumentSymbol,
    newInstrumentName,
    newInstrumentType,
    transactions,
    accountOptions,
    investmentAccountOptions,
    instrumentOptions,
    currencyOptions,
    treeOptions,
    expenseTransactions,
    refundTargetOptions,
    refundTarget,
    isInvestmentTransaction,
    investmentAmount,
    submit,
    createNewInstrument,
    loadInstruments,
    loadTransactions,
    resetForm,
  }
}
