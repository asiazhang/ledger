<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, h, nextTick, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  NDataTable,
  NButton,
  NButtonGroup,
  NIcon,
  NEmpty,
  NSpace,
  useMessage,
  useThemeVars,
  type DataTableColumn,
  type DropdownOption,
  type PaginationProps,
} from 'naive-ui'
import { ChevronDown } from '@vicons/ionicons5'
import AppModal from '@/components/AppModal.vue'
import AppDropdown from '@/components/AppDropdown.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import { useAppDialog } from '@/composables/useAppDialog'
import TransactionForm from '@/components/TransactionForm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import RefundForm from '@/components/RefundForm.vue'
import AddItemForm from '@/components/AddItemForm.vue'
import { buildRowMenuOptions } from '@/components/transaction-row-menu'
import { useCreateShortcuts, CREATE_KIND_KEYS } from '@/composables/useCreateShortcuts'
import { useTransactionFilter } from '@/composables/useTransactionFilter'
import { useTransactionModalState } from '@/composables/useTransactionModalState'
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
  type TransactionTrade,
} from '@/types'

const reference = useReferenceStore()
// 物品 store（issue #119）：仅用于右键菜单「加入物品」的置灰态判断；
// self-init + ledger:changed 自动重拉，创建成功后菜单下次打开即为置灰态。
const itemsStore = useItemsStore()
const message = useMessage()
const dialog = useAppDialog()
const route = useRoute()
const data = ref<Transaction[]>([])
const total = ref(0)
const loading = ref(false)

// 过滤状态机（ADR-0030）：「用户意图进、列表状态出」。filters / page / pageSize / 重拉版本号
// 归 TransactionFilter 模块所有；手动筛选变更走 setFilter、清除筛选走 resetFilters、
// 记一笔/退款回填与页大小切换走 refresh（重拉 + 翻回第一页）、删除成功走 afterRowDelete
// （页码回退入口，ADR-0045）——「翻页归零 + 刷新」全仓仅模块统一出口一处，视图不再持有
// 翻页归零样板与首刷防双刷标志，也不直写页码回退。
const {
  filters,
  page,
  pageSize,
  refreshVersion,
  setFilter,
  resetFilters,
  refresh,
  afterRowDelete,
  syncUrlQuery,
} = useTransactionFilter()

// 行操作弹窗编排（ADR-0045）：意图闭集四为唯一事实源，显示开关由「意图非空」派生，
// 回调序号随 open 递增内化（作表单 key 强制重建实例）。本票（#339）先接线退款与
// 加入物品两同步弹窗；记一笔（#338）与编辑（#340）仍走各自过渡态，后续票接线。
const { intent: modalIntent, seq: modalSeq, open: openModal, close: closeModal } =
  useTransactionModalState()

/** 是否有任一激活的过滤条件（控制清除按钮可用性与空态文案）。 */
const filtersActive = computed(() => Object.values(filters).some((v) => v !== null))

/** 账户下拉选项：来自参考数据账户映射（list_accounts 不含 is_hidden 黑洞账户，沿用既有边界）。 */
const accountOptions = computed(() =>
  reference.accounts.map((a) => ({ label: a.name, value: a.id })),
)

/** 商户下拉选项：来自 merchantMap（在用 + 软删，issue #191）——
 * 软删商户仍有历史交易，需可被选中过滤；按名称排序保证稳定。 */
const merchantOptions = computed(() =>
  [...reference.merchantMap.values()]
    .sort((a, b) => a.name.localeCompare(b.name, 'zh'))
    .map((m) => ({ label: m.name, value: m.id })),
)

/** 类型下拉选项：前端 TransactionKind 的 6 种（income/expense/transfer/refund/buy/sell；
 * Rust 侧另有 dividend/split 未在前端类型暴露，不进过滤选项）。 */
const kindOptions: Array<{ label: string; value: TransactionKind }> = (
  Object.entries(TRANSACTION_KIND_LABELS) as [TransactionKind, string][]
).map(([value, label]) => ({ label, value }))

/** 列表请求（ADR-0030 决策 6：请求发起、loading、行数据归视图）：以模块当前状态装配
 * 请求参数并发起查询。 */
async function load() {
  loading.value = true
  try {
    const filter: TransactionListFilter = {
      page: page.value,
      page_size: pageSize.value,
    }
    // 过滤参数按需携带（空值省略，与后端可选字段语义一致）
    if (filters.dateFrom) filter.from = filters.dateFrom
    if (filters.dateTo) filter.to = filters.dateTo
    if (filters.involvingAccountId) filter.involving_account_id = filters.involvingAccountId
    if (filters.merchantId) filter.merchant_id = filters.merchantId
    if (filters.kind) filter.kind = filters.kind
    const res = await api.listTransactions(filter)
    data.value = res.items
    total.value = res.total
  } catch (e) {
    message.error(`加载失败: ${errorMessage(e)}`)
  } finally {
    loading.value = false
  }
}

// 重拉唯一触发点：模块 bump 版本号 = 需以当前模块状态重拉。首刷（onMounted 经统一出口
// refresh）与全部意图入口共用此路径；同一同步批次内的多次 bump（如 URL 两维度同时
// 声明意图）由 watcher 去重为一次请求，双刷被出口唯一性消灭。
watch(refreshVersion, () => {
  void load()
})

// URL 下钻只读入口（issue #234 / #96 决策 3/4）：?account= / ?merchant= 的解析与校验、
// 复位规则（两维度均无有效参数时复位日期/类型）、参考数据就绪补判与字段级让位
// 全部内化在 TransactionFilter 参数表；视图只监听路由并把 query 递给模块，
// 不持任何时序标志与解析逻辑。URL 只读不写回（组件状态是唯一事实源）。
watch(() => route.query, (query) => syncUrlQuery(query), { immediate: true })

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

/** 提交成功：回填意图 refresh（重拉 + 翻回第 1 页，新记录按日期/时间排序最可能落在第 1 页），
 * 保留筛选条件（与手动过滤同等语义，不重置）。 */
function onFormCreated() {
  createKind.value = null
  refresh()
}

/** 手动过滤处理器：声明意图即生效（翻页归零 + 重拉，同值守卫在模块 setFilter 内：
 * 条件实际变化才动作），不同步回 URL（组件状态是唯一事实源）。 */
function onAccountFilterChange(id: string | null) {
  setFilter({ involvingAccountId: id })
}

function onMerchantFilterChange(id: string | null) {
  setFilter({ merchantId: id })
}

function onDateFromChange(value: string | null) {
  setFilter({ dateFrom: value })
}

function onDateToChange(value: string | null) {
  setFilter({ dateTo: value })
}

function onKindFilterChange(value: TransactionKind | null) {
  setFilter({ kind: value })
}

async function remove(id: string) {
  try {
    await api.deleteTransaction(id)
    message.success('已删除')
    // 删除成功 → 页码回退入口（ADR-0045）：声明本页删后剩 N 条，回退与重拉由模块内化，
    // 视图不再直写页码、不再自行发起请求（删前本页 1 条 ⇔ 删后超页，ADR-0008）
    afterRowDelete(data.value.length - 1)
  } catch (e) {
    message.error(`删除失败: ${errorMessage(e)}`)
  }
}

/** 删除走 useAppDialog 二次确认（issue #151）：取消不删，确认后才删除。
 * 遮罩点击不构成关闭意图（issue #252 弹层关闭语义）：确认/取消须显式点击。 */
function confirmDelete(row: Transaction) {
  dialog.warning({
    title: '删除交易',
    content: '确认删除该条交易？删除后不可恢复。',
    positiveText: '删除',
    negativeText: '取消',
    maskClosable: false,
    onPositiveClick: () => remove(row.id),
  })
}

/** 行内退款弹窗（issue #151）：目标交易行由右键所在行固定（fixed-target），
 * 序号随每次开启递增（作表单 key 强制重建实例，金额/币种/账户由 fixedTarget
 * 重新初始化，不依赖弹窗内容卸载）。开启/关闭编排经 TransactionModalState
 * （ADR-0045）；同一支出可多次发起退款（部分退款语义，不阻断）。 */
function openRefundFromRow(row: Transaction) {
  void openModal({ type: 'refund', row })
}

/** 退款提交成功：关窗（编排内化关闭意图）并回填意图 refresh（重拉 + 翻回第 1 页，
 * 新退款按日期排序最可能落在第 1 页，保留筛选条件）。 */
function onRefundCreated() {
  closeModal()
  refresh()
}

/** 编辑弹窗（issue #178，issue #180 扩到 buy/sell）：行右键「编辑」，回填该笔交易
 * 全部业务字段，kind 锁死不可切换（跨 kind 编辑本期边界外，见 issue #176 边界）；
 * 提交走全字段更新命令（update_transaction，与 HTTP PUT 同一行为层权威）。
 * buy/sell 行另取买卖明细（get_transaction_trade，扩展表投影）回填标的/数量/价格/费用；
 * 取明细失败不弹窗（直接报错）。editSeq 作为表单 key：每次打开强制重建表单实例
 * （镜像退款弹窗机制），回填/提交均指向本次右键所在行。提交失败弹窗不关、
 * 已填内容不丢（错误提示与不重置均在表单 composable 内）。 */
const showEdit = ref(false)
const editTarget = ref<Transaction | null>(null)
const editTrade = ref<TransactionTrade | null>(null)
const editSeq = ref(0)

async function openEditFromRow(row: Transaction) {
  if (row.kind === 'buy' || row.kind === 'sell') {
    try {
      editTrade.value = await api.getTransactionTrade(row.id)
    } catch (e) {
      message.error(`无法编辑: ${errorMessage(e)}`)
      return
    }
  } else {
    editTrade.value = null
  }
  editTarget.value = row
  editSeq.value += 1
  showEdit.value = true
}

/** 编辑成功：关窗并以当前页码重拉列表（保持当前页与筛选，不重置 page → 视图侧 load，
 * 不经模块出口）。 */
function onEditSaved() {
  showEdit.value = false
  void load()
}

/** 「加入物品」确认弹窗（issue #119 / ADR-0025 创建唯一入口）：目标交易行由右键所在行
 * 固定，日期/成本/币种只读带出，名称默认备注可微调；提交走既有物品创建命令
 * （溯源必填）。开启/关闭编排经 TransactionModalState（ADR-0045）；成功与取消都
 * 只是关窗（物品写入与交易列表无关，物品 store 经 ledger:changed 自动重拉，菜单
 * 下次打开即为置灰态）。 */
function openAddItemFromRow(row: Transaction) {
  void openModal({ type: 'add-item', row })
}

/** 成功与取消都只关窗（经编排内化关闭意图；物品列表经 ledger:changed 自动重拉）。 */
function closeAddItem() {
  closeModal()
}

/** 退款/加入物品弹窗经 ✕ / ESC 显式关闭：走编排内化关闭（意图清回空终态）。 */
function onModalShowUpdate(show: boolean) {
  if (!show) closeModal()
}

/** 行右键菜单（issue #151 / #119 / #177 / #178 / #180）：除 refund 外行首项「编辑」，
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
    void load()
  },
  onUpdatePageSize: (size: number) => {
    // 页大小归模块分页所有：写入后经统一出口重拉（翻回第 1 页）
    pageSize.value = size
    refresh()
  },
}))

const columns: DataTableColumn<Transaction>[] = [...buildTransactionColumns(reference)]

// scroll-x：列中所有固定列（有 width 的列，备注为弹性列不计入）宽度总和
const scrollX = sumFixedColumnWidths(columns)

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll；
  // 首刷经模块统一出口（refresh 即「翻回第 1 页 + 重拉」；URL 初始化已在 setup 期
  // 声明意图，同一同步批次内被 watcher 去重为一次首刷请求）
  refresh()
})
</script>

<template>
  <NSpace vertical :size="12">
    <!-- 过滤行：账户（涉及账户语义，可清除）+ 商户（可清除，issue #191）+ 日期起止 + 类型（可清除）+ 清除筛选。
         任一条件变化即重新查询并回到第 1 页；手动改动不同步回 URL
         （组件状态是唯一事实源，issue #96 决策 3/4），分页/页大小切换保持过滤条件。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <PinyinSelect
        :value="filters.involvingAccountId"
        :options="accountOptions"
        placeholder="账户"
        clearable
        style="width: 160px"
        @update:value="onAccountFilterChange"
      />
      <PinyinSelect
        :value="filters.merchantId"
        :options="merchantOptions"
        placeholder="商户"
        clearable
        style="width: 160px"
        @update:value="onMerchantFilterChange"
      />
      <AppDatePicker
        :formatted-value="filters.dateFrom"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="起始日期"
        clearable
        style="width: 140px"
        @update:formatted-value="onDateFromChange"
      />
      <AppDatePicker
        :formatted-value="filters.dateTo"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="结束日期"
        clearable
        style="width: 140px"
        @update:formatted-value="onDateToChange"
      />
      <AppSelect
        :value="filters.kind"
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
        @click="resetFilters"
      >
        清除筛选
      </NButton>
      <!-- 分裂按钮：主体直开支出弹窗，箭头展开 5 项类型菜单（issue #150） -->
      <NButtonGroup>
        <NButton type="primary" @click="openCreate('expense')">记一笔</NButton>
        <AppDropdown
          trigger="click"
          :options="createKindOptions"
          @select="(k: string | number) => openCreate(k as CreateTransactionKind)"
        >
          <NButton type="primary" aria-label="更多记账类型">
            <NIcon><ChevronDown /></NIcon>
          </NButton>
        </AppDropdown>
      </NButtonGroup>
    </NSpace>
    <!-- 快速记账弹窗：标题标明入口选定类型，内嵌收窄后的 TransactionForm（无类型单选），
         提交成功关闭并刷新列表 -->
    <AppModal
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
    </AppModal>
    <!-- 行内退款弹窗：原交易由右键所在行固定（fixed-target），账户/币种锁定继承，
         金额默认原交易金额（可改）；提交走现有 kind=refund 写路径。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="modalIntent?.type === 'refund'"
      title="退款"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
      @update:show="onModalShowUpdate"
    >
      <RefundForm
        :key="modalSeq"
        v-if="modalIntent?.type === 'refund'"
        :fixed-target="modalIntent.row"
        @created="onRefundCreated"
      />
    </AppModal>
    <!-- 「加入物品」确认弹窗（issue #119）：原交易由右键所在行固定，自动带出只读展示。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="modalIntent?.type === 'add-item'"
      title="加入物品"
      preset="card"
      display-directive="if"
      style="width: 440px"
      :bordered="false"
      @update:show="onModalShowUpdate"
    >
      <AddItemForm
        :key="modalSeq"
        v-if="modalIntent?.type === 'add-item'"
        :transaction="modalIntent.row"
        @created="closeAddItem"
        @cancel="closeAddItem"
      />
    </AppModal>
    <!-- 编辑弹窗（issue #178）：回填既有交易全部业务字段，kind 锁死；
         提交走全字段更新命令，成功关窗并刷新列表（保持当前页与筛选） -->
    <AppModal
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
        :trade="editTrade"
        @saved="onEditSaved"
      />
    </AppModal>
    <!-- 行右键菜单（issue #151 / #119）：expense 行「退款」「加入物品」+ 所有行「删除」，手动定位弹出 -->
    <AppDropdown
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
            <NButton size="tiny" quaternary type="primary" @click="resetFilters">
              清除筛选
            </NButton>
          </template>
        </NEmpty>
      </template>
    </NDataTable>
  </NSpace>
</template>
