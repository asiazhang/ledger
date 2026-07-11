<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NTreeSelect,
  NDatePicker,
  NButton,
  NSpace,
  NRadioGroup,
  NRadio,
  NText,
  useMessage,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount, INSTRUMENT_TYPE_LABELS } from '@/types'
import type { Instrument, InstrumentType, Transaction, TransactionInput, TransactionKind } from '@/types'

const props = defineProps<{ onCreated?: () => void }>()
const emit = defineEmits<{ created: [] }>()

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

// 投资交易字段
const instrumentId = ref<string | null>(null)
const quantity = ref<number | null>(null)
const price = ref<number | null>(null)
const fee = ref<number | null>(null)
const instruments = ref<Instrument[]>([])
const showNewInstrument = ref(false)
const newInstrumentSymbol = ref('')
const newInstrumentName = ref('')
const newInstrumentType = ref<InstrumentType>('stock')

// 交易列表（供退款关联选择原支出交易）
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
  // 退款时账户由关联决定，清空用户先前选的账户
  if (kind.value === 'refund') accountId.value = null
})

// 买入/卖出时账户切换，自动同步币种为账户本位币
watch(accountId, () => {
  if (isInvestmentTransaction.value && accountId.value) {
    const account = store.accountMap.get(accountId.value)
    if (account) currencyCode.value = account.currency_code
  }
})

// 选定退款关联后，锁定账户与币种为原交易
watch(refundTargetId, () => {
  if (refundTarget.value) {
    accountId.value = refundTarget.value.account_id
    currencyCode.value = refundTarget.value.currency_code
  }
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
      emit('created')
      props.onCreated?.()
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
      emit('created')
      props.onCreated?.()
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
      emit('created')
      props.onCreated?.()
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
    emit('created')
    props.onCreated?.()
  } catch (e) {
    message.error(`记账失败: ${e}`)
  }
}

async function loadTransactions() {
  try {
    transactions.value = await api.listTransactions()
  } catch {
    // 加载失败忽略，退款关联可后续重试
  }
}

onMounted(async () => {
  await store.loadAll()
  await Promise.all([loadTransactions(), loadInstruments()])
})
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NRadioGroup v-model:value="kind">
        <NRadio value="expense">支出</NRadio>
        <NRadio value="income">收入</NRadio>
        <NRadio value="transfer">转账</NRadio>
        <NRadio value="refund">退款</NRadio>
        <NRadio value="buy">买入</NRadio>
        <NRadio value="sell">卖出</NRadio>
      </NRadioGroup>

      <NFormItem v-if="!isInvestmentTransaction" label="金额">
        <NInputNumber
          v-model:value="amount"
          :min="0"
          :precision="2"
          placeholder="金额"
          style="width: 160px"
        />
        <NSelect
          v-model:value="currencyCode"
          :disabled="kind === 'refund'"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem v-if="isInvestmentTransaction" label="金额">
        <NInputNumber
          :value="investmentAmount"
          :disabled="true"
          :precision="2"
          placeholder="自动计算"
          style="width: 160px"
        />
        <NSelect
          v-model:value="currencyCode"
          :options="currencyOptions"
          :disabled="true"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem v-if="kind !== 'refund' && !isInvestmentTransaction" label="账户">
        <NSelect
          v-model:value="accountId"
          :options="accountOptions"
          placeholder="选择账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem v-if="isInvestmentTransaction" label="投资账户">
        <NSelect
          v-model:value="accountId"
          :options="investmentAccountOptions"
          placeholder="选择投资账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem v-if="kind === 'transfer'" label="转入">
        <NSelect
          v-model:value="toAccountId"
          :options="accountOptions"
          placeholder="目标账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem v-if="isInvestmentTransaction" label="标的">
        <NSpace align="center" :size="8">
          <NSelect
            v-model:value="instrumentId"
            :options="instrumentOptions"
            placeholder="选择标的"
            filterable
            clearable
            style="width: 200px"
          />
          <NButton size="tiny" @click="showNewInstrument = !showNewInstrument">
            {{ showNewInstrument ? '取消' : '新增标的' }}
          </NButton>
        </NSpace>
      </NFormItem>

      <NSpace v-if="isInvestmentTransaction && showNewInstrument" vertical :size="8">
        <NFormItem label="代码">
          <NInput
            v-model:value="newInstrumentSymbol"
            placeholder="如 NVDA"
            style="width: 120px"
          />
        </NFormItem>
        <NFormItem label="名称">
          <NInput
            v-model:value="newInstrumentName"
            placeholder="名称（可选）"
            style="width: 180px"
          />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect
            v-model:value="newInstrumentType"
            :options="Object.entries(INSTRUMENT_TYPE_LABELS).map(([value, label]) => ({ label, value }))"
            style="width: 120px"
          />
        </NFormItem>
        <NButton size="small" @click="createNewInstrument">
          保存标的
        </NButton>
      </NSpace>

      <NFormItem v-if="isInvestmentTransaction" label="数量">
        <NInputNumber
          v-model:value="quantity"
          :min="0"
          :precision="4"
          placeholder="数量"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem v-if="isInvestmentTransaction" label="单价">
        <NInputNumber
          v-model:value="price"
          :min="0"
          :precision="2"
          placeholder="单价"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem v-if="isInvestmentTransaction" label="手续费">
        <NInputNumber
          v-model:value="fee"
          :min="0"
          :precision="2"
          placeholder="手续费"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem v-if="kind === 'expense' || kind === 'income'" label="分类">
        <NTreeSelect
          v-model:value="categoryId"
          :options="treeOptions"
          filterable
          clearable
          placeholder="选择分类"
          :consistent-menu-width="false"
          style="width: 220px"
        />
      </NFormItem>

      <NFormItem v-if="kind === 'refund'" label="退款关联">
        <NSelect
          v-model:value="refundTargetId"
          :options="refundTargetOptions"
          filterable
          placeholder="选择原支出交易"
          style="width: 340px"
        />
      </NFormItem>

      <NFormItem v-if="kind === 'refund' && refundTarget" label="原交易">
        <NText depth="3" style="font-size: 12px">
          {{ formatAmount(refundTarget.amount_native_cents, store.getCurrency(refundTarget.currency_code)) }}
          · {{ store.categoryPath(refundTarget.category_id) || '-' }}
          · {{ store.accountMap.get(refundTarget.account_id)?.name ?? '-' }}
        </NText>
      </NFormItem>

      <NFormItem label="日期">
        <NDatePicker v-model:value="date" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="note" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="submit">
        {{ kind === 'refund' ? '记退款' : '记一笔' }}
      </NButton>
    </NSpace>
  </NForm>
</template>
