<script setup lang="ts">
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NDataTable,
  NEmpty,
  NInput,
  NSpace,
  NText,
  useMessage,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import QuickTimeRange from '@/components/QuickTimeRange.vue'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transaction-columns'
import { formatAmount, type Transaction, type TransactionSearchFilter } from '@/types'
import type { NullableDateRange } from '@/utils/time-period'
import { yuanToCents } from '@/utils/money'

const store = useAppStore()
const reference = useReferenceStore()
const message = useMessage()

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
const loading = ref(false)
// 是否已完成至少一次搜索（区分「占位提示」与「空结果」两种空态）
const searched = ref(false)

let debounceTimer: ReturnType<typeof setTimeout> | undefined
// 请求序号：丢弃过期响应，防快速输入/清空下的竞态
let searchSeq = 0

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
  const seq = ++searchSeq
  loading.value = true
  try {
    const res = await api.searchTransactions(
      keyword.value.trim(),
      page.value,
      pageSize,
      buildFilter(),
    )
    if (seq !== searchSeq) return
    results.value = res.items
    total.value = res.total
    searched.value = true
  } catch (e) {
    if (seq !== searchSeq) return
    message.error(t('search.searchFailed', { msg: errorMessage(e) }))
  } finally {
    if (seq === searchSeq) loading.value = false
  }
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
  searchSeq++ // 使在途请求过期
  results.value = []
  total.value = 0
  page.value = 1
  searched.value = false
  loading.value = false
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

// 服务端分页：翻页时携带 page 重新搜索
const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: (p: number) => {
    page.value = p
    runSearch()
  },
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
