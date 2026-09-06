<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
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
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import PlanRowActions from '@/components/scheduled/PlanRowActions.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import type { ScheduledTransactionOccurrence } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from '@/composables/useScheduledPlanList'
import { useModalIntent } from '@/composables/useModalIntent'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { usePlanFocusLanding } from '@/composables/usePlanFocusLanding'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 定时转账页签 = ScheduledPlanList 计划清单模块（ADR-0041）的薄适配器：
 * 清单加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符与周期标签全在模块；
 * 本组件只留转账形态真差异——同币种转入过滤、币种跟随与跨币种清空、
 * 一次性/周期期数语义（总期数三态）、取消确认文案与列/单元格渲染。
 * 转出 / 转入账户必须同币种（词汇表 ScheduledTransfer 边界，后端行为层强制）。
 */

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 清单编排（ADR-0041）：全部经 ScheduledPlanList 模块；确认弹层在本适配器渲染
// ---------------------------------------------------------------------------

/** 下期转账扩展：最早 pending 期次（无则 null，占位「—」）。 */
interface TransferExt {
  next: ScheduledTransactionOccurrence | null
}
type TransferRow = ScheduledPlanRow<TransferExt>

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

const list = useScheduledPlanList<TransferExt>({
  kind: 'scheduled_transfer',
  expandDetail: (_plan, detail) => ({
    next: detail ? earliestPendingOccurrence(detail) : null,
  }),
  loadErrorText: () => t('scheduled.pane.transferLoadError'),
  cancelConfirmText: () => t('scheduled.pane.transferCancelConfirm'),
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
})
const { loading, statusFilter, statusFilterOptions, filteredRows } = list

/** 期次详情弹窗内重试成功会发 changed，清单随之刷新。 */
async function onDetailChanged() {
  await list.load()
}

// ---------------------------------------------------------------------------
// 新建定时转账弹窗（issue #203 / #204）：开启/关闭编排归弹窗意图工厂 ModalIntent
// （ADR-0072，词汇表 ModalIntent）——纯新建布尔方言收编为单成员意图闭集
// （type: create，无目标载荷），显示由「意图非空」派生（无独立 show 布尔），
// 关闭（提交成功 / ✕ / ESC / 取消）统一经工厂清回 null 终态；表单重置留视图。
// ---------------------------------------------------------------------------

/** 新建定时转账弹窗意图（单成员闭集）：纯新建，无目标载荷。 */
interface TransferCreateIntent {
  type: 'create'
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<TransferCreateIntent>()

const toAccountId = ref<string | null>(null)
const amountYuan = ref('')
/** 总期数：null = 无限循环（留空），1 = 一次性，N = 有限期数 */
const totalOccurrences = ref<number | null>(null)

// ---------------------------------------------------------------------------
// 新建定时转账 = 模态对话框（与订阅页签同模式）。
// 表单接缝（ADR-0041）：公共草稿字段、公共 payload 组装与新建提交流程编排全仓单点——
// 转出账户即接缝的「账户」字段；转入账户过滤、币种跟随与总期数语义留本页签。
// submitCreate 接缝持编排（商户解析跳过 → payload 合并 → 创建 → 提示 → 公共草稿重置），
// 成功回调注入本页签原子动作：关窗 + 特化字段重置 + 清单刷新（spec #520）。
// ---------------------------------------------------------------------------

const form = useScheduledPlanForm({
  onSubmitted: () => {
    // 提交成功后原子动作：关窗 + 特化字段重置 + 清单刷新（公共草稿已由接缝重置）
    closeCreateIntent()
    toAccountId.value = null
    amountYuan.value = ''
    totalOccurrences.value = null
    void list.load()
  },
})
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

/** 新建提交：表单校验留页签，提交流程编排由接缝 submitCreate 持有（spec #520）。 */
async function create() {
  if (!fromAccountId.value) {
    message.warning(t('scheduled.form.selectFromAccount'))
    return
  }
  if (!toAccountId.value) {
    message.warning(t('scheduled.form.selectToAccount'))
    return
  }
  const amountCents = yuanToCents(amountYuan.value)
  if (amountCents === null || amountCents <= 0) {
    message.warning(t('scheduled.form.amountPositive'))
    return
  }
  await form.submitCreate({
    kind: 'scheduled_transfer',
    amountCents,
    specific: { to_account_id: toAccountId.value, total_occurrences: totalOccurrences.value },
  })
}

// ---------------------------------------------------------------------------
// 展示助手：参考数据名称解析与单元格渲染留适配器；周期标签走模块单源
// ---------------------------------------------------------------------------

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

/** 周期下拉选项（computed 现取标签，切语言即时生效） */
const recurrenceOptions = computed(scheduledRecurrenceOptions)

const columns = computed<DataTableColumns<TransferRow>>(() => [
  {
    title: t('scheduled.column.note'),
    key: 'note',
    render: (row) => row.plan.core.note ?? '—',
  },
  {
    title: t('scheduled.column.fromAccount'),
    key: 'from',
    render: (row) =>
      reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id,
  },
  {
    title: t('scheduled.column.toAccount'),
    key: 'to',
    render: (row) => {
      const id = row.plan.to_account_id
      return id ? (reference.accountMap.get(id)?.name ?? id) : '—'
    },
  },
  {
    title: t('scheduled.column.amount'),
    key: 'amount',
    render: (row) =>
      `${formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code))}${row.plan.total_occurrences != null ? ` ${t('scheduled.column.occurrencesSuffix', { n: row.plan.total_occurrences })}` : ''}`,
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
    title: t('scheduled.column.nextTransfer'),
    key: 'next',
    // 测试锚点：无 pending 期次（预生成窗口之外）时断言占位，不现场推算日期
    render: (row) =>
      h(
        'span',
        { 'data-testid': `next-transfer-${row.plan.core.id}` },
        row.ext.next
          ? `${row.ext.next.scheduled_date} · ${formatAmount(row.ext.next.amount_cents, reference.getCurrency(row.plan.core.currency_code))}`
          : row.detailFailed
            ? t('scheduled.list.loadFailed')
            : '—',
      ),
  },
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
    <NCard :title="t('scheduled.pane.transferList')" size="small">
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
            data-testid="transfer-create-open"
            @click="openCreateIntent({ type: 'create' })"
          >
            {{ t('scheduled.pane.createTransfer') }}
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

    <!-- 新建定时转账弹窗：转入候选按转出账户币种过滤，无商户字段（issue #203）；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createTransferModal')"
      preset="card"
      display-directive="if"
      card-size="md"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="transfer-note"
              :placeholder="t('scheduled.form.transferNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.fromAccount')">
            <PinyinSelect
              v-model:value="fromAccountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.fromAccountPlaceholder')"
              style="width: 200px"
              data-testid="transfer-from-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.toAccount')">
            <PinyinSelect
              v-model:value="toAccountId"
              :options="toAccountOptions"
              :placeholder="t('scheduled.form.toAccountPlaceholder')"
              style="width: 200px"
              data-testid="transfer-to-account"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.amount')">
            <NInput
              v-model:value="amountYuan"
              data-testid="transfer-amount"
              :placeholder="t('scheduled.form.amountPerPeriodPlaceholder')"
              style="width: 160px"
            />
            <AppSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
              data-testid="transfer-currency"
              style="width: 130px; margin-left: 8px"
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
                data-testid="transfer-recurrence"
                style="width: 90px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.totalOccurrences')">
            <NInputNumber
              v-model:value="totalOccurrences"
              :min="1"
              :precision="0"
              :placeholder="t('scheduled.form.totalOccurrencesPlaceholder')"
              data-testid="transfer-total-occurrences"
              style="width: 200px"
            />
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
            <NButton data-testid="transfer-create-cancel" @click="closeCreateIntent">
              {{ t('scheduled.form.cancel') }}
            </NButton>
            <NButton type="primary" data-testid="transfer-create" @click="create">
              {{ t('scheduled.pane.createTransferSubmit') }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 期次详情弹窗（issue #205）：三页签通用 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
