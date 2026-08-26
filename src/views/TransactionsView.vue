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
import { useReferenceStore } from '@/stores/reference'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transactionColumns'
import type { Transaction } from '@/types'

const reference = useReferenceStore()
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
  ...buildTransactionColumns(reference),
  {
    title: '操作',
    key: 'actions',
    width: 80,
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

// scroll-x：列中所有固定列（有 width 的列，备注为弹性列不计入）宽度总和
const scrollX = sumFixedColumnWidths(columns)

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpace vertical :size="12">
    <!-- 备注列为弹性列（transactionColumns 中不设 width），表格始终铺满容器；
         窄窗口时备注先收缩，scroll-x（固定列宽总和）作为横向滚动下限 -->
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
