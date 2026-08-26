<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
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
import type { Transaction, TransactionListFilter } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const route = useRoute()
const data = ref<Transaction[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(20)
const loading = ref(false)

/** 涉及账户过滤（账户名下钻，issue #97）。
 * 组件状态是过滤的唯一事实源：当前仅由 URL `?account=<id>` 初始化（只读入口），
 * 用户手动改动不同步回 URL；无效参数（账户不存在/已删除）回退 null（全量）；
 * 不带参数进入同样复位为 null，回到全量列表。 */
const involvingAccountId = ref<string | null>(null)

/** 首次求值不触发刷新：immediate 与 onMounted 间不双刷（onMounted 统一承担首次加载）。 */
let firstApply = true

/** 解析 URL 账户参数 → 过滤状态；过滤条件实际变化时才刷新并回到第 1 页。
 * 校验依赖参考数据（accountMap）：冷启动直连深链时参考数据可能晚到，
 * 待其就绪（status → ready）后自动补判一次，避免有效参数被误判为无效而静默丢失。 */
function applyAccountParam() {
  const param = typeof route.query.account === 'string' ? route.query.account : null
  const next =
    param !== null && reference.status === 'ready' && reference.accountMap.has(param)
      ? param
      : null
  if (next !== involvingAccountId.value) {
    involvingAccountId.value = next
    // 过滤条件变化 → 回到第 1 页
    page.value = 1
    if (!firstApply) void refresh()
  }
  firstApply = false
}

// URL query 只读入口：/transactions?account=<id> 进入时自动按该账户过滤；
// query 变化（含导航清除 query）时同步复位。
watch(() => route.query.account, applyAccountParam, { immediate: true })

// 参考数据就绪后重判：冷启动直连深链时初始校验可能因数据未到而回退全量，
// 数据到达后自动补判（仅当过滤条件实际变化才触发刷新，避免无关重刷）。
watch(
  () => reference.status,
  (status) => {
    if (status === 'ready') applyAccountParam()
  },
)

/** 页大小选项（不持久化，遵守 ViewState 决策） */
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100]

async function refresh() {
  loading.value = true
  try {
    const filter: TransactionListFilter = {
      page: page.value,
      page_size: pageSize.value,
    }
    if (involvingAccountId.value) {
      filter.involving_account_id = involvingAccountId.value
    }
    const res = await api.listTransactions(filter)
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
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll；
  // 首次加载（过滤条件已由 applyAccountParam 的 immediate 求值确定）
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
