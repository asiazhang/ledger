<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NDataTable,
  NDatePicker,
  NEmpty,
  NInput,
  NSpace,
  NText,
  useMessage,
} from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { buildTransactionColumns, sumFixedColumnWidths } from '@/components/transaction-columns'
import { formatAmount, type Transaction, type TransactionSearchFilter } from '@/types'
import { yuanToCents } from '@/utils/money'

const store = useAppStore()
const reference = useReferenceStore()
const message = useMessage()

const keyword = ref('')
// 金额筛选：用户以「元」输入（支持小数），内部转分后传后端
const amountMinYuan = ref('')
const amountMaxYuan = ref('')
// 日期筛选：NDatePicker value-format 直接绑定 YYYY-MM-DD 字符串
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

/** 当前筛选条件的可读描述（供「已应用筛选」展示） */
const activeFilterDescriptions = computed(() => {
  const parts: string[] = []
  // 按用户默认币种展示符号（设置页可改），避免硬编码 CNY
  const currency = reference.getCurrency(store.defaultCurrency)
  const min = amountMinCents.value
  const max = amountMaxCents.value
  if (min !== null && max !== null) {
    parts.push(`金额 ${formatAmount(min, currency)} ~ ${formatAmount(max, currency)}`)
  } else if (min !== null) {
    parts.push(`最低 ${formatAmount(min, currency)}`)
  } else if (max !== null) {
    parts.push(`最高 ${formatAmount(max, currency)}`)
  }
  if (dateFrom.value) parts.push(`起始 ${dateFrom.value}`)
  if (dateTo.value) parts.push(`结束 ${dateTo.value}`)
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
    message.error(`搜索失败: ${errorMessage(e)}`)
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

// 复用交易列表列配置（日期/类型/分类/账户/备注/金额），结果只读
const columns: DataTableColumn<Transaction>[] = buildTransactionColumns(reference)

// scroll-x：列中所有固定列（有 width 的列，备注为弹性列不计入）宽度总和
const scrollX = sumFixedColumnWidths(columns)

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
      placeholder="输入关键字开始搜索（备注、账户名、拼音首字母，支持多关键字）"
      clearable
      @keyup.enter="onEnter"
    />
    <NSpace :size="8" align="center" :wrap="true">
      <NInput
        v-model:value="amountMinYuan"
        placeholder="最低金额（元）"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <NInput
        v-model:value="amountMaxYuan"
        placeholder="最高金额（元）"
        clearable
        style="width: 150px"
        @keyup.enter="onEnter"
      />
      <NDatePicker
        v-model:formatted-value="dateFrom"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="起始日期"
        clearable
        style="width: 140px"
      />
      <NDatePicker
        v-model:formatted-value="dateTo"
        type="date"
        value-format="yyyy-MM-dd"
        placeholder="结束日期"
        clearable
        style="width: 140px"
      />
      <template v-if="filtersActive">
        <NText depth="3">已应用筛选：{{ activeFilterDescriptions.join('、') }}</NText>
        <NButton size="tiny" quaternary type="primary" @click="clearFilters">
          清除筛选
        </NButton>
      </template>
    </NSpace>
    <template v-if="searched">
      <NText depth="3">命中 {{ total }} 条</NText>
      <NEmpty v-if="total === 0" description="无匹配结果" />
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
    <NEmpty v-else description="输入关键字或设置筛选开始搜索" />
  </NSpace>
</template>
