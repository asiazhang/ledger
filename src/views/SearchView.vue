<script setup lang="ts">
import { t } from '@ledger/i18n'
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NDataTable,
  NEmpty,
  NInput,
  NPagination,
  NSpace,
  NSpin,
  NText,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import QuickTimeRange from '@/components/QuickTimeRange.vue'
import TransactionCardList from '@/components/TransactionCardList.vue'
import { useWindowTier } from '@/composables/useWindowTier'
import { useLoadable } from '@/composables/useLoadable'
import { api } from '@ledger/api'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { buildTransactionColumns } from '@/components/transaction-columns'
import { sumFixedColumnWidths } from '@/utils/table'
import { type Transaction, type TransactionSearchFilter } from '@ledger/types'
import type { NullableDateRange } from '@/utils/time-period'
import { yuanToCents, formatAmount } from '@ledger/money'

const store = useAppStore()
const reference = useReferenceStore()
// 窗口分级（ADR-0088 决策 9）：搜索结果断点双渲染——移动档同构复用交易卡片列表
// （第二消费方，只读形态：无「⋯」、无整卡编辑，与桌面搜索结果只读口径一致）
const tier = useWindowTier()
const isMobile = computed(() => tier.value === 'mobile')

const keyword = ref('')
// 金额筛选：用户以「元」输入（支持小数），内部转分后传后端
const amountMinYuan = ref('')
const amountMaxYuan = ref('')
// 日期筛选：由时间范围快捷选择写入的期间边界快照（YYYY-MM-DD 双端有界，
// 「全部」= 双空 = 默认态）；不持第二状态源，唯一事实源仍是本视图本地日期条件
const dateFrom = ref<string | null>(null)
const dateTo = ref<string | null>(null)
const results = ref<Transaction[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 20
// 是否已完成至少一次搜索（区分「占位提示」与「空结果」两种空态）
const searched = ref(false)

let debounceTimer: ReturnType<typeof setTimeout> | undefined

// 搜索加载收编 Loadable（issue #1008 / ADR-0040）：loading 置收、竞态后发覆盖先发
// 与错误 toast（默认策略 = 裸 errorMessage）内化；任务只产结果不写状态。
const { loading, run: runSearchTask, invalidate } = useLoadable(() =>
  api.searchTransactions(keyword.value.trim(), page.value, pageSize, buildFilter()),
)

const amountMinCents = computed(() => yuanToCents(amountMinYuan.value))
const amountMaxCents = computed(() => yuanToCents(amountMaxYuan.value))

// 时间范围快捷选择（issue #526 / ADR-0070，消费形态三）：搜索页时间控件唯一形态——
// 五枚芯片（全部 | 当月 | 当季 | 当年 | 去年，缺省预设零配置）＋期间步进器＋期间直达
// 面板，随组件整体继承快照语义、游标派生与数据期间边界钳制。受控桥接与交易页同构：
// 快照区间 v-model 进出，组件不持状态源，唯一事实源是本地日期条件（dateFrom/dateTo
// 成对写入）；防抖自动搜索与「清除筛选」清回「全部」由既有 watcher/clearFilters 免费
// 继承。两个独立日期选择器（任意起止/可单边）随本次接入退役，后端单边可选语义照旧
// 冻结并存（比照 ADR-0057 遗留参数，新代码不使用单边语义）。
const quickRange = computed<NullableDateRange>({
  get: () => ({ from: dateFrom.value, to: dateTo.value }),
  set: (range) => {
    // 无条件成对写入：组件产出闭集只有双端有界或双空（「全部」须能清回默认态，
    // 不能像报表页那样拒绝双空）；单端 null 不在产出闭集内，无需双端有界守卫。
    dateFrom.value = range.from
    dateTo.value = range.to
  },
})

/** 是否有激活的筛选条件（金额任一边或日期任一端非空） */
const filtersActive = computed(
  () =>
    amountMinCents.value !== null ||
    amountMaxCents.value !== null ||
    !!dateFrom.value ||
    !!dateTo.value,
)

/** 是否具备查询条件：关键字非空或筛选激活（仅筛选也可出结果） */
const hasQuery = computed(() => keyword.value.trim() !== '' || filtersActive.value)

/** 当前筛选条件的可读描述（供「已应用筛选」展示，文案随语言切换）。 */
const activeFilterDescriptions = computed(() => {
  const parts: string[] = []
  // 按用户默认币种展示符号（设置页可改），避免硬编码 CNY
  const currency = reference.getCurrency(store.defaultCurrency)
  const min = amountMinCents.value
  const max = amountMaxCents.value
  if (min !== null && max !== null) {
    parts.push(
      t('search.filter.amountRange', {
        min: formatAmount(min, currency),
        max: formatAmount(max, currency),
      }),
    )
  } else if (min !== null) {
    parts.push(t('search.filter.amountMin', { amount: formatAmount(min, currency) }))
  } else if (max !== null) {
    parts.push(t('search.filter.amountMax', { amount: formatAmount(max, currency) }))
  }
  if (dateFrom.value) parts.push(t('search.filter.dateFrom', { date: dateFrom.value }))
  if (dateTo.value) parts.push(t('search.filter.dateTo', { date: dateTo.value }))
  return parts
})

function buildFilter(): TransactionSearchFilter {
  return {
    amountMinCents: amountMinCents.value,
    amountMaxCents: amountMaxCents.value,
    dateFrom: dateFrom.value || null,
    dateTo: dateTo.value || null,
  }
}

async function runSearch() {
  const res = await runSearchTask()
  if (res === null) return
  results.value = res.items
  total.value = res.total
  searched.value = true
}

function scheduleSearch() {
  clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    page.value = 1
    runSearch()
  }, 300)
}

function resetResults() {
  clearTimeout(debounceTimer)
  invalidate() // 作废在途请求：迟到结果不落位、loading 收尾
  results.value = []
  total.value = 0
  page.value = 1
  searched.value = false
}

// 关键字或筛选任一变化：空查询（无关键字且无筛选）→ 占位；否则防抖查询
watch([keyword, amountMinYuan, amountMaxYuan, dateFrom, dateTo], () => {
  if (!hasQuery.value) {
    resetResults()
    return
  }
  scheduleSearch()
})

// 回车立即搜索（不等防抖），关键字或筛选任一存在即可
function onEnter() {
  if (!hasQuery.value) return
  clearTimeout(debounceTimer)
  page.value = 1
  runSearch()
}

function clearFilters() {
  amountMinYuan.value = ''
  amountMaxYuan.value = ''
  dateFrom.value = null
  dateTo.value = null
}

// 复用交易列表列配置（日期/类型/分类/账户/备注/金额），结果只读；
// 经 computed 构造：列名（t()）随语言切换即时重建
const columns = computed<DataTableColumn<Transaction>[]>(() => buildTransactionColumns(reference))

// scroll-x：列中所有固定列（有 width 的列，备注为弹性列不计入）宽度总和
const scrollX = computed(() => sumFixedColumnWidths(columns.value))

// 服务端分页：翻页时携带 page 重新搜索（移动档 NPagination 与桌面表格同一回调）
function onPageChange(p: number): void {
  page.value = p
  void runSearch()
}

const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: onPageChange,
}))
</script>

<template>
  <NSpace vertical :size="12">
    <NInput
      v-model:value="keyword"
      :placeholder="t('search.keywordPlaceholder')"
      clearable
      @keyup.enter="onEnter"
    />
    <!-- 时间范围快捷选择行（issue #526 / ADR-0070）：搜索页唯一时间控件——
         芯片「全部 | 当月 | 当季 | 当年 | 去年」＋期间步进器＋期间直达面板整行由
         QuickTimeRange 渲染；快照区间 v-model 进出（唯一事实源是本地日期条件），
         防抖自动搜索与清除筛选回「全部」由既有链路继承（交易页时间维度行同构）。 -->
    <QuickTimeRange v-model="quickRange" />
    <NSpace :size="8" align="center" :wrap="true">
      <NInput
        v-model:value="amountMinYuan"
        :placeholder="t('search.amountMinPlaceholder')"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <NInput
        v-model:value="amountMaxYuan"
        :placeholder="t('search.amountMaxPlaceholder')"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <template v-if="filtersActive">
        <NText depth="3">{{
          t('search.appliedFilters', { filters: activeFilterDescriptions.join(t('search.filterSeparator')) })
        }}</NText>
        <NButton size="tiny" quaternary type="primary" @click="clearFilters">
          {{ t('search.clearFilters') }}
        </NButton>
      </template>
    </NSpace>
    <template v-if="searched">
      <NText depth="3">{{ t('search.hitCount', { n: total }) }}</NText>
      <NEmpty v-if="total === 0" :description="t('search.noResults')" />
      <!-- 移动档（issue #846）：同构复用交易卡片列表（只读：不传行菜单/整卡回调），
           分页语义不变（翻页重新搜索） -->
      <template v-else-if="isMobile">
        <NSpin :show="loading">
          <TransactionCardList :rows="results" />
        </NSpin>
        <NPagination
          :page="page"
          :page-size="pageSize"
          :item-count="total"
          @update:page="onPageChange"
        />
      </template>
      <!-- 备注列为弹性列，表格铺满容器；窄窗口时备注先收缩，scroll-x（固定列宽总和）作为横向滚动下限 -->
      <NDataTable
        v-else
        :columns="columns"
        :data="results"
        :loading="loading"
        :bordered="false"
        size="small"
        remote
        :scroll-x="scrollX"
        :pagination="pagination"
      />
    </template>
    <NEmpty v-else :description="t('search.emptyPlaceholder')" />
  </NSpace>
</template>
