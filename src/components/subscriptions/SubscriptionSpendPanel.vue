<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NCard, NDataTable, NEmpty, NSpace, NSpin, useMessage, type DataTableColumns } from 'naive-ui'
import { Bar } from 'vue-chartjs'
import { BarElement, CategoryScale, Chart as ChartJS, LinearScale, Tooltip } from 'chart.js'
import type { ChartOptions, TooltipItem } from 'chart.js'
import { api } from '@/api'
import { formatAmount } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import { scheduledStatusLabel } from '@/utils/scheduled'
import type { SubscriptionSpendOverview, SubscriptionSpendRow } from '@/types'

// 订阅花费「实际口径」分析区（issue #160，ADR-0023 决策二）：
// 本月/本年实际花费 + 过去 12 个月逐月趋势（不摊销，忠实统计期次生成的流水）。
// 推算成本口径由后续迭代接入；数据全部来自只读聚合命令 subscription_spend_overview。

const reference = useReferenceStore()
const message = useMessage()

const overview = ref<SubscriptionSpendOverview | null>(null)
const loading = ref(false)
const loadFailed = ref(false)

async function reload() {
  loading.value = true
  loadFailed.value = false
  try {
    overview.value = await api.subscriptionSpendOverview()
  } catch (e) {
    // 后端缺汇率等中文错误直接上抛展示，不静默混算（ADR-0023）
    loadFailed.value = true
    message.error(`加载订阅花费失败: ${e}`)
  } finally {
    loading.value = false
  }
}
defineExpose({ reload })
onMounted(reload)

const currency = computed(() =>
  overview.value ? reference.getCurrency(overview.value.native_currency) : undefined,
)

/** 趋势图数据：x 轴 12 个日历月（YYYY-MM），y 轴本位币金额（分） */
const chartData = computed(() => ({
  labels: overview.value?.months.map((m) => m.month) ?? [],
  datasets: [
    {
      label: '实际花费',
      data: overview.value?.months.map((m) => m.native_cents) ?? [],
      backgroundColor: 'rgba(32, 128, 240, 0.55)',
      borderRadius: 4,
    },
  ],
}))

const chartOptions: ChartOptions<'bar'> = {
  responsive: true,
  maintainAspectRatio: false,
  plugins: {
    legend: { display: false },
    tooltip: {
      callbacks: {
        label: (context: TooltipItem<'bar'>) =>
          formatAmount(context.raw as number, currency.value),
      },
    },
  },
  scales: {
    x: { ticks: { maxRotation: 0, autoSkip: true, maxTicksLimit: 12 } },
    y: {
      ticks: {
        callback: (value: number | string) => formatAmount(Number(value), currency.value),
      },
    },
  },
}

ChartJS.register(BarElement, CategoryScale, LinearScale, Tooltip)

/** 逐订阅行（含已取消/暂停计划，历史花费如实保留） */
const rows = computed(() => overview.value?.rows ?? [])

const rowColumns: DataTableColumns<SubscriptionSpendRow> = [
  {
    title: '订阅',
    key: 'note',
    render: (row) => row.note ?? row.counterparty ?? '—',
  },
  {
    title: '状态',
    key: 'status',
    render: (row) => scheduledStatusLabel(row.status),
  },
  {
    title: '本月实际花费',
    key: 'this_month',
    align: 'right',
    render: (row) => formatAmount(row.this_month_native_cents, currency.value),
  },
  {
    title: '本年实际花费',
    key: 'this_year',
    align: 'right',
    render: (row) => formatAmount(row.this_year_native_cents, currency.value),
  },
]
</script>

<template>
  <NCard title="实际花费" size="small">
    <template #header-extra>
      <span class="spend-caption">
        {{ overview ? `单位：${overview.native_currency}（本位币）· 不摊销` : '' }}
      </span>
    </template>
    <NSpin :show="loading">
      <NEmpty
        v-if="loadFailed"
        description="加载失败，请重试"
        data-testid="spend-failed"
      />
      <NSpace v-else-if="overview" vertical :size="12">
        <NSpace :size="48" align="center">
          <div class="spend-stat">
            <div class="spend-stat-label">本月实际花费</div>
            <div class="spend-stat-value" data-testid="spend-this-month">
              {{ formatAmount(overview.this_month_native_cents, currency) }}
            </div>
          </div>
          <div class="spend-stat">
            <div class="spend-stat-label">本年实际花费</div>
            <div class="spend-stat-value" data-testid="spend-this-year">
              {{ formatAmount(overview.this_year_native_cents, currency) }}
            </div>
          </div>
        </NSpace>
        <!-- 测试锚点：趋势图数据经桩组件序列化断言（jsdom 无 canvas），桩根节点 data-testid="bar-chart" -->
        <div class="spend-chart-box">
          <Bar :data="chartData" :options="chartOptions" />
        </div>
        <NDataTable
          :columns="rowColumns"
          :data="rows"
          size="small"
          :bordered="false"
          :row-key="(row: SubscriptionSpendRow) => row.plan_id"
          data-testid="spend-rows"
        />
      </NSpace>
    </NSpin>
  </NCard>
</template>

<style scoped>
.spend-caption {
  font-size: 12px;
  color: var(--n-text-color-disabled, #999);
}
.spend-stat-label {
  font-size: 12px;
  color: var(--n-text-color-disabled, #999);
}
.spend-stat-value {
  font-size: 22px;
  font-weight: 600;
}
.spend-chart-box {
  height: 220px;
}
</style>
