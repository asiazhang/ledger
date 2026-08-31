<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, h, onMounted, ref, type VNode } from 'vue'
import { t } from '@/i18n'
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
  NProgress,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import { installmentSchedule } from '@/utils/installment'
import type {
  ScheduledTransactionWithExt,
  UpdateStatusInput,
} from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import {
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
} from '@/composables/useScheduledPlanList'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 新建分期 = 模态对话框（issue #204）：录入「总金额 + 期数」，实时预览每期金额
// 与最后一期（含尾差）。每期金额口径唯一来源是 installmentSchedule（与后端
// expand_occurrences 的 floor 均分、尾差进最后一期一致）。商户接入见 issue #206
// （输入即建，解析单点走表单接缝；不暴露「每月几号」）。
// ---------------------------------------------------------------------------

// 表单接缝（ADR-0041）：公共草稿字段、商户解析（含重名兜底竞态）与公共 payload
// 组装全仓单点；总额/期数、校验与提交编排留本页签。
const form = useScheduledPlanForm()
const {
  note,
  accountId,
  categoryId,
  merchantRef,
  currencyCode,
  recurrenceType,
  recurrenceInterval,
  startDate,
  accountOptions,
  currencyOptions,
  categoryTreeOptions,
  merchantOptions,
} = form

const showCreateModal = ref(false)

const totalYuan = ref('')
const periods = ref<number | null>(null)

/** 周期下拉选项（与另两页签同单源；computed 现取标签，切语言即时生效） */
const recurrenceOptions = computed(scheduledRecurrenceOptions)

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
  if (s.lastPeriodCents === s.perPeriodCents) return t('scheduled.preview.perPeriod', { amount: per })
  return t('scheduled.preview.withLast', {
    per,
    last: formatAmount(s.lastPeriodCents, currency),
  })
})

/** 重置新建表单到初始态：公共字段走接缝 reset，总额/期数留本页签。 */
function resetCreateForm() {
  totalYuan.value = ''
  periods.value = null
  form.reset()
}

async function create() {
  if (!accountId.value) {
    message.warning(t('scheduled.form.selectAccount'))
    return
  }
  const totalCents = yuanToCents(totalYuan.value)
  if (totalCents === null || totalCents <= 0) {
    message.warning(t('scheduled.form.totalPositive'))
    return
  }
  const totalOccurrences = periods.value
  if (totalOccurrences === null || totalOccurrences < 1) {
    message.warning(t('scheduled.form.periodsMin'))
    return
  }
  if (totalCents < totalOccurrences) {
    message.warning(t('scheduled.form.totalBelowPeriods'))
    return
  }
  const s = installmentSchedule(totalCents, totalOccurrences)
  try {
    const merchantId = await form.resolveMerchant()
    await api.createScheduledTransaction(
      form.buildCreateInput({
        kind: 'installment',
        // amount_cents 存每期金额（floor 口径），与期次生成一致（见 e2e 先例）
        amountCents: s.perPeriodCents,
        merchantId,
        specific: { total_amount_cents: totalCents, total_occurrences: totalOccurrences },
      }),
    )
    message.success(t('scheduled.toast.installmentCreated'))
    showCreateModal.value = false
    resetCreateForm()
    await load()
  } catch (e) {
    message.error(t('scheduled.toast.createFailed', { message: errorMessage(e) }))
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
    message.error(`${t('scheduled.pane.installmentLoadError')}: ${errorMessage(e)}`)
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
    message.success(
      newStatus === 'paused'
        ? t('scheduled.status.paused')
        : newStatus === 'active'
          ? t('scheduled.toast.resumed')
          : t('scheduled.status.cancelled'),
    )
    await load()
  } catch (e) {
    message.error(t('scheduled.toast.operationFailed', { message: errorMessage(e) }))
  }
}

// ---------------------------------------------------------------------------
// 展示助手
// ---------------------------------------------------------------------------

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

/** 进度百分比：期数维度（已完成期数 / 总期数），总额异常时兜底 0。 */
function progressPercentage(row: InstallmentRow): number {
  const total = row.plan.total_occurrences ?? 0
  if (total <= 0) return 0
  return Math.min(100, (row.completedCount / total) * 100)
}

const filterOptions = computed(() => [
  { key: 'active' as const, label: scheduledStatusLabel('active') },
  { key: 'paused' as const, label: scheduledStatusLabel('paused') },
  { key: 'cancelled' as const, label: scheduledStatusLabel('cancelled') },
])

const columns = computed<DataTableColumns<InstallmentRow>>(() => [
  {
    title: t('scheduled.column.note'),
    key: 'note',
    render: (row) => row.plan.core.note ?? '—',
  },
  {
    title: t('scheduled.column.merchant'),
    key: 'merchant',
    // 改名即时生效（引用指向 id）：merchantMap 含软删商户会话缓存，历史计划照常显示
    render: (row) => {
      const m = row.plan.merchant_id ? reference.merchantMap.get(row.plan.merchant_id) : undefined
      return m?.name ?? '—'
    },
  },
  {
    title: t('scheduled.column.category'),
    key: 'category',
    render: (row) => reference.categoryPath(row.plan.core.category_id) || '—',
  },
  {
    title: t('scheduled.column.account'),
    key: 'account',
    render: (row) => reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id,
  },
  {
    title: t('scheduled.column.totalAmount'),
    key: 'total',
    render: (row) =>
      formatAmount(row.plan.total_amount_cents ?? 0, reference.getCurrency(row.plan.core.currency_code)),
  },
  {
    title: t('scheduled.column.progress'),
    key: 'progress',
    render: (row) => {
      const currency = reference.getCurrency(row.plan.core.currency_code)
      const text = row.detailFailed
        ? t('scheduled.list.loadFailed')
        : t('scheduled.progress.repaid', {
            paid: formatAmount(row.completedAmountCents, currency),
            total: formatAmount(row.plan.total_amount_cents ?? 0, currency),
            count: row.completedCount,
            occurrences: row.plan.total_occurrences ?? 0,
          })
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
  {
    title: t('scheduled.column.recurrence'),
    key: 'recurrence',
    render: (row) =>
      scheduledRecurrenceLabel(row.plan.core.recurrence_type, row.plan.core.recurrence_interval),
  },
  { title: t('scheduled.column.startDate'), key: 'start_date', render: (row) => row.plan.core.start_date },
  { title: t('scheduled.column.status'), key: 'status', render: (row) => statusLabel(row.plan.core.status) },
  {
    title: t('scheduled.column.actions'),
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
          () => t('scheduled.action.detail'),
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
            () => t('scheduled.action.pause'),
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
            () => t('scheduled.action.resume'),
          ),
        )
      }
      if (status === 'active' || status === 'paused') {
        // 取消不删已生成交易与历史期次（后端行为，ADR-0024），二次确认防误触
        buttons.push(
          h(
            AppPopconfirm,
            { onPositiveClick: () => changeStatus(row.plan.core.id, 'cancelled') },
            {
              default: () => t('scheduled.pane.installmentCancelConfirm'),
              trigger: () =>
                h(
                  NButton,
                  {
                    size: 'tiny',
                    type: 'error',
                    quaternary: true,
                    'data-testid': `op-cancel-${row.plan.core.id}`,
                  },
                  () => t('scheduled.action.cancel'),
                ),
            },
          ),
        )
      }
      if (buttons.length === 0) return '—'
      return h(NSpace, { size: 4 }, () => buttons)
    },
  },
])

onMounted(() => {
  void load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('scheduled.pane.installmentList')" size="small">
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
            {{ t('scheduled.pane.createInstallment') }}
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
    <AppModal
      v-model:show="showCreateModal"
      :title="t('scheduled.pane.createInstallment')"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.totalAmount')">
            <NInput
              v-model:value="totalYuan"
              data-testid="inst-total"
              :placeholder="t('scheduled.form.totalAmountPlaceholder')"
              style="width: 160px"
            />
            <AppSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
              style="width: 130px; margin-left: 8px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.periods')">
            <NInputNumber
              v-model:value="periods"
              data-testid="inst-periods"
              :min="1"
              :precision="0"
              :placeholder="t('scheduled.form.periodsPlaceholder')"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.perPeriod')">
            <span data-testid="inst-preview">{{ previewText }}</span>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="inst-note"
              :placeholder="t('scheduled.form.installmentNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
              data-testid="inst-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.category')">
            <AppTreeSelect
              v-model:value="categoryId"
              :options="categoryTreeOptions"
              :placeholder="t('scheduled.form.categoryPlaceholder')"
              filterable
              clearable
              :consistent-menu-width="false"
              style="width: 220px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.merchant')">
            <PinyinSelect
              v-model:value="merchantRef"
              :options="merchantOptions"
              tag
              clearable
              :placeholder="t('scheduled.form.merchantPlaceholder')"
              style="width: 220px"
              data-testid="inst-merchant"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.recurrence')">
            <NSpace :size="8" align="center" :wrap="false">
              <span>{{ t('scheduled.form.every') }}</span>
              <NInputNumber
                v-model:value="recurrenceInterval"
                :min="1"
                :precision="0"
                style="width: 90px"
              />
              <AppSelect
                v-model:value="recurrenceType"
                :options="recurrenceOptions"
                style="width: 100px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.startDate')">
            <AppDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 200px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="inst-create-cancel" @click="showCreateModal = false">{{ t('scheduled.form.cancel') }}</NButton>
            <NButton type="primary" data-testid="inst-create" @click="create">{{ t('scheduled.pane.createInstallmentSubmit') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
