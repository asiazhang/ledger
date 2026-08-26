<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSpace,
  NPopconfirm,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import AccountLink from '@/components/AccountLink.vue'
import { ACCOUNT_TYPE_LABELS, formatAmount } from '@/types'
import type { AccountBalance, AccountInput, AccountType } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const balances = ref<AccountBalance[]>([])

const name = ref('')
const type = ref<AccountType>('cash')
const currencyCode = ref('CNY')
const initial = ref<number | null>(0)

const typeOptions = (Object.keys(ACCOUNT_TYPE_LABELS) as AccountType[]).map((k) => ({
  label: ACCOUNT_TYPE_LABELS[k],
  value: k,
}))
const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }))

async function refresh() {
  balances.value = await api.listAccountBalances()
}

async function create() {
  if (!name.value.trim()) {
    message.warning('请输入账户名称')
    return
  }
  const input: AccountInput = {
    name: name.value,
    type: type.value,
    currency_code: currencyCode.value,
    initial_balance_cents: Math.round((initial.value ?? 0) * 100),
  }
  try {
    await api.createAccount(input)
    message.success('已创建账户')
    name.value = ''
    initial.value = 0
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(`创建失败: ${e}`)
  }
}

async function remove(id: string) {
  try {
    await api.deleteAccount(id)
    message.success('已删除')
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

const columns: DataTableColumns<AccountBalance> = [
  {
    title: '名称',
    key: 'account.name',
    // 账户名下钻：点击跳转交易页并按涉及账户过滤（issue #97）
    render: (row) => h(AccountLink, { accountId: row.account.id }),
  },
  {
    title: '类型',
    key: 'account.type',
    render: (row) => ACCOUNT_TYPE_LABELS[row.account.type],
  },
  { title: '币种', key: 'account.currency_code' },
  {
    title: '余额',
    key: 'balance_cents',
    render: (row) => formatAmount(row.balance_cents, reference.getCurrency(row.account.currency_code)),
  },
  {
    title: '操作',
    key: 'actions',
    width: 100,
    render: (row) =>
      h(
        NPopconfirm,
        { onPositiveClick: () => remove(row.account.id) },
        {
          default: () => '删除账户将影响相关交易，确认？',
          trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        },
      ),
  },
]

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增账户" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="账户名称" style="width: 160px" />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect v-model:value="type" :options="typeOptions" style="width: 120px" />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect v-model:value="currencyCode" :options="currencyOptions()" style="width: 140px" />
        </NFormItem>
        <NFormItem label="初始余额">
          <NInputNumber v-model:value="initial" :precision="2" style="width: 140px" />
        </NFormItem>
        <NButton type="primary" @click="create">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="账户列表" size="small">
      <NDataTable :columns="columns" :data="balances" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
