<script setup lang="ts">
import { h, ref, computed, watch } from 'vue'
import {
  NButton,
  NCard,
  NCheckbox,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumn,
  type PaginationProps,
} from 'naive-ui'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import MerchantEditModal from '@/components/merchants/MerchantEditModal.vue'
import { api } from '@/api'
import { useRouter } from 'vue-router'
import { useReferenceStore } from '@/stores/reference'
import { matchLabel } from '@/utils/pinyin-filter'
import { t } from '@/i18n'
import { formatQuantity } from '@/utils/money'
import type { Merchant, MerchantInput } from '@/types'

// 商户管理（issue #189 / ADR-0028）：字典为扁平表（无层级、无 sort_order，按名称排序），
// 交互沿用分类管理先例——新增表单卡片 + 列表卡片 + 编辑弹窗；写入成功后参考数据
// 由 ledger:changed 信号自动重拉，交易列表/表单补全即时更新。
// 商户回归「名字字典」（issue #223）：只处理名称，无图标/颜色列与输入框。
// 关联交易条数列（issue #445，毛笔数口径）：条数为独立只读聚合
// （list_merchant_transaction_counts，实时推导不落库），不进参考 store 四表——
// 列表行仍消费参考数据单一来源，计数按 merchant_id 客户端拼接；
// 商户写入经既有失效信号触发 store 重拉，store version 变化即伴随重拉计数。
// 条数下钻（issue #446）：点击条数直达按该商户过滤的交易列表，与商户名链接
// （MerchantLink）、报表分类下钻同一 URL 下钻机制（?merchant=<id>）；落地后的
// 过滤行为由 TransactionFilter 既有机制承担（含与分类/账户/日期参数 AND 并存、
// 软删商户经 merchantMap 历史交易口径可解析）。本票只负责产生正确的跳转。
// 拼音搜索与显示已删（issue #447）：搜索按统一模糊搜索语义本地过滤（ADR-0027，
// 唯一定义点为核心交易域 TransactionSearch）；已删行只读展示、条数照常可下钻。
// 前端分页（issue #457）：数据仍全量驻留前端、搜索仍是本地过滤，仅展示层切片——
// 非表格 remote 分页形态（那是 ADR-0008 服务端分页载体），NDataTable 以客户端
// 分页模式对过滤后全量行自行先排序后切片；分页条与交易页同形态（过滤后总数 +
// 页大小选择）。过滤意图变化（搜索输入/清空、显示已删、排序）页码归零；删除
// 当前页最后一条页码回退一页（ADR-0045 先例，按展示集合事实判定「本页删后
// 剩余条数」：显示已删开启时软删行不离开展示集合，不回退）。页码与页大小为
// 组件内受控状态、不持久化，页签卸载重挂即回第一页。

interface MerchantRow extends Merchant {
  /** 关联交易条数（毛笔数）：无引用商户为 0 */
  transactionCount: number
}

const reference = useReferenceStore()
const message = useMessage()
const router = useRouter()

/** 条数下钻（issue #446）：按行 id 产生跳转，不对条数/商户状态设门——
 * 条数为 0 点击只见空列表（诚实行为）；软删商户行（#447 引入展示后）同样可下钻。 */
function goMerchantTransactions(m: MerchantRow) {
  router.push({ name: 'transactions', query: { merchant: m.id } })
}

// —— 关联交易条数（独立读模型，非关键路径：失败保留旧值不阻塞字典管理）——
const transactionCounts = ref(new Map<string, number>())

async function loadTransactionCounts() {
  try {
    const rows = await api.listMerchantTransactionCounts()
    transactionCounts.value = new Map(rows.map((r) => [r.merchant_id, r.transaction_count]))
  } catch {
    /* 条数加载失败静默保留旧值（展示 0 优于阻塞字典管理） */
  }
}

// 拉取接缝单点：立即执行一次（初始拉取）+ 参考数据失效重拉（新建/改名/软删商户后）
// 时伴随重拉计数，两条路径收敛在同一个 watch 上
watch(
  () => reference.version,
  () => {
    void loadTransactionCounts()
  },
  { immediate: true },
)

/** 行视图模型：商户行 + 客户端拼接的条数（缺失补 0）；在用与已删同构。 */
function toRow(m: Merchant): MerchantRow {
  return { ...m, transactionCount: transactionCounts.value.get(m.id) ?? 0 }
}

/** 列表行视图模型：参考数据单一来源的在用商户行。 */
const rows = computed<MerchantRow[]>(() => reference.merchants.map(toRow))

// —— 显示已删（issue #447）：默认只显示在用商户；切换后已软删商户以只读行
// 追加在尾部展示（无编辑/删除操作），条数照常显示、照常可下钻。已删字典
// 消费参考 store 既有软删缓存（历史交易口径同一数据源），无新增拉取。
const showDeleted = ref(false)

/** 已删行：软删商户同样拼接条数（照常计数、可下钻）。默认（名称序）展示在
 * 在用行之后；条数列排序激活后由表格排序接管，不另行隔离已删行。 */
const deletedRows = computed<MerchantRow[]>(() =>
  [...reference.deletedMerchants.values()].map(toRow),
)

// —— 搜索（issue #447）：统一模糊搜索语义（全库唯一定义点为核心交易域
// TransactionSearch，ADR-0027），复用拼音过滤工具的前端同规格纯函数；
// 商户字典前端全量驻留，属本地过滤形态（拼音可搜下拉同款）。searchTerm
// 过滤只隐藏未命中项、剩余项顺序不变（保护位置记忆），清空恢复完整列表。
const searchTerm = ref('')

/** 展示行：（显示已删？在用 + 已删：仅在用）→ 搜索词过滤
 *（matchLabel 空输入恒命中，清空即完整列表；filter 保序不重排）。
 * 表格以此全量集合作 data，排序与分页切片由表格客户端模式自行完成。 */
const displayRows = computed<MerchantRow[]>(() => {
  const base = showDeleted.value ? [...rows.value, ...deletedRows.value] : rows.value
  return base.filter((m) => matchLabel(searchTerm.value, m.name))
})

// —— 前端分页（issue #457）：组件内受控状态，不持久化；页签卸载重挂即回第一页 ——
const PAGE_SIZE_OPTIONS = [10, 20, 50, 100]
const currentPage = ref(1)
const pageSize = ref(50)

/** 「本页删后剩 N 条」（ADR-0045 删尾回退判定，就地声明）：按展示集合事实——
 * 显示已删关闭时软删行离开展示集合，删前本页仅 1 条 ⇔ 剩 0；开启时行仍在
 * （转已删行），集合不减。页码有效时本页条数 = 当前页起点到过滤后列表末尾的
 * 行数（≤ pageSize，排序不影响行数）。 */
function remainingOnPageAfterDelete(): number {
  const start = (currentPage.value - 1) * pageSize.value
  const rowsOnPage = Math.max(0, displayRows.value.length - start)
  return showDeleted.value ? rowsOnPage : rowsOnPage - 1
}

/** 页码回退入口（ADR-0045 同形）：N 为 0 且当前页非第一页时减一页；
 * 只回退不归零，与「过滤意图归零」是两类语义。 */
function afterRowDelete(remainingOnPage: number) {
  if (remainingOnPage === 0 && currentPage.value > 1) {
    currentPage.value -= 1
  }
}

// 过滤意图变化 → 页码归零：搜索输入/清空、切换「显示已删」；排序变化经
// update:sorter 就地归零。新增/改名等参考数据重拉不是过滤意图，保持当前页。
watch([searchTerm, showDeleted], () => {
  currentPage.value = 1
})

/** 排序切换（表格内部非受控排序作用于过滤后全量行，先排序后切片）→ 页码归零。 */
function resetPageOnSort() {
  currentPage.value = 1
}

/** 分页条（与交易页同形态：过滤后总数 + 页大小选择 + 快捷跳页）；非 remote
 * 模式下 itemCount 由表格按过滤后数据长度自动推导，prefix 直接消费。 */
const pagination = computed<PaginationProps>(() => ({
  page: currentPage.value,
  pageSize: pageSize.value,
  showSizePicker: true,
  showQuickJumper: true,
  pageSizes: PAGE_SIZE_OPTIONS,
  prefix: ({ itemCount }) =>
    h('span', null, () => t('settings.merchants.total', { n: itemCount ?? 0 })),
  onChange: (p: number) => {
    currentPage.value = p
  },
  onUpdatePageSize: (size: number) => {
    // 页大小切换与交易页同语义：写入后回第一页
    pageSize.value = size
    currentPage.value = 1
  },
}))

// —— 新增 ——
const name = ref('')

async function addMerchant() {
  const trimmed = name.value.trim()
  if (!trimmed) {
    message.warning(t('settings.merchants.msg.nameRequired'))
    return
  }
  const input: MerchantInput = { name: trimmed }
  try {
    await api.createMerchant(input)
    message.success(t('settings.merchants.msg.added'))
    name.value = ''
  } catch (e) {
    // 重名错误（「商户已存在: X」）原样上抛展示，表单不清空、用户可直接修正
    message.error(t('settings.merchants.msg.addFailed', { msg: e }))
  }
}

// —— 编辑 ——
const showEditModal = ref(false)
const editingMerchant = ref<Merchant | null>(null)

function openEdit(m: Merchant) {
  editingMerchant.value = m
  showEditModal.value = true
}

// —— 删除（软删：历史引用照常显示，不可再被新交易选择） ——
async function removeMerchant(id: string) {
  try {
    await api.deleteMerchant(id)
    // 删尾页码回退（issue #457，ADR-0045 先例）：判定用删除前状态，重拉未到。
    afterRowDelete(remainingOnPageAfterDelete())
    message.success(t('settings.merchants.msg.deleted'))
  } catch (e) {
    message.error(t('settings.merchants.msg.deleteFailed', { msg: e }))
  }
}

// —— 列表 ——
const columns: DataTableColumn<MerchantRow>[] = [
  {
    // 已删行带「已删除」标记（issue #447）：与在用行可区分。
    title: () => t('settings.merchants.columns.name'),
    key: 'name',
    width: 200,
    ellipsis: { tooltip: true },
    render: (m) =>
      m.is_deleted
        ? h(NSpace, { size: 'small', align: 'center', wrap: false }, () => [
            h('span', m.name),
            h(NTag, { size: 'small', bordered: false }, () => t('settings.merchants.deletedTag')),
          ])
        : m.name,
  },
  {
    // 关联交易条数（issue #445）：毛笔数、可排序；展示走数字分组口径（数量列）。
    // 点击条数下钻（issue #446）：文字按钮跳转交易列表并携带商户过滤参数，
    // title 与 MerchantLink 同源（common.link.viewMerchant）。
    title: () => t('settings.merchants.columns.transactionCount'),
    key: 'transactionCount',
    width: 110,
    sorter: (a, b) => a.transactionCount - b.transactionCount,
    render: (m) =>
      h(
        NButton,
        {
          text: true,
          type: 'primary',
          title: t('common.link.viewMerchant'),
          onClick: () => goMerchantTransactions(m),
        },
        () => formatQuantity(m.transactionCount),
      ),
  },
  {
    title: () => t('settings.merchants.columns.actions'),
    key: 'actions',
    width: 140,
    // 已删行只读（issue #447）：无编辑/删除操作。
    render: (m) =>
      m.is_deleted
        ? null
        : h(NSpace, { size: 'small' }, () => [
            h(
              NButton,
              { size: 'tiny', quaternary: true, type: 'primary', onClick: () => openEdit(m) },
              () => t('settings.merchants.rowActions.edit'),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => removeMerchant(m.id) },
              {
                default: () => t('settings.merchants.deleteConfirm'),
                trigger: () =>
                  h(
                    NButton,
                    { size: 'tiny', type: 'error', quaternary: true },
                    () => t('settings.merchants.rowActions.delete'),
                  ),
              },
            ),
          ]),
  },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.merchants.addTitle')" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem :label="t('settings.merchants.form.name')">
          <NInput v-model:value="name" :placeholder="t('settings.merchants.form.namePlaceholder')" style="width: 160px" />
        </NFormItem>
        <NButton type="primary" @click="addMerchant">{{ t('settings.merchants.form.add') }}</NButton>
      </NForm>
    </NCard>

    <NCard :title="t('settings.merchants.listTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center">
          <NInput
            v-model:value="searchTerm"
            clearable
            :placeholder="t('settings.merchants.searchPlaceholder')"
            style="width: 240px"
          />
          <NCheckbox v-model:checked="showDeleted">
            {{ t('settings.merchants.showDeleted') }}
          </NCheckbox>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="displayRows"
          :bordered="false"
          size="small"
          :row-key="(m: MerchantRow) => m.id"
          :pagination="pagination"
          @update:sorter="resetPageOnSort"
        />
      </NSpace>
    </NCard>

    <MerchantEditModal v-model:show="showEditModal" :merchant="editingMerchant" />
  </NSpace>
</template>
