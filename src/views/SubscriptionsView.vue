<script setup lang="ts">
import { h, computed, onMounted, ref, type VNode } from 'vue'
import {
  NCard,
  NButton,
  NButtonGroup,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NDatePicker,
  NSelect,
  NTreeSelect,
  NPopconfirm,
  NSpace,
  useMessage,
  type DataTableColumns,
  type TreeSelectOption,
} from 'naive-ui'
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
import { useFormShared } from '@/composables/useFormShared'

const reference = useReferenceStore()
const { accountOptions, currencyOptions } = useFormShared()
const message = useMessage()

// ---------------------------------------------------------------------------
// 新建订阅（走既有 create_scheduled_transaction，kind=subscription）
// ---------------------------------------------------------------------------

const note = ref('')
const accountId = ref<string | null>(null)
const categoryId = ref<string | null>(null)
const amountYuan = ref('')
const currencyCode = ref('CNY')
const recurrenceType = ref<RecurrenceType>('monthly')
const recurrenceInterval = ref(1)
const startDate = ref(todayStr())

function todayStr(): string {
  const d = new Date()
  const month = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${d.getFullYear()}-${month}-${day}`
}

const recurrenceOptions = [
  { label: '每天', value: 'daily' },
  { label: '每周', value: 'weekly' },
  { label: '每月', value: 'monthly' },
  { label: '每年', value: 'yearly' },
]

/** 订阅扣款为支出，分类候选仅支出类（树形）。 */
const categoryTreeOptions = computed(
  () => reference.treeCategoryOptions('expense') as unknown as TreeSelectOption[],
)

async function create() {
  if (!accountId.value) {
    message.warning('请选择扣款账户')
    return
  }
  const amountCents = yuanToCents(amountYuan.value)
  if (amountCents === null || amountCents <= 0) {
    message.warning('请输入大于 0 的金额')
    return
  }
  try {
    await api.createScheduledTransaction({
      kind: 'subscription',
      account_id: accountId.value,
      category_id: categoryId.value,
      amount_cents: amountCents,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: null,
      start_date: startDate.value,
      note: note.value.trim() || null,
    })
    message.success('已创建订阅')
    note.value = ''
    amountYuan.value = ''
    await load()
  } catch (e) {
    message.error(`创建失败: ${e}`)
  }
}

// ---------------------------------------------------------------------------
// 清单：list_scheduled_transactions 过滤 subscription + 状态过滤；
// 下期扣款取 get_scheduled_transaction_detail 的最早 pending 期次（窗口外显示 —）
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 下期 pending 期次（无则为 null，占位「—」）。 */
interface SubscriptionRow {
  plan: ScheduledTransactionWithExt
  next: ScheduledTransactionOccurrence | null
}

const rows = ref<SubscriptionRow[]>([])
const loading = ref(false)
/** 清单状态过滤：默认只看进行中（issue #159 验收）。 */
const statusFilter = ref<'active' | 'paused' | 'cancelled'>('active')

const filteredRows = computed(() =>
  rows.value.filter((r) => r.plan.core.status === statusFilter.value),
)

async function load() {
  loading.value = true
  try {
    const plans = (await api.listScheduledTransactions()).filter(
      (p) => p.core.kind === 'subscription',
    )
    // 下期扣款来自既有详情命令的 pending 期次（ASC 排序，取首条）；
    // 预生成窗口之外不现场推算日期（避免第三套日期口径）
    const details = await Promise.all(
      plans.map(async (p) => {
        try {
          const d = await api.getScheduledTransactionDetail(p.core.id)
          // 后端按 scheduled_date ASC 返回；这里再取最早一条作为「下期」（仅选取，不推算）
          const next =
            [...d.pending_occurrences].sort((a, b) =>
              a.scheduled_date.localeCompare(b.scheduled_date),
            )[0] ?? null
          return { plan: p, next } satisfies SubscriptionRow
        } catch {
          return { plan: p, next: null } satisfies SubscriptionRow
        }
      }),
    )
    rows.value = details
  } catch (e) {
    message.error(`加载订阅失败: ${e}`)
  } finally {
    loading.value = false
  }
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
    message.error(`操作失败: ${e}`)
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

function recurrenceLabel(row: SubscriptionRow): string {
  const { recurrence_type, recurrence_interval } = row.plan.core
  const unit = recurrenceUnit[recurrence_type as RecurrenceType] ?? recurrence_type
  return recurrence_interval > 1 ? `每${recurrence_interval}${unit}` : `每${unit}`
}

function statusLabel(status: string): string {
  return status === 'active' ? '进行中' : status === 'paused' ? '已暂停' : status === 'cancelled' ? '已取消' : status
}

const filterOptions = [
  { key: 'active' as const, label: '进行中' },
  { key: 'paused' as const, label: '已暂停' },
  { key: 'cancelled' as const, label: '已取消' },
]

const columns: DataTableColumns<SubscriptionRow> = [
  {
    title: '备注',
    key: 'note',
    render: (row) => row.plan.core.note ?? '—',
  },
  {
    title: '分类',
    key: 'category',
    render: (row) => reference.categoryPath(row.plan.core.category_id) || '—',
  },
  {
    title: '扣款账户',
    key: 'account',
    render: (row) => reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id,
  },
  {
    title: '金额',
    key: 'amount',
    render: (row) => formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code)),
  },
  { title: '周期', key: 'recurrence', render: recurrenceLabel },
  { title: '开始日', key: 'start_date', render: (row) => row.plan.core.start_date },
  { title: '状态', key: 'status', render: (row) => statusLabel(row.plan.core.status) },
  {
    title: '下期扣款',
    key: 'next',
    // 测试锚点：无 pending 期次（预生成窗口之外）时断言占位，不现场推算日期
    render: (row) =>
      h(
        'span',
        { 'data-testid': `next-charge-${row.plan.core.id}` },
        row.next
          ? `${row.next.scheduled_date} · ${formatAmount(row.next.amount_cents, reference.getCurrency(row.plan.core.currency_code))}`
          : '—',
      ),
  },
  {
    title: '操作',
    key: 'actions',
    render: (row) => {
      const status = row.plan.core.status
      const buttons: VNode[] = []
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
            NPopconfirm,
            { onPositiveClick: () => changeStatus(row.plan.core.id, 'cancelled') },
            {
              default: () => '取消后不再扣款，已生成的交易与历史期次保留。确认取消？',
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
  // 参考数据走单一来源 store（ensureFresh + ledger:changed 失效自动重拉）
  void reference.ensureFresh().catch(() => {})
  void load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新建订阅" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="备注">
          <NInput
            v-model:value="note"
            data-testid="sub-note"
            placeholder="服务名称，如：视频会员"
            style="width: 160px"
          />
        </NFormItem>
        <NFormItem label="扣款账户">
          <NSelect v-model:value="accountId" :options="accountOptions" style="width: 140px" />
        </NFormItem>
        <NFormItem label="分类">
          <NTreeSelect
            v-model:value="categoryId"
            :options="categoryTreeOptions"
            placeholder="支出分类"
            clearable
            style="width: 150px"
          />
        </NFormItem>
        <NFormItem label="金额">
          <NInput
            v-model:value="amountYuan"
            data-testid="sub-amount"
            placeholder="每期金额"
            style="width: 110px"
          />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect v-model:value="currencyCode" :options="currencyOptions" style="width: 120px" />
        </NFormItem>
        <NFormItem label="周期">
          <NSelect v-model:value="recurrenceType" :options="recurrenceOptions" style="width: 90px" />
        </NFormItem>
        <NFormItem label="间隔">
          <NInputNumber
            v-model:value="recurrenceInterval"
            :min="1"
            :precision="0"
            style="width: 80px"
          />
        </NFormItem>
        <NFormItem label="开始日">
          <NDatePicker
            v-model:formatted-value="startDate"
            type="date"
            value-format="yyyy-MM-dd"
            style="width: 140px"
          />
        </NFormItem>
        <NButton type="primary" data-testid="sub-create" @click="create">创建订阅</NButton>
      </NForm>
    </NCard>

    <NCard title="订阅清单" size="small">
      <template #header-extra>
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
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :row-key="(row: SubscriptionRow) => row.plan.core.id"
      />
    </NCard>
  </NSpace>
</template>
