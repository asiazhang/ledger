<script setup lang="ts">
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { computed, h, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
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
import { ChevronBack, ChevronDown, ChevronForward } from '@vicons/ionicons5'
import AppModal from '@/components/AppModal.vue'
import AppDropdown from '@/components/AppDropdown.vue'
import AppSelect from '@/components/AppSelect.vue'
import { useAppDialog } from '@/composables/useAppDialog'
import TransactionForm from '@/components/TransactionForm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import RefundForm from '@/components/RefundForm.vue'
import AddItemForm from '@/components/AddItemForm.vue'
import { buildRowMenuOptions } from '@/components/transaction-row-menu'
import { useCreateShortcuts, CREATE_KIND_KEYS } from '@/composables/useCreateShortcuts'
import { useTransactionFilter, UNCATEGORIZED_ONLY } from '@/composables/useTransactionFilter'
import { useTransactionModalState } from '@/composables/useTransactionModalState'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useItemsStore } from '@/stores/items'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transaction-columns'
import { isLendingEntryKind } from '@/domain/lending'
import {
  TIME_PERIOD_PRESETS,
  canStepPeriod,
  derivePeriodBoundary,
  formatPeriodLabel,
  matchPreset,
  periodRange,
  presetRange,
  rangeToPeriod,
  stepPeriod,
  type TimePeriodPreset,
} from '@/utils/time-period'
import {
  CREATE_KINDS,
  LENDING_CREATE_DIRECTIONS,
  TRANSACTION_KINDS,
  type CreateFormKind,
  type ReportDateRange,
  type Transaction,
  type TransactionKind,
  type TransactionListFilter,
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

// 行操作弹窗编排（ADR-0045）：意图闭集为唯一事实源，显示开关由「意图非空」派生，
// 回调序号随 open 递增内化（作表单 key 强制重建实例）。四个行操作弹窗——记一笔（#338）、
// 退款/加入物品（#339）、编辑（#340）——同经本模块实例开启。
const { intent, seq, open: openModal, close: closeModal } = useTransactionModalState()

/** 是否有任一激活的过滤条件（控制清除按钮可用性与空态文案）。 */
const filtersActive = computed(() => Object.values(filters).some((v) => v !== null))

// 时间维度行（issue #381/#382/#383）：预设芯片是过滤模块「部分修改过滤维度」入口的触发方式，
// 点选即把含边界日期快照写入 dateFrom/dateTo（TransactionFilter 零改动，翻页归零与
// 自动刷新免费继承）。「全部」= 无日期过滤 = 默认态；高亮纯派生（matchPreset）——
// 当前区间恰为某预设定义（相对今天）时点亮，跨月/季/年后自动熄灭，列表快照不漂移。
// 今天以分钟级 tick 保持响应式（视图长驻跨期场景下预设定义随之翻转）；不持久化，
// 清除筛选与 URL 下钻复位把日期维度清空后自然回「全部」。
// 期间步进器（#383）在同一行尾部：游标不落状态——步进前从当前区间唯一反推
//（单位，期间），±1 后换算回区间写回快照；「全部」（无可反推区间）时置灰。
// 数据期间边界（#391，修订 #383「不钳制未来」）：步进钳制于边界内，
// 边界末端对应箭头置灰，不再可达无数据的期间。快照语义保持：步进后的高亮仍纯派生，
// 步到恰为某预设的历史周期时该芯片自然点亮（如当年 < 一步落到去年）。
const nowTick = ref(Date.now())
let nowTicker: ReturnType<typeof setInterval> | undefined
const activePreset = computed(() => matchPreset(filters.dateFrom, filters.dateTo, nowTick.value))

/** 当前可步进游标：从日期区间唯一反推的自然周期；「全部」/任意区间为 null（置灰）。 */
const currentPeriod = computed(() => rangeToPeriod(filters.dateFrom, filters.dateTo))

// 数据期间边界原始日期对（issue #391）：挂载拉取 + ledger:changed 失效重拉
//（AI 导入外扩历史、删除收窄边界即时跟随）。null = 在途或失败 → 钳制退化为
// 不钳制（不阻塞步进）；空库（双 null 日期对，非 null 对象）由派生单点回退为
// 单当前期间。重拉在途时沿用旧值到成功替换（stale-while-revalidate，与参考
// store 同形，不闪烁）；仅在失败时置空退化，静默不 toast（辅助钳制状态，
// 列表错误通道不受影响）。不走 useLoadable（ADR-0040）：需持值
// stale-while-revalidate + 刻意静默退化，均在其形态之外，序号守卫为该形态最小实现。
const dateRange = ref<ReportDateRange | null>(null)
let dateRangeSeq = 0
let unlistenLedgerChanged: UnlistenFn | null = null
let ledgerListenerDisposed = false

async function loadDateRange() {
  const seq = ++dateRangeSeq
  try {
    const range = await api.reportDateRange()
    if (seq === dateRangeSeq) dateRange.value = range
  } catch {
    if (seq === dateRangeSeq) dateRange.value = null
  }
}

/** 当前游标单位下的数据期间边界；「全部」无游标或边界未知（在途/失败）时为 null。 */
const periodBoundary = computed(() => {
  const p = currentPeriod.value
  if (!p || !dateRange.value) return null
  return derivePeriodBoundary(p.unit, dateRange.value, nowTick.value)
})

/** 步进可达性（#391）：边界末端对应箭头置灰；边界未知时 canStepPeriod
 * 退化为不钳制（恒 true）。公式 [最早交易期间, max(当前期间, 最新交易期间)]
 * 由期间数学单点派生，「与今天更晚者」的抬升随分钟级 nowTick 推移。 */
const canStepPrev = computed(() => {
  const p = currentPeriod.value
  return p !== null && canStepPeriod(p, -1, periodBoundary.value)
})
const canStepNext = computed(() => {
  const p = currentPeriod.value
  return p !== null && canStepPeriod(p, 1, periodBoundary.value)
})

/** 期间标签随步进实时更新（常驻行尾）；无游标时展示占位符。 */
const periodLabelText = computed(() =>
  currentPeriod.value
    ? formatPeriodLabel(currentPeriod.value)
    : t('transactions.filter.periodLabel.none'),
)

/** 步进：< / > 按当前区间单位步进到上一个/下一个自然周期并写回快照
 *（同值守卫在模块内；步进必然变更区间，不触发守卫）。钳制守卫双保险：
 * 按钮置灰为主，边界在点击派发间隙到达时在此拦下。 */
function onStepPeriod(delta: 1 | -1) {
  const p = currentPeriod.value
  if (!p || !canStepPeriod(p, delta, periodBoundary.value)) return
  const range = periodRange(stepPeriod(p, delta))
  setFilter({ dateFrom: range.from, dateTo: range.to })
}

/** 点芯片：换算含边界日期快照并写入过滤模块（同值守卫在模块内，重复点同芯片不重刷）。 */
function onPresetSelect(preset: TimePeriodPreset) {
  if (preset === 'all') {
    setFilter({ dateFrom: null, dateTo: null })
    return
  }
  const range = presetRange(preset, nowTick.value)
  setFilter({ dateFrom: range.from, dateTo: range.to })
}

/** 芯片文案 key：闭集枚举 → i18n（transactions.filter.period.*）。 */
function presetLabel(preset: TimePeriodPreset): string {
  return t(`transactions.filter.period.${preset}`)
}

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
 * Rust 侧另有 dividend/split 未在前端类型暴露，不进过滤选项）。标签经 t() 随语言切换。 */
const kindOptions = computed<Array<{ label: string; value: TransactionKind }>>(() =>
  TRANSACTION_KINDS.map((value) => ({
    label: t(`transactions.kind.${value}`),
    value,
  })),
)

/** 列表请求（ADR-0030 决策 6：请求发起、loading、行数据归视图）：以模块当前状态装配
 * 请求参数并发起查询。 */
async function load() {
  loading.value = true
  try {
    const filter: TransactionListFilter = {
      page: page.value,
      page_size: pageSize.value,
    }
    // 过滤参数按需携带（空值省略，与后端可选字段语义一致）；分类维度三态装配（issue #377）
    if (filters.dateFrom) filter.from = filters.dateFrom
    if (filters.dateTo) filter.to = filters.dateTo
    if (filters.involvingAccountId) filter.involving_account_id = filters.involvingAccountId
    if (filters.merchantId) filter.merchant_id = filters.merchantId
    if (filters.categoryId === UNCATEGORIZED_ONLY) filter.uncategorized_only = true
    else if (filters.categoryId) filter.category_id = filters.categoryId
    if (filters.kind) filter.kind = filters.kind
    const res = await api.listTransactions(filter)
    data.value = res.items
    total.value = res.total
  } catch (e) {
    message.error(t('transactions.list.loadFailed', { msg: errorMessage(e) }))
  } finally {
    loading.value = false
  }
}

// 重拉唯一触发点：模块 bump 版本号 = 需以当前模块状态重拉。首刷（onMounted 经统一出口
// refresh）与全部意图入口共用此路径；同一同步批次内的多次 bump（如 URL 多维度同时
// 声明意图）由 watcher 去重为一次请求，双刷被出口唯一性消灭。
watch(refreshVersion, () => {
  void load()
})

// URL 下钻只读入口（issue #234 / #96 决策 3/4）：?account= / ?merchant= / ?category=（issue #377）
// 的解析与校验、复位规则、参考数据就绪补判与字段级让位全部内化在 TransactionFilter 参数表；
// 视图只监听路由并把 query 递给模块，不持任何时序标志与解析逻辑。
// URL 只读不写回（组件状态是唯一事实源）。
watch(() => route.query, (query) => syncUrlQuery(query), { immediate: true })

/** 页大小选项（不持久化，遵守 ViewState 决策） */
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100]

/** 记一笔（create）经共享模块实例开启（意图/序号见上方编排声明）：
 * 类型由入口单点表达，弹窗内不提供切换，中途换类型 = 关闭重开。 */

/** 创建形态标签：借贷变体入口取借贷文案，交易 kind 取 transactions.kind.*（issue #374）。 */
function createKindLabel(kind: CreateFormKind): string {
  return isLendingEntryKind(kind)
    ? t(`transactions.lending.${kind}`)
    : t(`transactions.kind.${kind}`)
}

/** 下拉选项：5 种可创建类型（refund 不在入口：退款已移出表单域，入口由交易条目
 * 右键菜单承接，独立 ticket 落地前处于过渡态）+ 借贷变体「借出」「借入」两项
 * （issue #374，分隔线分组；不占快捷键键位）。kind 项标签后附裸键快捷键提示（issue #153），
 * 键位来自 CREATE_KIND_KEYS 单一来源，与 keydown 匹配共用。 */
const createKindOptions = computed<DropdownOption[]>(() => [
  ...CREATE_KINDS.map((k) => ({
    label: t('transactions.create.kindWithKey', {
      kind: t(`transactions.kind.${k}`),
      key: CREATE_KIND_KEYS[k],
    }),
    key: k,
  })),
  { type: 'divider', key: 'create-lending-divider' },
  ...LENDING_CREATE_DIRECTIONS.map((d) => ({
    label: t(`transactions.lending.${d}`),
    key: d,
  })),
])

const createTitle = computed(() => {
  const current = intent.value
  return current?.type === 'create'
    ? t('transactions.create.titleWithKind', { kind: createKindLabel(current.kind) })
    : t('transactions.create.title')
})

/** 记一笔各入口（顶栏主体 / 子类型下拉 / 裸键快捷键）统一经模块开启。 */
function openCreate(k: CreateFormKind) {
  void openModal({ type: 'create', kind: k })
}

// 裸键快捷键（issue #153）：a/z/i/b/s 直达对应类型弹窗，与点下拉对应项同一入口；
// 焦点在可编辑元素或弹层打开时抑制；随视图装卸，仅交易页生效
useCreateShortcuts(openCreate)

/** 提交成功：关窗（模块意图清回终态），回填意图 refresh（重拉 + 翻回第 1 页，
 * 新记录按日期/时间排序最可能落在第 1 页），保留筛选条件（与手动过滤同等语义，不重置）。 */
function onFormCreated() {
  closeModal()
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

function onKindFilterChange(value: TransactionKind | null) {
  setFilter({ kind: value })
}

async function remove(id: string) {
  try {
    await api.deleteTransaction(id)
    message.success(t('transactions.list.deleted'))
    // 删除成功 → 页码回退入口（ADR-0045）：声明本页删后剩 N 条，回退与重拉由模块内化，
    // 视图不再直写页码、不再自行发起请求（删前本页 1 条 ⇔ 删后超页，ADR-0008）
    afterRowDelete(data.value.length - 1)
  } catch (e) {
    message.error(t('transactions.list.deleteFailed', { msg: errorMessage(e) }))
  }
}

/** 删除走 useAppDialog 二次确认（issue #151）：取消不删，确认后才删除。
 * 遮罩点击不构成关闭意图（issue #252 弹层关闭语义）：确认/取消须显式点击。 */
function confirmDelete(row: Transaction) {
  dialog.warning({
    title: t('transactions.deleteDialog.title'),
    content: t('transactions.deleteDialog.content'),
    positiveText: t('transactions.deleteDialog.confirm'),
    negativeText: t('transactions.deleteDialog.cancel'),
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
 * 开启/关闭编排经 TransactionModalState（ADR-0045，#340）：目标行由意图携带
 * （fixed-target），序号作表单 key 强制重建实例（回填/提交均指向本次右键所在行）；
 * buy/sell 的「先取买卖明细再开窗、失败不开窗」时序与慢取竞态守卫内化在模块，
 * 取数不经视图。提交失败弹窗不关、已填内容不丢（错误提示与不重置均在表单 composable 内）。 */
function openEditFromRow(row: Transaction) {
  void openModal({ type: 'edit', row })
}

/** 编辑成功：关窗（编排内化关闭意图）并以当前页码重拉列表（保持当前页与筛选，
 * 不重置 page → 视图侧 load，不经模块出口 refresh 的翻回第 1 页语义）。 */
function onEditSaved() {
  closeModal()
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
  prefix: ({ itemCount }) =>
    h('span', null, () => t('transactions.list.total', { n: itemCount ?? 0 })),
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

// 列经 computed 构造：列名（t()）随语言切换即时重建（列宽总和随之联动）
const columns = computed<DataTableColumn<Transaction>[]>(() => [
  ...buildTransactionColumns(reference),
])

// scroll-x：列中所有固定列（有 width 的列，备注为弹性列不计入）宽度总和
const scrollX = computed(() => sumFixedColumnWidths(columns.value))

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll；
  // 首刷经模块统一出口（refresh 即「翻回第 1 页 + 重拉」；URL 初始化已在 setup 期
  // 声明意图，同一同步批次内被 watcher 去重为一次首刷请求）
  refresh()
  // 数据期间边界首拉（issue #391）
  void loadDateRange()
  // 订阅 ledger:changed：数据写入/删除后边界重拉，即时外扩/收窄。注册为异步，
  // 注册完成前到达的信号会丢失（窗口极窄，与参考 store 订阅同形）。
  void listen('ledger:changed', () => {
    void loadDateRange()
  })
    .then((fn) => {
      if (ledgerListenerDisposed) {
        fn()
        return
      }
      unlistenLedgerChanged = fn
    })
    .catch(() => {
      /* 监听注册失败不阻塞视图（本地事件，极少发生） */
    })
  // 今天 tick：分钟级刷新响应式「今天」，驱动预设定义、高亮派生与边界抬升跨期翻转
  nowTicker = setInterval(() => {
    nowTick.value = Date.now()
  }, 60_000)
})
onBeforeUnmount(() => {
  if (nowTicker !== undefined) clearInterval(nowTicker)
  ledgerListenerDisposed = true
  unlistenLedgerChanged?.()
  unlistenLedgerChanged = null
})
</script>

<style scoped>
/* 期间标签：常驻步进器中央，min-width 抑制不同期间文案宽度抖动 */
.period-label {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 96px;
  padding: 0 8px;
  font-size: 13px;
  white-space: nowrap;
}
</style>

<template>
  <NSpace vertical :size="12">
    <!-- 过滤行（第一行）：账户（涉及账户语义，可清除）+ 商户（可清除，issue #191）+ 类型（可清除）+ 清除筛选。
         任意起止日期筛选已自交易页移除（搜索页保留，issue #381）；
         任一条件变化即重新查询并回到第 1 页；手动改动不同步回 URL
         （组件状态是唯一事实源，issue #96 决策 3/4），分页/页大小切换保持过滤条件。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <PinyinSelect
        :value="filters.involvingAccountId"
        :options="accountOptions"
        :placeholder="t('transactions.filter.account')"
        clearable
        style="width: 160px"
        @update:value="onAccountFilterChange"
      />
      <PinyinSelect
        :value="filters.merchantId"
        :options="merchantOptions"
        :placeholder="t('transactions.filter.merchant')"
        clearable
        style="width: 160px"
        @update:value="onMerchantFilterChange"
      />
      <AppSelect
        :value="filters.kind"
        :options="kindOptions"
        :placeholder="t('transactions.filter.kind')"
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
        {{ t('transactions.filter.clear') }}
      </NButton>
      <!-- 分裂按钮：主体直开支出弹窗，箭头展开 5 项类型菜单（issue #150） -->
      <NButtonGroup>
        <NButton type="primary" @click="openCreate('expense')">{{ t('transactions.create.button') }}</NButton>
        <AppDropdown
          trigger="click"
          :options="createKindOptions"
          @select="(k: string | number) => openCreate(k as CreateFormKind)"
        >
          <NButton type="primary" :aria-label="t('transactions.create.moreTypes')">
            <NIcon><ChevronDown /></NIcon>
          </NButton>
        </AppDropdown>
      </NButtonGroup>
    </NSpace>
    <!-- 时间维度行（第二行，issue #381/#382/#383）：单选分段芯片「全部 | 当月 | 当季 | 当年 | 去年」
         ＋尾部常驻期间步进器「< 期间标签 >」。点芯片/步进写入日期快照（翻页归零 + 重拉免费继承）；
         点亮纯派生；「全部」= 默认态；「全部」时步进置灰；步进钳制于数据期间边界（#391）。
         分段控件非弹层（#381），不涉弹层注册表与快捷键抑制。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <NButtonGroup size="small">
        <NButton
          v-for="p in TIME_PERIOD_PRESETS"
          :key="p"
          size="small"
          :type="activePreset === p ? 'primary' : 'default'"
          :quaternary="activePreset !== p"
          :aria-pressed="activePreset === p"
          @click="onPresetSelect(p)"
        >
          {{ presetLabel(p) }}
        </NButton>
      </NButtonGroup>
      <NButtonGroup size="small">
        <NButton
          size="small"
          quaternary
          :disabled="!canStepPrev"
          :aria-label="t('transactions.filter.period.prev')"
          @click="onStepPeriod(-1)"
        >
          <NIcon><ChevronBack /></NIcon>
        </NButton>
        <span class="period-label">{{ periodLabelText }}</span>
        <NButton
          size="small"
          quaternary
          :disabled="!canStepNext"
          :aria-label="t('transactions.filter.period.next')"
          @click="onStepPeriod(1)"
        >
          <NIcon><ChevronForward /></NIcon>
        </NButton>
      </NButtonGroup>
    </NSpace>
    <!-- 快速记账弹窗：标题标明入口选定类型，内嵌收窄后的 TransactionForm（无类型单选），
         提交成功关闭并刷新列表；显示开关由模块意图派生，序号作表单 key 强制重建（ADR-0045） -->
    <AppModal
      :show="intent?.type === 'create'"
      :title="createTitle"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
      @update:show="(show: boolean) => { if (!show) closeModal() }"
    >
      <TransactionForm
        v-if="intent?.type === 'create'"
        :key="seq"
        :kind="intent.kind"
        @created="onFormCreated"
      />
    </AppModal>
    <!-- 行内退款弹窗：原交易由右键所在行固定（fixed-target），账户/币种锁定继承，
         金额默认原交易金额（可改）；提交走现有 kind=refund 写路径。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="intent?.type === 'refund'"
      :title="t('transactions.refund.title')"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
      @update:show="onModalShowUpdate"
    >
      <RefundForm
        :key="seq"
        v-if="intent?.type === 'refund'"
        :fixed-target="intent.row"
        @created="onRefundCreated"
      />
    </AppModal>
    <!-- 「加入物品」确认弹窗（issue #119）：原交易由右键所在行固定，自动带出只读展示。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="intent?.type === 'add-item'"
      :title="t('transactions.addItem.title')"
      preset="card"
      display-directive="if"
      style="width: 440px"
      :bordered="false"
      @update:show="onModalShowUpdate"
    >
      <AddItemForm
        :key="seq"
        v-if="intent?.type === 'add-item'"
        :transaction="intent.row"
        @created="closeAddItem"
        @cancel="closeAddItem"
      />
    </AppModal>
    <!-- 编辑弹窗（issue #178）：回填既有交易全部业务字段，kind 锁死；提交走全字段更新命令，
         成功关窗并刷新列表（保持当前页与筛选）。开启/关闭经 TransactionModalState 编排
         （目标行与买卖明细由意图携带，序号作表单 key 强制重建） -->
    <AppModal
      :show="intent?.type === 'edit'"
      :title="t('transactions.edit.title')"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
      @update:show="onModalShowUpdate"
    >
      <TransactionForm
        :key="seq"
        v-if="intent?.type === 'edit'"
        :editing="intent.row"
        :trade="intent.trade"
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
        <NEmpty
          :description="filtersActive ? t('transactions.list.emptyFiltered') : t('transactions.list.empty')"
          size="small"
        >
          <template v-if="filtersActive" #extra>
            <NButton size="tiny" quaternary type="primary" @click="resetFilters">
              {{ t('transactions.filter.clear') }}
            </NButton>
          </template>
        </NEmpty>
      </template>
    </NDataTable>
  </NSpace>
</template>
