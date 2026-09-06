<script setup lang="ts">
import { computed, h, onMounted, ref } from 'vue'
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
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import { installmentSchedule } from '@/utils/installment'
import { useReferenceStore } from '@/stores/reference'
import { useModalIntent } from '@/composables/useModalIntent'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import {
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from '@/composables/useScheduledPlanList'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanRowActions from '@/components/scheduled/PlanRowActions.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { usePlanFocusLanding } from '@/composables/usePlanFocusLanding'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 分期页签 = ScheduledPlanList 计划清单模块（ADR-0041 迁移步 3）的薄适配器：
 * 清单加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符与周期标签全在模块；
 * 状态过滤补「已完成」项（#309 显式可见变化之二在此落地：按 Plan Lifecycle 自然
 * 完成的分期计划恢复可见可查，此前从清单消失且无入口可见）。本组件只留分期形态
 * 真差异——期数预览（含尾差文案）与进度列（期数维度百分比 + 已还金额/总额实时
 * 汇总，不持久化、不推算）。行操作经共享渲染组件 PlanRowActions 透传渲染（确认
 * 弹层/锚点/空占位只此一份，ADR-0041 决策 7）；表单公共部分与提交流程编排走
 * ScheduledPlanForm 接缝（商户挂靠保留，issue #206；submitCreate 编排，spec #520），
 * 校验、每期 floor 口径与总额/期数特化组装留本页签。
 */

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 清单编排（ADR-0041）：全部经 ScheduledPlanList 模块；行操作经共享渲染组件
// PlanRowActions 透传渲染（确认弹层由组件承载，ADR-0041 决策 3 注 / spec #520）。
// 进度来自既有详情命令的已完成期次实时汇总（金额与期数，不持久化、不推算）。
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 形态扩展器产出的已完成期次汇总（期数与金额）。 */
interface InstallmentExt {
  completedCount: number
  completedAmountCents: number
}
type InstallmentRow = ScheduledPlanRow<InstallmentExt>

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

const list = useScheduledPlanList<InstallmentExt>({
  kind: 'installment',
  expandDetail: (_plan, detail) => ({
    completedCount: detail?.completed_occurrences ?? 0,
    completedAmountCents: detail?.completed_amount_cents ?? 0,
  }),
  loadErrorText: () => t('scheduled.pane.installmentLoadError'),
  cancelConfirmText: () => t('scheduled.pane.installmentCancelConfirm'),
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
})
const { loading, statusFilter, statusFilterOptions, filteredRows } = list

/** 期次详情弹窗内重试成功会发 changed，清单随之刷新（进度由详情实时汇总，重拉即可）。 */
async function onDetailChanged() {
  await list.load()
}

// ---------------------------------------------------------------------------
// 新建分期弹窗（issue #204 / #206）：开启/关闭编排归弹窗意图工厂 ModalIntent
// （ADR-0072，词汇表 ModalIntent）——纯新建布尔方言收编为单成员意图闭集
// （type: create，无目标载荷），显示由「意图非空」派生（无独立 show 布尔），
// 关闭（提交成功 / ✕ / ESC / 取消）统一经工厂清回 null 终态；表单重置留视图。
// ---------------------------------------------------------------------------

/** 新建分期弹窗意图（单成员闭集）：纯新建，无目标载荷。 */
interface InstallmentCreateIntent {
  type: 'create'
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<InstallmentCreateIntent>()

const totalYuan = ref('')
const periods = ref<number | null>(null)

// ---------------------------------------------------------------------------
// 表单接缝（ADR-0041）：公共草稿字段、商户解析（含重名兜底竞态）与公共 payload
// 组装全仓单点；提交流程编排沉入接缝 submitCreate（spec #520）——商户解析进编排
// → 公共 payload 合并形态特化字段 → 创建命令 → 提示 → 公共草稿重置 → 成功后回调，
// 成功回调注入本页签原子动作：关窗 + 特化字段重置 + 清单刷新。总额/期数、校验与
// 每期 floor 口径留本页签（每期金额预览口径唯一来源是 installmentSchedule，
// 与后端 expand_occurrences 的 floor 均分、尾差进末期一致）。
// ---------------------------------------------------------------------------

const form = useScheduledPlanForm({
  onSubmitted: () => {
    // 提交成功后原子动作：关窗 + 特化字段重置 + 清单刷新（公共草稿已由接缝重置）
    closeCreateIntent()
    totalYuan.value = ''
    periods.value = null
    void list.load()
  },
})
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

// ---------------------------------------------------------------------------
// 每期金额预览与新建提交（分期形态真差异）
// ---------------------------------------------------------------------------

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

/** 新建提交：校验与每期 floor 口径留页签，提交流程编排由接缝 submitCreate 持有（spec #520）。 */
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
  // amount_cents 存每期金额（floor 口径），与期次生成一致（见 e2e 先例）
  const s = installmentSchedule(totalCents, totalOccurrences)
  await form.submitCreate({
    kind: 'installment',
    amountCents: s.perPeriodCents,
    specific: { total_amount_cents: totalCents, total_occurrences: totalOccurrences },
  })
}

// ---------------------------------------------------------------------------
// 展示助手：参考数据名称解析与单元格渲染留适配器；周期标签走模块单源
// ---------------------------------------------------------------------------

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

/** 进度百分比：期数维度（已完成期数 / 总期数），总额异常时兜底 0。 */
function progressPercentage(row: InstallmentRow): number {
  const total = row.plan.total_occurrences ?? 0
  if (total <= 0) return 0
  return Math.min(100, (row.ext.completedCount / total) * 100)
}

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
            paid: formatAmount(row.ext.completedAmountCents, currency),
            total: formatAmount(row.plan.total_amount_cents ?? 0, currency),
            count: row.ext.completedCount,
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
    // 行操作描述符（可用性矩阵/标签/run）由模块构建；此处透传共享渲染组件，
    // 确认弹层/测试锚点/空占位只此一份（ADR-0041 决策 7，spec #520）
    render: (row) =>
      h(PlanRowActions, {
        actions: list.rowActions(row),
        rowId: row.plan.core.id,
      }),
  },
])

/** 来源跳转落点入参（spec #704 / issue #707）：待开的计划 id（视图侧 focus
 * 读一次后的暂存；空则无落点）。 */
const props = defineProps<{ focusPlanId?: string | null }>()

const emit = defineEmits<{ (e: 'focusConsumed'): void }>()

// 计划来源落点时序（读 id → 开窗 → 回报）收口共享工厂，三页签零手搓：
usePlanFocusLanding({
  focusPlanId: () => props.focusPlanId,
  openDetail: (id) => void planDetailRef.value?.open(id),
  onConsumed: () => emit('focusConsumed'),
})

onMounted(() => {
  void list.load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('scheduled.pane.installmentList')" size="small">
      <template #header-extra>
        <NSpace :size="12">
          <NButtonGroup size="small">
            <NButton
              v-for="f in statusFilterOptions"
              :key="f.key"
              :type="statusFilter === f.key ? 'primary' : 'default'"
              :data-testid="`filter-${f.key}`"
              @click="list.setStatusFilter(f.key)"
            >
              {{ f.label }}
            </NButton>
          </NButtonGroup>
          <NButton
            type="primary"
            size="small"
            data-testid="inst-create-open"
            @click="openCreateIntent({ type: 'create' })"
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

    <!-- 新建分期弹窗：总金额 + 期数实时预览；其余字段与订阅表单同款；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createInstallment')"
      preset="card"
      display-directive="if"
      card-size="md"
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
            <NButton data-testid="inst-create-cancel" @click="closeCreateIntent">{{ t('scheduled.form.cancel') }}</NButton>
            <NButton type="primary" data-testid="inst-create" @click="create">{{ t('scheduled.pane.createInstallmentSubmit') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
