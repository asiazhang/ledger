<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NDataTable,
  NButton,
  NSpace,
  NPopconfirm,
  useMessage,
  type DataTableColumn,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { buildTransactionColumns } from '@/components/transactionColumns'
import type { Transaction } from '@/types'

const store = useAppStore()
const message = useMessage()
const data = ref<Transaction[]>([])
const loading = ref(false)

async function refresh() {
  loading.value = true
  try {
    data.value = (await api.listTransactions()).items
  } catch (e) {
    message.error(`加载失败: ${e}`)
  } finally {
    loading.value = false
  }
}

async function remove(id: string) {
  try {
    await api.deleteTransaction(id)
    message.success('已删除')
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

const columns: DataTableColumn<Transaction>[] = [
  ...buildTransactionColumns(store),
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
