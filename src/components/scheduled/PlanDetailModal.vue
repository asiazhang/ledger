<script setup lang="ts">
import { computed, h, ref } from 'vue'
import { NButton, NDataTable, NEmpty, NModal, NSpace, NSpin, useMessage, type DataTableColumns } from 'naive-ui'
import { formatAmount } from '@/types'
import { errorMessage } from '@/utils/errors'
import { occurrenceStatusLabel } from '@/utils/scheduled'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import type { ScheduledTransactionDetail, ScheduledTransactionOccurrence } from '@/types'

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：订阅 / 分期 / 定时转账三页签通用。
// 数据走既有计划详情命令（含期次列表）；操作仅两个——
// - 失败期次「重试」：走既有单期执行命令，仅对 failed 状态开放（不做提前执行 pending）；
// - 「展开更多期次」：走既有期次展开命令，查看预生成窗口之外的更远日期。
// ---------------------------------------------------------------------------

const reference = useReferenceStore()
const message = useMessage()

const emit = defineEmits<{
  /** 重试成功后期次状态与生成交易已变化，通知清单刷新 */
  (e: 'changed'): void
}>()

const show = ref(false)
const planId = ref<string | null>(null)
const loading = ref(false)
const loadFailed = ref(false)
const detail = ref<ScheduledTransactionDetail | null>(null)
/** 正在重试的期次 id（防重复点击） */
const retryingId = ref<string | null>(null)
/** 正在展开期次（防重复点击） */
const expanding = ref(false)

/** 对外入口：打开弹窗并拉取计划详情。 */
async function open(id: string) {
  planId.value = id
  show.value = true
  await load()
}

async function load() {
  if (!planId.value) return
  loading.value = true
  loadFailed.value = false
  try {
    detail.value = await api.getScheduledTransactionDetail(planId.value)
  } catch {
    detail.value = null
    loadFailed.value = true
  } finally {
    loading.value = false
  }
}

/** 期次合并视图：全量期次（含 pending/processing/completed/failed/cancelled）按日期升序。 */
const occurrenceRows = computed<ScheduledTransactionOccurrence[]>(() => {
  const d = detail.value
  if (!d) return []
  return [...d.occurrences].sort(
    (a, b) =>
      a.scheduled_date.localeCompare(b.scheduled_date) || a.created_at.localeCompare(b.created_at),
  )
})

/** 计划总期数：分期/有限期数定时转账有值；订阅与无限循环为 null。 */
const totalOccurrences = computed<number | null>(() => {
  const ext = detail.value?.extension
  if (!ext) return null
  return 'total_occurrences' in ext ? (ext.total_occurrences ?? null) : null
})

/**
 * 「展开更多期次」门控：仅 active 计划可展开（后端同口径）；有限期数计划
 * 期次已全部生成（含历史各状态，与后端展开的 existing_count 同口径）时不再
 * 显示——避免无意义的空展开。
 */
const canExpand = computed(() => {
  const d = detail.value
  if (!d) return false
  if (d.core.status !== 'active') return false
  if (totalOccurrences.value === null) return true
  return occurrenceRows.value.length < totalOccurrences.value
})

/** 重试门控（验收）：仅 failed 期次提供重试入口。 */
function canRetry(occ: ScheduledTransactionOccurrence): boolean {
  return occ.status === 'failed'
}

async function retry(occ: ScheduledTransactionOccurrence) {
  if (!canRetry(occ) || retryingId.value) return
  retryingId.value = occ.id
  try {
    await api.executeScheduledOccurrence({ occurrence_id: occ.id })
    message.success('重试成功，交易已入账')
    await load()
    emit('changed')
  } catch (e) {
    message.error(`重试失败: ${errorMessage(e)}`)
  } finally {
    retryingId.value = null
  }
}

async function expand() {
  const d = detail.value
  if (!d || !canExpand.value || expanding.value) return
  expanding.value = true
  try {
    const ids = await api.expandScheduledOccurrences(d.core.id)
    await load()
    message.success(ids.length > 0 ? `已生成 ${ids.length} 期` : '没有更多期次了')
  } catch (e) {
    message.error(`展开失败: ${errorMessage(e)}`)
  } finally {
    expanding.value = false
  }
}

const columns: DataTableColumns<ScheduledTransactionOccurrence> = [
  {
    title: '日期',
    key: 'scheduled_date',
    render: (occ) =>
      h('span', { 'data-testid': `occ-date-${occ.id}` }, occ.scheduled_date),
  },
  {
    title: '金额',
    key: 'amount',
    render: (occ) => {
      // 期次不带币种，用计划的币种（amount 列仅在 detail 渲染时出现）
      const currency = reference.getCurrency(detail.value!.core.currency_code)
      return formatAmount(occ.amount_cents, currency)
    },
  },
  {
    title: '状态',
    key: 'status',
    render: (occ) =>
      h(
        'span',
        { 'data-testid': `occ-status-${occ.id}` },
        occurrenceStatusLabel(occ.status as ScheduledTransactionOccurrence['status']),
      ),
  },
  {
    title: '操作',
    key: 'actions',
    // 重试按钮状态门控：仅 failed 期次渲染重试入口
    render: (occ) =>
      canRetry(occ)
        ? h(
            NButton,
            {
              size: 'tiny',
              type: 'primary',
              quaternary: true,
              loading: retryingId.value === occ.id,
              'data-testid': `occ-retry-${occ.id}`,
              onClick: () => retry(occ),
            },
            () => '重试',
          )
        : '—',
  },
]

defineExpose({ open })
</script>

<template>
  <NModal
    v-model:show="show"
    title="期次详情"
    preset="card"
    display-directive="if"
    style="width: 560px"
    :bordered="false"
  >
    <NSpin :show="loading">
      <div v-if="loadFailed" data-testid="occ-load-failed">加载失败</div>
      <template v-else-if="detail">
        <NSpace vertical :size="8" class="plan-summary">
          <span data-testid="occ-plan-note">{{ detail.core.note ?? '（无备注）' }}</span>
          <span v-if="totalOccurrences !== null" class="plan-total">
            共 {{ totalOccurrences }} 期
          </span>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="occurrenceRows"
          :bordered="false"
          size="small"
          :row-key="(occ: ScheduledTransactionOccurrence) => occ.id"
          :max-height="400"
        />
        <NEmpty
          v-if="occurrenceRows.length === 0"
          description="暂无期次"
          data-testid="occ-empty"
        />
        <NSpace v-if="canExpand" justify="center" :size="8" style="margin-top: 12px">
          <NButton
            size="small"
            :loading="expanding"
            data-testid="occ-expand"
            @click="expand"
          >
            展开更多期次
          </NButton>
        </NSpace>
      </template>
    </NSpin>
  </NModal>
</template>

<style scoped>
.plan-summary {
  margin-bottom: 8px;
  font-size: 13px;
  color: var(--n-text-color-2, #666);
}
</style>
