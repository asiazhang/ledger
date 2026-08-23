<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NDataTable, NEmpty, NInput, NSpace, NText, useMessage } from 'naive-ui'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { buildTransactionColumns } from '@/components/transactionColumns'
import type { Transaction } from '@/types'

const store = useAppStore()
const message = useMessage()

const keyword = ref('')
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

async function runSearch() {
  const seq = ++searchSeq
  loading.value = true
  try {
    const res = await api.searchTransactions(keyword.value.trim(), page.value, pageSize)
    if (seq !== searchSeq) return
    results.value = res.items
    total.value = res.total
    searched.value = true
  } catch (e) {
    if (seq !== searchSeq) return
    message.error(`搜索失败: ${e}`)
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

// 空输入只显示占位提示，不触发查询
watch(keyword, (value) => {
  if (!value.trim()) {
    resetResults()
    return
  }
  scheduleSearch()
})

// 回车立即搜索（不等防抖）
function onEnter() {
  if (!keyword.value.trim()) return
  clearTimeout(debounceTimer)
  page.value = 1
  runSearch()
}

// 复用交易列表列配置（日期/类型/分类/账户/备注/金额），结果只读
const columns: DataTableColumn<Transaction>[] = buildTransactionColumns(store)

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

onMounted(async () => {
  await store.loadAll()
})
</script>

<template>
  <NSpace vertical :size="12">
    <NInput
      v-model:value="keyword"
      placeholder="输入关键字开始搜索（备注、账户名、拼音首字母，支持多关键字）"
      clearable
      @keyup.enter="onEnter"
    />
    <template v-if="searched">
      <NText depth="3">命中 {{ total }} 条</NText>
      <NEmpty v-if="total === 0" description="无匹配结果" />
      <NDataTable
        v-else
        :columns="columns"
        :data="results"
        :loading="loading"
        :bordered="false"
        size="small"
        remote
        :pagination="pagination"
      />
    </template>
    <NEmpty v-else description="输入关键字开始搜索" />
  </NSpace>
</template>
