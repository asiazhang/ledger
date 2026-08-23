<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
import {
  NDataTable,
  NButton,
  NSpace,
  NPopconfirm,
  useMessage,
  type DataTableColumn,
  type PaginationProps,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { buildTransactionColumns } from '@/components/transactionColumns'
import type { Transaction } from '@/types'

const store = useAppStore()
const message = useMessage()
const data = ref<Transaction[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(20)
const loading = ref(false)

/** 页大小选项（不持久化，遵守 ViewState 决策） */
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100]

async function refresh() {
  loading.value = true
  try {
    const res = await api.listTransactions({
      page: page.value,
      page_size: pageSize.value,
    })
    data.value = res.items
    total.value = res.total
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
    // 删除当前页最后一条 → 回退一页，避免出现空页
    if (data.value.length === 1 && page.value > 1) {
      page.value -= 1
    }
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

const pagination = computed<PaginationProps>(() => ({
  page: page.value,
  pageSize: pageSize.value,
  itemCount: total.value,
  showSizePicker: true,
  showQuickJumper: true,
  pageSizes: PAGE_SIZE_OPTIONS,
  prefix: ({ itemCount }) => h('span', null, () => `共 ${itemCount ?? 0} 条`),
  onChange: (p: number) => {
    page.value = p
    refresh()
  },
  onUpdatePageSize: (size: number) => {
    pageSize.value = size
    page.value = 1
    refresh()
  },
}))

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

// 列宽总和：fixed 布局下设置 scroll-x 可阻止列被自动拉伸填满容器
const scrollX = columns.reduce((sum, c) => sum + (typeof c.width === 'number' ? c.width : 0), 0)

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
      remote
      :scroll-x="scrollX"
      :pagination="pagination"
    />
  </NSpace>
</template>

<style scoped>
/* fixed 布局 + width:100% 会把列拉伸填满容器；覆盖为 auto 让列严格按指定 width，右侧留白 */
:deep(.n-data-table-table) {
  width: auto;
}
</style>
