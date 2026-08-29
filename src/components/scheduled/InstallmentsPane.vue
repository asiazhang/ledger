<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, h, onMounted, ref, type Ref, type VNode } from 'vue'
import {
  NCard,
  NButton,
  NButtonGroup,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NModal,
  NInputNumber,
  NDatePicker,
  NSelect,
  NTreeSelect,
  NPopconfirm,
  NSpace,
  NProgress,
  useMessage,
  type DataTableColumns,
  type TreeSelectOption,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import { todayStr } from '@/utils/date'
import { installmentSchedule } from '@/utils/installment'
import type {
  RecurrenceType,
  ScheduledTransactionWithExt,
  UpdateStatusInput,
} from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

const reference = useReferenceStore()
const appStore = useAppStore()
const { accountOptions, currencyOptions } = useFormShared()
const message = useMessage()

// ---------------------------------------------------------------------------
// 新建分期 = 模态对话框（issue #204）：录入「总金额 + 期数」，实时预览每期金额
// 与最后一期（含尾差）。每期金额口径唯一来源是 installmentSchedule（与后端
// expand_occurrences 的 floor 均分、尾差进最后一期一致）。商户接入见 issue #206
// （与订阅表单同款补全、未命中即建；不暴露「每月几号」）。
// ---------------------------------------------------------------------------

const showCreateModal = ref(false)

const note = ref('')
const accountId = ref<string | null>(null)
const categoryId = ref<string | null>(null)
const merchantRef = ref<string | null>(null)
const totalYuan = ref('')
const periods = ref<number | null>(null)
const currencyCode = ref(appStore.defaultCurrency)
const recurrenceType = ref<RecurrenceType>('monthly')
const recurrenceInterval = ref(1)
const startDate = ref(todayStr())

const recurrenceOptions = [
  { label: '每天', value: 'daily' },
  { label: '每周', value: 'weekly' },
  { label: '每月', value: 'monthly' },
  { label: '每年', value: 'yearly' },
]

/** 分期扣款为支出，分类候选仅支出类（树形）。 */
const categoryTreeOptions = computed(
  () => reference.treeCategoryOptions('expense') as unknown as TreeSelectOption[],
)

// 商户下拉选项（issue #206 / ADR-0028）：在用商户（与订阅表单同款补全，未命中即建）。
const merchantOptions = computed<{ label: string; value: string }[]>(() =>
  reference.merchants.map((m) => ({ label: m.name, value: m.id })),
)

/**
 * 商户解析（保存时单点收口，issue #206）：「输入即建」交互——
 * 1. 空 → null（无商户）；
 * 2. 选中已有商户（value 为 id）→ 原样携带；
 * 3. 输入文本精确命中在用商户名 → 按名复用；
 * 4. 未命中 → `create_merchant` 即建；重名兕底（store 陈旧竞态）先强制重拉
 *    按名复用，仍失败才向上抛。
 * （分期无编辑表单，无编辑路径的软删兜底分支。）
 */
async function resolveMerchantId(source: Ref<string | null>): Promise<string | null> {
  const ref = source.value
  if (!ref) return null
  if (reference.merchantMap.has(ref)) return ref
  const name = ref.trim()
  if (!name) return null
  const existing = reference.merchantByName.get(name)
  if (existing) return existing.id
  try {
    return await api.createMerchant({ name })
  } catch (e) {
    // 重名兕底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
    try {
      await reference.refresh()
    } catch {
      /* 保留原 create 错误 */
    }
    const retry = reference.merchantByName.get(name)
    if (retry) return retry.id
    throw e
  }
}

/** 每期金额预览：总额与期数均合法时给出每期与末期（含尾差），否则为空。 */
const schedule = computed(() => {
  const totalCents = yuanToCents(totalYuan.value)
  if (totalCents === null || periods.value === null || periods.value < 1) return null
  try {
    return installmentSchedule(totalCents, periods.value)
  } catch {
    return null
  }
})

/** 预览文案：整除时末期与每期一致，不提尾差；不整除时明确标注末期含尾差。 */
const previewText = computed(() => {
  const s = schedule.value
  if (!s) return ''
  const currency = reference.getCurrency(currencyCode.value)
  const per = formatAmount(s.perPeriodCents, currency)
  if (s.lastPeriodCents === s.perPeriodCents) return `每期 ${per}`
  return `每期 ${per} · 末期 ${formatAmount(s.lastPeriodCents, currency)}（含尾差）`
})

/** 重置新建表单到初始态：模态语义下每次打开应是全新表单。 */
function resetCreateForm() {
  note.value = ''
  accountId.value = null
  categoryId.value = null
  merchantRef.value = null
  totalYuan.value = ''
  periods.value = null
  currencyCode.value = appStore.defaultCurrency
  recurrenceType.value = 'monthly'
  recurrenceInterval.value = 1
  startDate.value = todayStr()
}

async function create() {
  if (!accountId.value) {
    message.warning('请选择扣款账户')
    return
  }
  const totalCents = yuanToCents(totalYuan.value)
  if (totalCents === null || totalCents <= 0) {
    message.warning('请输入大于 0 的总金额')
    return
  }
  const totalOccurrences = periods.value
  if (totalOccurrences === null || totalOccurrences < 1) {
    message.warning('请输入不小于 1 的期数')
    return
  }
  if (totalCents < totalOccurrences) {
    message.warning('总金额不能小于期数（每期至少 1 分）')
    return
  }
  const s = installmentSchedule(totalCents, totalOccurrences)
  try {
    await api.createScheduledTransaction({
      kind: 'installment',
      account_id: accountId.value,
      category_id: categoryId.value,
      merchant_id: await resolveMerchantId(merchantRef),
      // amount_cents 存每期金额（floor 口径），与期次生成一致（见 e2e 先例）
      amount_cents: s.perPeriodCents,
      total_amount_cents: totalCents,
      total_occurrences: totalOccurrences,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: null,
      start_date: startDate.value,
      note: note.value.trim() || null,
    })
    message.success('已创建分期计划')
    showCreateModal.value = false
    resetCreateForm()
    await load()
  } catch (e) {
    message.error(`创建失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 清单：list_scheduled_transactions 过滤 installment + 状态过滤；
// 进度来自既有详情命令的已完成期次实时汇总（金额与期数，不持久化、不推算）
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 已完成期次汇总（期数与金额）。 */
interface InstallmentRow {
  plan: ScheduledTransactionWithExt
  completedCount: number
  completedAmountCents: number
  /** 详情命令失败：进度格显示加载失败，不静默显示 0。 */
  detailFailed?: boolean
}

const rows = ref<InstallmentRow[]>([])
const loading = ref(false)
/** 清单状态过滤：默认只看进行中（与订阅清单一致）。 */
const statusFilter = ref<'active' | 'paused' | 'cancelled'>('active')

const filteredRows = computed(() =>
  rows.value.filter((r) => r.plan.core.status === statusFilter.value),
)

async function load() {
  loading.value = true
  try {
    const plans = (await api.listScheduledTransactions()).filter(
      (p) => p.core.kind === 'installment',
    )
    const details = await Promise.all(
      plans.map(async (p): Promise<InstallmentRow> => {
        try {
          const d = await api.getScheduledTransactionDetail(p.core.id)
          return {
            plan: p,
            completedCount: d.completed_occurrences,
            completedAmountCents: d.completed_amount_cents,
          }
        } catch {
          return { plan: p, completedCount: 0, completedAmountCents: 0, detailFailed: true }
        }
      }),
    )
    rows.value = details
  } catch (e) {
    message.error(`加载分期失败: ${errorMessage(e)}`)
  } finally {
    loading.value = false
  }
}

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：三页签通用组件；弹窗内重试成功会发 changed，
// 清单随之刷新（分期进度由详情实时汇总，重拉即可）
// ---------------------------------------------------------------------------

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

function openDetail(row: InstallmentRow) {
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

function recurrenceLabel(row: InstallmentRow): string {
  const { recurrence_type, recurrence_interval } = row.plan.core
  const unit = recurrenceUnit[recurrence_type as RecurrenceType] ?? recurrence_type
  return recurrence_interval > 1 ? `每${recurrence_interval}${unit}` : `每${unit}`
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

/** 进度百分比：期数维度（已完成期数 / 总期数），总额异常时兜底 0。 */
function progressPercentage(row: InstallmentRow): number {
  const total = row.plan.total_occurrences ?? 0
  if (total <= 0) return 0
  return Math.min(100, (row.completedCount / total) * 100)
}

const filterOptions = [
  { key: 'active' as const, label: '进行中' },
  { key: 'paused' as const, label: '已暂停' },
  { key: 'cancelled' as const, label: '已取消' },
]

const columns: DataTableColumns<InstallmentRow> = [
  {
    title: '备注',
    key: 'note',
    render: (row) => row.plan.core.note ?? '—',
  },
  {
    title: '商户',
    key: 'merchant',
    // 改名即时生效（引用指向 id）：merchantMap 含软删商户会话缓存，历史计划照常显示
    render: (row) => {
      const m = row.plan.merchant_id ? reference.merchantMap.get(row.plan.merchant_id) : undefined
      return m?.name ?? '—'
    },
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
    title: '总金额',
    key: 'total',
    render: (row) =>
      formatAmount(row.plan.total_amount_cents ?? 0, reference.getCurrency(row.plan.core.currency_code)),
  },
  {
    title: '进度',
    key: 'progress',
    render: (row) => {
      const currency = reference.getCurrency(row.plan.core.currency_code)
      const text = row.detailFailed
        ? '加载失败'
        : `已还 ${formatAmount(row.completedAmountCents, currency)} / ${formatAmount(row.plan.total_amount_cents ?? 0, currency)} · ${row.completedCount}/${row.plan.total_occurrences ?? 0} 期`
      return h('div', { 'data-testid': `inst-progress-${row.plan.core.id}` }, [
        row.detailFailed
          ? null
          : h(NProgress, {
              type: 'line',
              percentage: progressPercentage(row),
              showIndicator: false,
              style: 'max-width: 160px',
            }),
        h('span', text),
      ])
    },
  },
  { title: '周期', key: 'recurrence', render: recurrenceLabel },
  { title: '开始日', key: 'start_date', render: (row) => row.plan.core.start_date },
  { title: '状态', key: 'status', render: (row) => statusLabel(row.plan.core.status) },
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
        // 取消不删已生成交易与历史期次（后端行为，ADR-0024），二次确认防误触
        buttons.push(
          h(
            NPopconfirm,
            { onPositiveClick: () => changeStatus(row.plan.core.id, 'cancelled') },
            {
              default: () => '取消后不再自动扣款，已生成的交易与历史期次保留。确认取消？',
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
    <NCard title="分期清单" size="small">
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
            data-testid="inst-create-open"
            @click="showCreateModal = true"
          >
            新建分期
          </NButton>
        </NSpace>
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :row-key="(row: InstallmentRow) => row.plan.core.id"
      />
    </NCard>

    <!-- 新建分期弹窗：总金额 + 期数实时预览；其余字段与订阅表单同款 -->
    <NModal
      v-model:show="showCreateModal"
      title="新建分期"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem label="总金额">
            <NInput
              v-model:value="totalYuan"
              data-testid="inst-total"
              placeholder="分期总金额"
              style="width: 160px"
            />
            <NSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
              style="width: 130px; margin-left: 8px"
            />
          </NFormItem>
          <NFormItem label="期数">
            <NInputNumber
              v-model:value="periods"
              data-testid="inst-periods"
              :min="1"
              :precision="0"
              placeholder="总期数"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem label="每期金额">
            <span data-testid="inst-preview">{{ previewText }}</span>
          </NFormItem>
          <NFormItem label="备注">
            <NInput
              v-model:value="note"
              data-testid="inst-note"
              placeholder="分期用途，如：手机分期"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem label="扣款账户">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              placeholder="选择账户"
              style="width: 200px"
              data-testid="inst-account"
            />
          </NFormItem>
          <NFormItem label="分类">
            <NTreeSelect
              v-model:value="categoryId"
              :options="categoryTreeOptions"
              placeholder="选择分类"
              filterable
              clearable
              :consistent-menu-width="false"
              style="width: 220px"
            />
          </NFormItem>
          <NFormItem label="商户">
            <PinyinSelect
              v-model:value="merchantRef"
              :options="merchantOptions"
              tag
              clearable
              placeholder="选择商户，可直接输入新名称"
              style="width: 220px"
              data-testid="inst-merchant"
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
              <NSelect
                v-model:value="recurrenceType"
                :options="recurrenceOptions"
                style="width: 100px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem label="开始日">
            <NDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 200px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="inst-create-cancel" @click="showCreateModal = false">取消</NButton>
            <NButton type="primary" data-testid="inst-create" @click="create">创建分期</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </NModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
