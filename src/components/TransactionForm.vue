<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NDatePicker,
  NButton,
  NSpace,
  NRadioGroup,
  NRadio,
  useMessage,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { TransactionInput, TransactionKind } from '@/types'

const props = defineProps<{ onCreated?: () => void }>()
const emit = defineEmits<{ created: [] }>()

const store = useAppStore()
const message = useMessage()

const kind = ref<TransactionKind>('expense')
const amount = ref<number | null>(null)
const currencyCode = ref('CNY')
const accountId = ref<number | null>(null)
const toAccountId = ref<number | null>(null)
const categoryId = ref<number | null>(null)
const note = ref('')
const date = ref(Date.now())

const accountOptions = computed(() =>
  store.accounts.map((a) => ({ label: a.name, value: a.id })),
)
const currencyOptions = computed(() =>
  store.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code })),
)
const categoryOptions = computed(() => {
  const list = kind.value === 'income' ? store.incomeCategories : store.expenseCategories
  return list.map((c) => ({ label: c.name, value: c.id }))
})

watch(kind, () => {
  categoryId.value = null
  toAccountId.value = null
})

async function submit() {
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
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NRadioGroup v-model:value="kind">
        <NRadio value="expense">支出</NRadio>
        <NRadio value="income">收入</NRadio>
        <NRadio value="transfer">转账</NRadio>
      </NRadioGroup>

      <NFormItem label="金额">
        <NInputNumber
          v-model:value="amount"
          :min="0"
          :precision="2"
          placeholder="金额"
          style="width: 160px"
        />
        <NSelect
          v-model:value="currencyCode"
          :options="currencyOptions"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="账户">
        <NSelect
          v-model:value="accountId"
          :options="accountOptions"
          placeholder="选择账户"
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

      <NFormItem v-if="kind !== 'transfer'" label="分类">
        <NSelect
          v-model:value="categoryId"
          :options="categoryOptions"
          placeholder="选择分类"
          style="width: 200px"
          clearable
        />
      </NFormItem>

      <NFormItem label="日期">
        <NDatePicker v-model:value="date" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="note" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="submit">记一笔</NButton>
    </NSpace>
  </NForm>
</template>
