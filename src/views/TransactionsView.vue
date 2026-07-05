<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NDataTable,
  NButton,
  NSpace,
  NPopconfirm,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount, TRANSACTION_KIND_LABELS } from '@/types'
import type { Transaction } from '@/types'

const store = useAppStore()
const message = useMessage()
const data = ref<Transaction[]>([])
const loading = ref(false)

async function refresh() {
  loading.value = true
  try {
    data.value = await api.listTransactions()
  } catch (e) {
    message.error(`加载失败: ${e}`)
  } finally {
    loading.value = false
  }
}

async function remove(id: number) {
  try {
    await api.deleteTransaction(id)
    message.success('已删除')
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

const columns: DataTableColumns<Transaction> = [
  { title: '日期', key: 'date', width: 120 },
  {
    title: '类型',
    key: 'kind',
    width: 80,
    render: (row) => TRANSACTION_KIND_LABELS[row.kind as keyof typeof TRANSACTION_KIND_LABELS],
  },
  {
    title: '分类',
    key: 'category_id',
    render: (row) =>
      row.category_id ? store.categoryMap.get(row.category_id)?.name ?? '-' : '-',
  },
  {
    title: '账户',
    key: 'account_id',
    render: (row) => store.accountMap.get(row.account_id)?.name ?? '-',
  },
  { title: '备注', key: 'note', render: (row) => row.note ?? '-' },
  {
    title: '金额',
    key: 'amount_native_cents',
    width: 140,
    render: (row) =>
      h(
        'span',
        {
          style:
            row.kind === 'income'
              ? 'color: #18a058'
              : row.kind === 'expense'
                ? 'color: #d03050'
                : '',
        },
        formatAmount(row.amount_native_cents, store.getCurrency(row.currency_code)),
      ),
  },
  {
    title: '操作',
    key: 'actions',
    width: 100,
    render: (row) =>
      h(
        NPopconfirm,
        { onPositiveClick: () => remove(row.id) },
        {
          default: () => '确认删除？',
          trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        },
      ),
  },
]

onMounted(async () => {
  await store.loadAll()
  await refresh()
})
</script>

<template>
  <NSpace vertical :size="12">
    <NDataTable
      :columns="columns"
      :data="data"
      :loading="loading"
      :bordered="false"
      size="small"
      :pagination="{ pageSize: 20 }"
    />
  </NSpace>
</template>
