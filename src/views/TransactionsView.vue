<script setup lang="ts">
import { computed, h, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  NDataTable,
  NButton,
  NButtonGroup,
  NDropdown,
  NIcon,
  NDatePicker,
  NEmpty,
  NSelect,
  NSpace,
  NModal,
  useDialog,
  useMessage,
  useThemeVars,
  type DataTableColumn,
  type DropdownOption,
  type PaginationProps,
} from 'naive-ui'
import { ChevronDown } from '@vicons/ionicons5'
import TransactionForm from '@/components/TransactionForm.vue'
import RefundForm from '@/components/RefundForm.vue'
import AddItemForm from '@/components/AddItemForm.vue'
import { buildRowMenuOptions } from '@/components/transaction-row-menu'
import { useCreateShortcuts, CREATE_KIND_KEYS } from '@/composables/useCreateShortcuts'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useItemsStore } from '@/stores/items'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transaction-columns'
import {
  CREATE_KINDS,
  TRANSACTION_KIND_LABELS,
  type CreateTransactionKind,
  type Transaction,
  type TransactionKind,
  type TransactionListFilter,
} from '@/types'

const reference = useReferenceStore()
// 物品 store（issue #119）：仅用于右键菜单「加入物品」的置灰态判断；
// self-init + ledger:changed 自动重拉，创建成功后菜单下次打开即为置灰态。
const itemsStore = useItemsStore()
const message = useMessage()
const dialog = useDialog()
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

/** 「记一笔」分裂按钮（issue #150）：主体点击直接以 expense 打开弹窗（最高频路径一步直达），
 * 右侧箭头展开 5 项菜单（不含退款）。createKind 为 null 表示弹窗关闭；
 * 类型由入口单点表达，弹窗内不再提供切换，中途换类型 = 关闭重开。 */
const createKind = ref<CreateTransactionKind | null>(null)

/** 下拉选项：5 种可创建类型（refund 不在入口：退款已移出表单域，入口由交易条目
 * 右键菜单承接，独立 ticket 落地前处于过渡态）。标签后附裸键快捷键提示（issue #153），
 * 键位来自 CREATE_KIND_KEYS 单一来源，与 keydown 匹配共用。 */
const createKindOptions: DropdownOption[] = CREATE_KINDS.map((k) => ({
  label: `${TRANSACTION_KIND_LABELS[k]} ${CREATE_KIND_KEYS[k]}`,
  key: k,
}))

const createTitle = computed(() =>
  createKind.value ? `记一笔 · ${TRANSACTION_KIND_LABELS[createKind.value]}` : '记一笔',
)

function openCreate(k: CreateTransactionKind) {
  createKind.value = k
}

function onCreateShowUpdate(show: boolean) {
  if (!show) createKind.value = null
}

// 裸键快捷键（issue #153）：a/z/i/b/s 直达对应类型弹窗，与点下拉对应项同一入口；
// 焦点在可编辑元素或弹层打开时抑制；随视图装卸，仅交易页生效
useCreateShortcuts(openCreate)

/** 提交成功：回到第 1 页再刷新（新记录按日期/时间排序最可能落在第 1 页），
 * 保留筛选条件（与手动过滤同等语义，不重置）。 */
function onFormCreated() {
  createKind.value = null
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

/** 删除走 useDialog 二次确认（issue #151）：取消不删，确认后才删除。 */
function confirmDelete(row: Transaction) {
  dialog.warning({
    title: '删除交易',
    content: '确认删除该条交易？删除后不可恢复。',
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: () => remove(row.id),
  })
}

/** 行内退款弹窗（issue #151）：原交易由右键所在行确定，不经搜索选择；
 * 同一支出可多次发起退款（部分退款语义，不阻断）。
 * refundSeq 作为表单 key：每次打开强制重建表单实例，
 * 金额/币种/账户由 fixedTarget 重新初始化（不依赖弹窗内容卸载）。 */
const showRefund = ref(false)
const refundSource = ref<Transaction | null>(null)
const refundSeq = ref(0)

function openRefundFromRow(row: Transaction) {
  refundSource.value = row
  refundSeq.value += 1
  showRefund.value = true
}

function onRefundCreated() {
  showRefund.value = false
  page.value = 1
  void refresh()
}

/** 编辑弹窗（issue #178）：income/expense/transfer 行右键「编辑」，回填该笔交易
 * 全部业务字段，kind 锁死不可切换（跨 kind 编辑本期边界外，见 issue #176 边界）；
 * 提交走全字段更新命令（update_transaction，与 HTTP PUT 同一行为层权威）。
 * editSeq 作为表单 key：每次打开强制重建表单实例（镜像退款弹窗机制），
 * 回填/提交均指向本次右键所在行。提交失败弹窗不关、已填内容不丢
 * （错误提示与不重置均在表单 composable 内）。 */
const showEdit = ref(false)
const editTarget = ref<Transaction | null>(null)
const editSeq = ref(0)

function openEditFromRow(row: Transaction) {
  editTarget.value = row
  editSeq.value += 1
  showEdit.value = true
}

/** 编辑成功：关窗并刷新列表（保持当前页与筛选，不重置 page）。 */
function onEditSaved() {
  showEdit.value = false
  void refresh()
}

/** 「加入物品」确认弹窗（issue #119 / ADR-0025 创建唯一入口）：原交易由右键所在行固定，
 * 日期/成本/币种只读带出，名称默认备注可微调；提交走既有物品创建命令（溯源必填）。
 * 成功后不手动刷新交易列表（物品写入与交易列表无关），物品 store 经
 * ledger:changed 自动重拉，菜单下次打开即为置灰态。 */
const showAddItem = ref(false)
const addItemSource = ref<Transaction | null>(null)
const addItemSeq = ref(0)

function openAddItemFromRow(row: Transaction) {
  addItemSource.value = row
  addItemSeq.value += 1
  showAddItem.value = true
}

/** 成功与取消都只是关窗（物品列表经 ledger:changed 自动重拉，交易列表无关）。 */
function closeAddItem() {
  showAddItem.value = false
}

/** 右键菜单（issue #151 / #119 / #177 / #178）：income/expense/transfer 行首项「编辑」，
 * expense 行另有「退款」「加入物品」（已建物品置灰），所有行含「删除」；
 * 选项组装收口在 transaction-row-menu 纯函数（可独立测试），
 * 菜单项图标与删除项 error 色也由该函数统一注入。 */
const menuShow = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuRow = ref<Transaction | null>(null)

/** 已建物品的交易 id 集合（按物品溯源指针比对，不新增查询、不建反向引用）。 */
const linkedTxIds = computed(
  () =>
    new Set(
      itemsStore.items.map((i) => i.purchase_transaction_id).filter((id): id is string => id !== null),
    ),
)

// 主题 error 色（issue #177）：删除项经 DropdownOption props 着色，不硬编码色值，
// 暗色模式自动适配（useThemeVars 随当前主题响应式取值）。
const themeVars = useThemeVars()

const menuOptions = computed(() =>
  menuRow.value
    ? buildRowMenuOptions(menuRow.value, {
        hasItem: linkedTxIds.value.has(menuRow.value.id),
        errorColor: themeVars.value.errorColor,
      })
    : [],
)

/** 行右键弹出菜单：先收起再 nextTick 展开，保证换行弹出时位置刷新。 */
function showRowMenu(e: MouseEvent, row: Transaction) {
  e.preventDefault()
  menuRow.value = row
  menuX.value = e.clientX
  menuY.value = e.clientY
  menuShow.value = false
  void nextTick(() => {
    menuShow.value = true
  })
}

function onMenuSelect(key: string) {
  menuShow.value = false
  const row = menuRow.value
  if (!row) return
  if (key === 'edit') openEditFromRow(row)
  else if (key === 'refund') openRefundFromRow(row)
  else if (key === 'add-item') openAddItemFromRow(row)
  else if (key === 'delete') confirmDelete(row)
}

/** 表格行属性：绑定行右键菜单。 */
const rowProps = (row: Transaction) => ({
  onContextmenu: (e: MouseEvent) => showRowMenu(e, row),
})

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

const columns: DataTableColumn<Transaction>[] = [...buildTransactionColumns(reference)]

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
      <!-- 分裂按钮：主体直开支出弹窗，箭头展开 5 项类型菜单（issue #150） -->
      <NButtonGroup>
        <NButton type="primary" @click="openCreate('expense')">记一笔</NButton>
        <NDropdown
          trigger="click"
          :options="createKindOptions"
          @select="(k: string | number) => openCreate(k as CreateTransactionKind)"
        >
          <NButton type="primary" aria-label="更多记账类型">
            <NIcon><ChevronDown /></NIcon>
          </NButton>
        </NDropdown>
      </NButtonGroup>
    </NSpace>
    <!-- 快速记账弹窗：标题标明入口选定类型，内嵌收窄后的 TransactionForm（无类型单选），
         提交成功关闭并刷新列表 -->
    <NModal
      :show="createKind !== null"
      :title="createTitle"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
      @update:show="onCreateShowUpdate"
    >
      <TransactionForm
        v-if="createKind"
        :kind="createKind"
        @created="onFormCreated"
      />
    </NModal>
    <!-- 行内退款弹窗：原交易由右键所在行固定（fixed-target），账户/币种锁定继承，
         金额默认原交易金额（可改）；提交走现有 kind=refund 写路径 -->
    <NModal
      v-model:show="showRefund"
      title="退款"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <RefundForm
        :key="refundSeq"
        v-if="refundSource"
        :fixed-target="refundSource"
        @created="onRefundCreated"
      />
    </NModal>
    <!-- 「加入物品」确认弹窗（issue #119）：原交易由右键所在行固定，自动带出只读展示 -->
    <NModal
      v-model:show="showAddItem"
      title="加入物品"
      preset="card"
      display-directive="if"
      style="width: 440px"
      :bordered="false"
    >
      <AddItemForm
        :key="addItemSeq"
        v-if="addItemSource"
        :transaction="addItemSource"
        @created="closeAddItem"
        @cancel="closeAddItem"
      />
    </NModal>
    <!-- 编辑弹窗（issue #178）：回填既有交易全部业务字段，kind 锁死；
         提交走全字段更新命令，成功关窗并刷新列表（保持当前页与筛选） -->
    <NModal
      v-model:show="showEdit"
      title="编辑交易"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <TransactionForm
        :key="editSeq"
        v-if="editTarget"
        :editing="editTarget"
        @saved="onEditSaved"
      />
    </NModal>
    <!-- 行右键菜单（issue #151 / #119）：expense 行「退款」「加入物品」+ 所有行「删除」，手动定位弹出 -->
    <NDropdown
      trigger="manual"
      placement="bottom-start"
      :show="menuShow"
      :x="menuX"
      :y="menuY"
      :options="menuOptions"
      :min-width="140"
      @select="onMenuSelect"
      @clickoutside="menuShow = false"
    />
    <!-- 备注列为弹性列（transaction-columns 中不设 width），表格始终铺满容器；
         窄窗口时备注先收缩，scroll-x（固定列宽总和）作为横向滚动下限 -->
    <NDataTable
      :columns="columns"
      :data="data"
      :loading="loading"
      :bordered="false"
      size="small"
      remote
      :row-props="rowProps"
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
