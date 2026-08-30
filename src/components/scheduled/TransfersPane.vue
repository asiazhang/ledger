<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, h, onMounted, ref, watch, type VNode } from 'vue'
import {
  NCard,
  NButton,
  NButtonGroup,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSpace,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import AppSelect from '@/components/AppSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import type {
  RecurrenceType,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  UpdateStatusInput,
} from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 定时转账页签（issue #203）：定时视图三页签之一。
 * 转出 / 转入账户必须同币种（词汇表 ScheduledTransfer 边界，后端行为层强制）：
 * 转入账户候选按转出账户币种过滤，币种自动跟随转出账户；不出现商户字段。
 * 支持「总期数」三态：留空 = 无限循环、1 = 一次性、N = 有限期数。
 */

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 新建定时转账 = 模态对话框（与订阅页签同模式）。
// 表单接缝（ADR-0041）：公共草稿字段与公共 payload 组装全仓单点——转出账户即
// 接缝的「账户」字段；转入账户过滤、币种跟随与总期数语义留本页签。
// ---------------------------------------------------------------------------

const form = useScheduledPlanForm()
const {
  note,
  accountId: fromAccountId,
  currencyCode,
  recurrenceType,
  recurrenceInterval,
  startDate,
  accountOptions,
  currencyOptions,
} = form

const showCreateModal = ref(false)

const toAccountId = ref<string | null>(null)
const amountYuan = ref('')
/** 总期数：null = 无限循环（留空），1 = 一次性，N = 有限期数 */
const totalOccurrences = ref<number | null>(null)

const recurrenceOptions = [
  { label: '天', value: 'daily' },
  { label: '周', value: 'weekly' },
  { label: '月', value: 'monthly' },
  { label: '年', value: 'yearly' },
]

const filterOptions = [
  { key: 'active' as const, label: '进行中' },
  { key: 'paused' as const, label: '已暂停' },
  { key: 'cancelled' as const, label: '已取消' },
  { key: 'completed' as const, label: '已完成' },
]

/** 转入账户候选（Vitest 验收 seam）：按转出账户币种过滤并排除转出账户本身
 * （转出 = 转入被后端拒绝）；未选转出账户时为全部账户。 */
const toAccountOptions = computed(() => {
  const from = fromAccountId.value ? reference.accountMap.get(fromAccountId.value) : undefined
  if (!from) return accountOptions.value
  return accountOptions.value.filter((o) => {
    const acc = reference.accountMap.get(o.value)
    return acc && o.value !== from.id && acc.currency_code === from.currency_code
  })
})

// 币种跟随转出账户；切换后清空币种不再匹配的转入账户选中（防跨币种提交）
watch(fromAccountId, (id) => {
  const from = id ? reference.accountMap.get(id) : undefined
  if (from) {
    currencyCode.value = from.currency_code
    if (
      toAccountId.value &&
      reference.accountMap.get(toAccountId.value)?.currency_code !== from.currency_code
    ) {
      toAccountId.value = null
    }
  }
})

/** 重置新建表单到初始态：公共字段走接缝 reset，转入账户/金额/总期数留本页签。 */
function resetCreateForm() {
  form.reset()
  toAccountId.value = null
  amountYuan.value = ''
  totalOccurrences.value = null
}

async function create() {
  if (!fromAccountId.value) {
    message.warning('请选择转出账户')
    return
  }
  if (!toAccountId.value) {
    message.warning('请选择转入账户')
    return
  }
  const amountCents = yuanToCents(amountYuan.value)
  if (amountCents === null || amountCents <= 0) {
    message.warning('请输入大于 0 的金额')
    return
  }
  try {
    await api.createScheduledTransaction(
      form.buildCreateInput({
        kind: 'scheduled_transfer',
        amountCents,
        // 定时转账不使用商户（无商户面，不暴露商户字段，issue #203）
        merchantId: null,
        specific: { to_account_id: toAccountId.value, total_occurrences: totalOccurrences.value },
      }),
    )
    message.success('已创建定时转账')
    showCreateModal.value = false
    resetCreateForm()
    await load()
  } catch (e) {
    message.error(`创建失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 清单：list_scheduled_transactions 过滤 scheduled_transfer + 状态过滤；
// 下期转账取 get_scheduled_transaction_detail 的最早 pending 期次（窗口外显示 —）
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 下期 pending 期次（无则为 null，占位「—」）。 */
interface TransferRow {
  plan: ScheduledTransactionWithExt
  next: ScheduledTransactionOccurrence | null
  /** 详情命令失败：与「无 pending 期次」区分，不静默显示「—」。 */
  nextFailed?: boolean
}

const rows = ref<TransferRow[]>([])
const loading = ref(false)
/** 清单状态过滤：默认只看进行中。 */
const statusFilter = ref<'active' | 'paused' | 'cancelled' | 'completed'>('active')

const filteredRows = computed(() =>
  rows.value.filter((r) => r.plan.core.status === statusFilter.value),
)

async function load() {
  loading.value = true
  try {
    const plans = (await api.listScheduledTransactions()).filter(
      (p) => p.core.kind === 'scheduled_transfer',
    )
    // 下期转账来自既有详情命令的 pending 期次（ASC 排序，取首条）；
    // 预生成窗口之外不现场推算日期（避免第三套日期口径）
    const details = await Promise.all(
      plans.map(async (p) => {
        try {
          const d = await api.getScheduledTransactionDetail(p.core.id)
          const next =
            [...d.pending_occurrences].sort((a, b) =>
              a.scheduled_date.localeCompare(b.scheduled_date),
            )[0] ?? null
          return { plan: p, next } satisfies TransferRow
        } catch {
          return { plan: p, next: null, nextFailed: true } satisfies TransferRow
        }
      }),
    )
    rows.value = details
  } catch (e) {
    message.error(`加载定时转账失败: ${errorMessage(e)}`)
  } finally {
    loading.value = false
  }
}

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：三页签通用组件；弹窗内重试成功会发 changed，
// 清单随之刷新
// ---------------------------------------------------------------------------

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

function openDetail(row: TransferRow) {
  void planDetailRef.value?.open(row.plan.core.id)
}

async function onDetailChanged() {
  await load()
}

// ---------------------------------------------------------------------------
// 状态操作：暂停 / 恢复 / 取消（走既有 update_scheduled_transaction_status）
// ---------------------------------------------------------------------------

async function changeStatus(id: string, newStatus: UpdateStatusInput['new_status']) {
  try {
    await api.updateScheduledTransactionStatus({ id, new_status: newStatus })
    message.success(newStatus === 'paused' ? '已暂停' : newStatus === 'active' ? '已恢复' : '已取消')
    await load()
  } catch (e) {
    message.error(`操作失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 展示助手
// ---------------------------------------------------------------------------

const recurrenceUnit: Record<RecurrenceType, string> = {
  daily: '天',
  weekly: '周',
  monthly: '月',
  yearly: '年',
}

function recurrenceLabel(row: TransferRow): string {
  const { recurrence_type, recurrence_interval } = row.plan.core
  const unit = recurrenceUnit[recurrence_type as RecurrenceType] ?? recurrence_type
  return recurrence_interval > 1 ? `每${recurrence_interval}${unit}` : `每${unit}`
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

const columns: DataTableColumns<TransferRow> = [
  {
    title: '备注',
    key: 'note',
    render: (row) => row.plan.core.note ?? '—',
  },
  {
    title: '转出账户',
    key: 'from',
    render: (row) =>
      reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id,
  },
  {
    title: '转入账户',
    key: 'to',
    render: (row) => {
      const id = row.plan.to_account_id
      return id ? (reference.accountMap.get(id)?.name ?? id) : '—'
    },
  },
  {
    title: '金额',
    key: 'amount',
    render: (row) =>
      `${formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code))}${row.plan.total_occurrences != null ? ` × ${row.plan.total_occurrences}期` : ''}`,
  },
  { title: '周期', key: 'recurrence', render: recurrenceLabel },
  { title: '开始日', key: 'start_date', render: (row) => row.plan.core.start_date },
  { title: '状态', key: 'status', render: (row) => statusLabel(row.plan.core.status) },
  {
    title: '下期转账',
    key: 'next',
    // 测试锚点：无 pending 期次（预生成窗口之外）时断言占位，不现场推算日期
    render: (row) =>
      h(
        'span',
        { 'data-testid': `next-transfer-${row.plan.core.id}` },
        row.next
          ? `${row.next.scheduled_date} · ${formatAmount(row.next.amount_cents, reference.getCurrency(row.plan.core.currency_code))}`
          : row.nextFailed
            ? '加载失败'
            : '—',
      ),
  },
  {
    title: '操作',
    key: 'actions',
    render: (row) => {
      const status = row.plan.core.status
      const buttons: VNode[] = []
      // 期次详情（issue #205）：所有状态都可查看期次执行情况
      buttons.push(
        h(
          NButton,
          {
            size: 'tiny',
            'data-testid': `op-detail-${row.plan.core.id}`,
            onClick: () => openDetail(row),
          },
          () => '期次',
        ),
      )
      if (status === 'active') {
        buttons.push(
          h(
            NButton,
            {
              size: 'tiny',
              'data-testid': `op-pause-${row.plan.core.id}`,
              onClick: () => changeStatus(row.plan.core.id, 'paused'),
            },
            () => '暂停',
          ),
        )
      }
      if (status === 'paused') {
        buttons.push(
          h(
            NButton,
            {
              size: 'tiny',
              'data-testid': `op-resume-${row.plan.core.id}`,
              onClick: () => changeStatus(row.plan.core.id, 'active'),
            },
            () => '恢复',
          ),
        )
      }
      if (status === 'active' || status === 'paused') {
        // 取消不删已生成交易与历史期次（后端行为），二次确认防误触
        buttons.push(
          h(
            AppPopconfirm,
            { onPositiveClick: () => changeStatus(row.plan.core.id, 'cancelled') },
            {
              default: () => '取消后不再自动转账，已生成的交易与历史期次保留。确认取消？',
              trigger: () =>
                h(
                  NButton,
                  {
                    size: 'tiny',
                    type: 'error',
                    quaternary: true,
                    'data-testid': `op-cancel-${row.plan.core.id}`,
                  },
                  () => '取消',
                ),
            },
          ),
        )
      }
      if (buttons.length === 0) return '—'
      return h(NSpace, { size: 4 }, () => buttons)
    },
  },
]

onMounted(() => {
  void load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="定时转账清单" size="small">
      <template #header-extra>
        <NSpace :size="12">
          <NButtonGroup size="small">
            <NButton
              v-for="f in filterOptions"
              :key="f.key"
              :type="statusFilter === f.key ? 'primary' : 'default'"
              :data-testid="`filter-${f.key}`"
              @click="statusFilter = f.key"
            >
              {{ f.label }}
            </NButton>
          </NButtonGroup>
          <NButton
            type="primary"
            size="small"
            data-testid="transfer-create-open"
            @click="showCreateModal = true"
          >
            新建转账
          </NButton>
        </NSpace>
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :row-key="(row: TransferRow) => row.plan.core.id"
      />
    </NCard>

    <!-- 新建定时转账弹窗：转入候选按转出账户币种过滤，无商户字段（issue #203） -->
    <AppModal
      v-model:show="showCreateModal"
      title="新建定时转账"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem label="备注">
            <NInput
              v-model:value="note"
              data-testid="transfer-note"
              placeholder="转账用途，如：月度储蓄"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem label="转出账户">
            <PinyinSelect
              v-model:value="fromAccountId"
              :options="accountOptions"
              placeholder="选择转出账户"
              style="width: 200px"
              data-testid="transfer-from-account"
            />
          </NFormItem>
          <NFormItem label="转入账户">
            <PinyinSelect
              v-model:value="toAccountId"
              :options="toAccountOptions"
              placeholder="选择转入账户（与转出同币种）"
              style="width: 200px"
              data-testid="transfer-to-account"
            />
          </NFormItem>
          <NFormItem label="金额">
            <NInput
              v-model:value="amountYuan"
              data-testid="transfer-amount"
              placeholder="每期金额"
              style="width: 160px"
            />
            <AppSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
              data-testid="transfer-currency"
              style="width: 130px; margin-left: 8px"
            />
          </NFormItem>
          <NFormItem label="重复">
            <NSpace :size="8" align="center" :wrap="false">
              <span>每</span>
              <NInputNumber
                v-model:value="recurrenceInterval"
                :min="1"
                :precision="0"
                style="width: 90px"
              />
              <AppSelect
                v-model:value="recurrenceType"
                :options="recurrenceOptions"
                style="width: 90px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem label="总期数">
            <NInputNumber
              v-model:value="totalOccurrences"
              :min="1"
              :precision="0"
              placeholder="留空为无限循环"
              data-testid="transfer-total-occurrences"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem label="开始日">
            <AppDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 200px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="transfer-create-cancel" @click="showCreateModal = false">
              取消
            </NButton>
            <NButton type="primary" data-testid="transfer-create" @click="create">
              创建转账
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
