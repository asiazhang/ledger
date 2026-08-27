<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  NDataTable,
  NButton,
  NDatePicker,
  NEmpty,
  NSelect,
  NSpace,
  NPopconfirm,
  NModal,
  useMessage,
  type DataTableColumn,
  type PaginationProps,
} from 'naive-ui'
import TransactionForm from '@/components/TransactionForm.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transaction-columns'
import {
  TRANSACTION_KIND_LABELS,
  type Transaction,
  type TransactionKind,
  type TransactionListFilter,
} from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const route = useRoute()
const data = ref<Transaction[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(20)
const loading = ref(false)

/** 涉及账户过滤（账户名下钻，issue #97；手动下拉，issue #98）。
 * 组件状态是过滤的唯一事实源：URL `?account=<id>` 仅为只读初始化入口，
 * 用户手动改动不同步回 URL；无效参数（账户不存在/已删除）回退 null（全量）；
 * 不带参数进入同样复位为 null，回到全量列表。 */
const involvingAccountId = ref<string | null>(null)

/** 日期起止过滤（YYYY-MM-DD，与后端 date 字典序一致，含边界）。 */
const dateFrom = ref<string | null>(null)
const dateTo = ref<string | null>(null)

/** 交易类型过滤（income / expense / transfer / refund / buy / sell）。 */
const kind = ref<TransactionKind | null>(null)

/** 是否有任一激活的过滤条件（控制清除按钮可用性与空态文案）。 */
const filtersActive = computed(
  () =>
    involvingAccountId.value !== null ||
    dateFrom.value !== null ||
    dateTo.value !== null ||
    kind.value !== null,
)

/** 账户下拉选项：来自参考数据账户映射（list_accounts 不含 is_hidden 黑洞账户，沿用既有边界）。 */
const accountOptions = computed(() =>
  reference.accounts.map((a) => ({ label: a.name, value: a.id })),
)

/** 类型下拉选项：前端 TransactionKind 的 6 种（income/expense/transfer/refund/buy/sell；
 * Rust 侧另有 dividend/split 未在前端类型暴露，不进过滤选项）。 */
const kindOptions: Array<{ label: string; value: TransactionKind }> = (
  Object.entries(TRANSACTION_KIND_LABELS) as [TransactionKind, string][]
).map(([value, label]) => ({ label, value }))

/** 首次求值不触发刷新：immediate 与 onMounted 间不双刷（onMounted 统一承担首次加载）。 */
let firstApply = true

/** URL 账户参数是否已在参考数据就绪下完成校验。
 * 冷启动深链时参考数据晚到，首次就绪补判一次后即视为已结算：
 * 后续 ledger:changed 重拉（status 再次 loading→ready）不再重放 URL 参数，
 * 避免把用户手动改动（如清除筛选）覆盖回 URL 值。 */
let urlAccountSettled = false

/** 过滤条件变化统一出口：回到第 1 页并重新查询。
 * 首次求值（URL 初始化）由 firstApply 标记跳过，避免与 onMounted 双刷。 */
function applyFilterChange() {
  if (firstApply) {
    firstApply = false
    return
  }
  page.value = 1
  void refresh()
}

/** 涉及账户过滤 setter（URL 入口与手动下拉共用）：条件实际变化才触发刷新。 */
function setInvolvingAccount(id: string | null) {
  if (id === involvingAccountId.value) return
  involvingAccountId.value = id
  applyFilterChange()
}

/** 解析 URL 账户参数 → 过滤状态。
 * 校验依赖参考数据（accountMap）：冷启动直连深链时参考数据可能晚到，
 * 待其就绪（status → ready）后自动补判一次，避免有效参数被误判为无效而静默丢失。 */
function applyAccountParam() {
  const param = typeof route.query.account === 'string' ? route.query.account : null
  const ready = reference.status === 'ready'
  const next = param !== null && ready && reference.accountMap.has(param) ? param : null
  // 无有效账户参数进入（侧边栏重进 / 无效参数）→ 复位日期/类型，回到全量列表（#96 决策 3）。
  // 先复位再走统一出口：setInvolvingAccount 触发刷新时读到的是复位后的过滤状态。
  const resetDateKind =
    next === null &&
    (dateFrom.value !== null || dateTo.value !== null || kind.value !== null)
  if (resetDateKind) {
    dateFrom.value = null
    dateTo.value = null
    kind.value = null
  }
  const accountChanged = next !== involvingAccountId.value
  setInvolvingAccount(next)
  // 账户未变但日期/类型被复位 → setInvolvingAccount 未触发刷新，补一次（避免列表陈旧）
  if (resetDateKind && !accountChanged) {
    page.value = 1
    if (!firstApply) void refresh()
  }
  // 参考数据就绪时本次校验即结算；未就绪保持未结算，待就绪后补判一次
  if (ready) urlAccountSettled = true
  // 无论是否变化都消耗首次标记：immediate 求值承担首载前的一次初始化
  firstApply = false
}

// URL query 只读入口：/transactions?account=<id> 进入时自动按该账户过滤；
// query 变化（含导航清除 query）时同步复位。
watch(() => route.query.account, applyAccountParam, { immediate: true })

// 参考数据就绪后重判：冷启动直连深链时初始校验可能因数据未到而回退全量，
// 数据到达后自动补判一次（仅当 URL 参数尚未结算；结算后不再重放，见 urlAccountSettled）。
watch(
  () => reference.status,
  (status) => {
    if (status === 'ready' && !urlAccountSettled) applyAccountParam()
  },
)

/** 页大小选项（不持久化，遵守 ViewState 决策） */
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100]

/** 「记一笔」弹窗开关：弹窗内嵌现有 TransactionForm，
 * 提交成功（created）后关闭并立即刷新列表，录完马上能看到记录（issue #141）。 */
const showCreate = ref(false)

/** 提交成功：回到第 1 页再刷新（新记录按日期/时间排序最可能落在第 1 页），
 * 保留筛选条件（与手动过滤同等语义，不重置）。 */
function onFormCreated() {
  showCreate.value = false
  page.value = 1
  void refresh()
}

async function refresh() {
  loading.value = true
  try {
    const filter: TransactionListFilter = {
      page: page.value,
      page_size: pageSize.value,
    }
    // 过滤参数按需携带（空值省略，与后端可选字段语义一致）
    if (dateFrom.value) filter.from = dateFrom.value
    if (dateTo.value) filter.to = dateTo.value
    if (involvingAccountId.value) filter.involving_account_id = involvingAccountId.value
    if (kind.value) filter.kind = kind.value
    const res = await api.listTransactions(filter)
    data.value = res.items
    total.value = res.total
  } catch (e) {
    message.error(`加载失败: ${e}`)
  } finally {
    loading.value = false
  }
}

/** 手动过滤处理器：任一条件变化即重新查询（回到第 1 页），不同步回 URL。 */
function onAccountFilterChange(id: string | null) {
  setInvolvingAccount(id)
}

function onDateFromChange(value: string | null) {
  if (value === dateFrom.value) return
  dateFrom.value = value
  applyFilterChange()
}

function onDateToChange(value: string | null) {
  if (value === dateTo.value) return
  dateTo.value = value
  applyFilterChange()
}

function onKindFilterChange(value: TransactionKind | null) {
  if (value === kind.value) return
  kind.value = value
  applyFilterChange()
}

/** 一键清除全部过滤条件并回到全量列表（第 1 页）。 */
function clearFilters() {
  if (!filtersActive.value) return
  dateFrom.value = null
  dateTo.value = null
  kind.value = null
  involvingAccountId.value = null
  page.value = 1
  void refresh()
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
    <!-- 过滤行：账户（涉及账户语义，可清除）+ 日期起止 + 类型（可清除）+ 清除筛选。
         任一条件变化即重新查询并回到第 1 页；手动改动不同步回 URL
         （组件状态是唯一事实源，issue #96 决策 3/4），分页/页大小切换保持过滤条件。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <NSelect
        :value="involvingAccountId"
        :options="accountOptions"
        placeholder="账户"
        clearable
        filterable
        style="width: 160px"
        @update:value="onAccountFilterChange"
      />
      <NDatePicker
        :formatted-value="dateFrom"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="起始日期"
        clearable
        style="width: 140px"
        @update:formatted-value="onDateFromChange"
      />
      <NDatePicker
        :formatted-value="dateTo"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="结束日期"
        clearable
        style="width: 140px"
        @update:formatted-value="onDateToChange"
      />
      <NSelect
        :value="kind"
        :options="kindOptions"
        placeholder="类型"
        clearable
        style="width: 120px"
        @update:value="onKindFilterChange"
      />
      <NButton
        size="tiny"
        quaternary
        type="primary"
        :disabled="!filtersActive"
        @click="clearFilters"
      >
        清除筛选
      </NButton>
      <NButton type="primary" @click="showCreate = true">记一笔</NButton>
    </NSpace>
    <!-- 快速记账弹窗：内嵌现有交易表单，提交成功关闭并刷新列表 -->
    <NModal
      v-model:show="showCreate"
      title="记一笔"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <TransactionForm @created="onFormCreated" />
    </NModal>
    <!-- 备注列为弹性列（transaction-columns 中不设 width），表格始终铺满容器；
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
    >
      <!-- 空态：过滤无结果时展示明确提示（与加载态区分：loading 时空态节点隐藏）；
           无过滤时为默认「暂无数据」文案 -->
      <template #empty>
        <NEmpty :description="filtersActive ? '没有符合条件的交易' : '暂无数据'" size="small">
          <template v-if="filtersActive" #extra>
            <NButton size="tiny" quaternary type="primary" @click="clearFilters">
              清除筛选
            </NButton>
          </template>
        </NEmpty>
      </template>
    </NDataTable>
  </NSpace>
</template>
