<script setup lang="ts">
import { computed, h, ref, nextTick, watch } from 'vue'
import { t } from '@/i18n'
import {
  NButton,
  NDataTable,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { errorMessage } from '@/utils/errors'
import { scheduledStatusLabel } from '@/utils/scheduled'
import { scheduledRecurrenceLabel } from '@/composables/useScheduledPlanList'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import type { Policy, ScheduledTransactionWithExt } from '@/types'
import PolicyAgreementFields from '@/components/PolicyAgreementFields.vue'

/**
 * 保单缴费协议区（issue #362 / ADR-0051 决策 2，编辑模式）：展示该保单名下的
 * 协议历史（1 张保单 → 多段协议：费率变更 = 取消旧协议 + 按新金额重建的价格
 * 分段真相，含已取消段），并提供：
 * - 无活跃协议时「添加缴费协议」（趸交/缴清保单随时可补建周期扣缴）；
 * - 有活跃协议时「改价」——沿用订阅既有语义：取消旧协议 + 按新金额重建，
 *   新段经保单上下文创建并携带引用；新段起始日预填旧协议下期扣款日
 *   （最早 pending 期次，无 pending 回落旧起始日），频率/账户/币种沿用旧段。
 *
 * 引用复制（协议 → 期次流水）由引擎继承，本组件只负责创建入参携带 policy_id。
 */
const props = defineProps<{ policy: Policy }>()

const message = useMessage()
const reference = useReferenceStore()
const fieldsRef = ref<InstanceType<typeof PolicyAgreementFields> | null>(null)

/** 协议历史（该保单名下全部订阅形态协议，按创建先后 = 分段先后）。 */
const segments = ref<ScheduledTransactionWithExt[]>([])
const loading = ref(false)

const activeSegment = computed(() => segments.value.find((s) => s.core.status === 'active') ?? null)

async function load() {
  loading.value = true
  try {
    const plans = await api.listScheduledTransactions()
    segments.value = plans
      .filter((p) => p.policy_id === props.policy.id)
      .sort((a, b) => a.core.created_at.localeCompare(b.core.created_at) || a.core.id.localeCompare(b.core.id))
  } catch (e) {
    message.error(t('policies.agreement.msg.loadFailed', { msg: errorMessage(e) }))
  } finally {
    loading.value = false
  }
}

// 弹窗复用同一实例（editing 换行 / 同保单重开）：随 policy 身份变化重拉历史，
// 避免串显上一张保单的协议段。
watch(
  () => props.policy,
  () => void load(),
  { immediate: true },
)

// ---------------------------------------------------------------------------
// 表单编排：添加 / 改价共用一个字段组实例；mode=null 表单隐藏
// ---------------------------------------------------------------------------

type FormMode = 'add' | 'rebuild'
const mode = ref<FormMode | null>(null)
/** 改价目标（旧段）——重建入参的频率/账户/币种与下期扣款日来源。 */
const rebuildFrom = ref<ScheduledTransactionWithExt | null>(null)

async function openAdd() {
  mode.value = 'add'
  rebuildFrom.value = null
  await nextTick()
  fieldsRef.value?.reset({ startDate: props.policy.start_date })
}

/** 下期扣款日（最早 pending 期次；无 pending 回落旧起始日）。 */
async function nextChargeDate(planId: string, fallback: string): Promise<string> {
  try {
    const detail = await api.getScheduledTransactionDetail(planId)
    const pending = detail.occurrences
      .filter((o) => o.status === 'pending')
      .map((o) => o.scheduled_date)
      .sort()
    return pending[0] ?? fallback
  } catch {
    return fallback
  }
}

async function openRebuild() {
  const old = activeSegment.value
  if (!old) return
  mode.value = 'rebuild'
  rebuildFrom.value = old
  await nextTick()
  fieldsRef.value?.reset({
    currencyCode: old.core.currency_code,
    recurrenceType: old.core.recurrence_type,
    recurrenceInterval: old.core.recurrence_interval,
    accountId: old.core.account_id,
    startDate: await nextChargeDate(old.core.id, old.core.start_date),
  })
}

function closeForm() {
  mode.value = null
  rebuildFrom.value = null
}

/** 提交：校验字段组 → 添加为直接创建；改价先取消旧协议再按新金额重建。 */
async function submit() {
  const fields = fieldsRef.value
  if (!fields) return
  const err = fields.validate()
  if (err) {
    message.warning(err)
    return
  }
  const input = fields.build(props.policy.id, props.policy.product_name)
  try {
    if (mode.value === 'rebuild' && rebuildFrom.value) {
      const oldId = rebuildFrom.value.core.id
      // 改价 = 取消旧协议 + 按新金额重建（订阅既有语义，ADR-0051 决策 2）；
      // 取消成功而重建失败时旧段已停（可经「添加缴费协议」补建，不产生重复扣缴）。
      await api.updateScheduledTransactionStatus({ id: oldId, new_status: 'cancelled' })
      await api.createScheduledTransaction(input)
      message.success(t('policies.agreement.msg.rebuilt'))
    } else {
      await api.createScheduledTransaction(input)
      message.success(t('policies.agreement.msg.created'))
    }
    closeForm()
    await load()
  } catch (e) {
    message.error(t('policies.agreement.msg.failed', { msg: errorMessage(e) }))
  }
}

const segmentColumns = computed<DataTableColumns<ScheduledTransactionWithExt>>(() => [
  {
    title: t('policies.agreement.column.amount'),
    key: 'amount',
    render: (s) => formatAmount(s.core.amount_cents, reference.getCurrency(s.core.currency_code)),
  },
  {
    title: t('policies.agreement.column.recurrence'),
    key: 'recurrence',
    render: (s) => scheduledRecurrenceLabel(s.core.recurrence_type, s.core.recurrence_interval),
  },
  { title: t('policies.agreement.column.startDate'), key: 'start_date', render: (s) => s.core.start_date },
  {
    title: t('policies.agreement.column.status'),
    key: 'status',
    render: (s) =>
      s.core.status === 'active'
        ? scheduledStatusLabel(s.core.status)
        : // 非活跃段（已取消/暂停）以弱化标签呈现——价格历史分段真相
          h(
            NTag,
            { size: 'small', bordered: false, type: s.core.status === 'cancelled' ? 'default' : 'warning' },
            () => scheduledStatusLabel(s.core.status),
          ),
  },
])
</script>

<template>
  <NSpace vertical :size="12">
    <NDataTable
      :columns="segmentColumns"
      :data="segments"
      :loading="loading"
      :bordered="false"
      size="small"
      :row-key="(s: ScheduledTransactionWithExt) => s.core.id"
      data-testid="policy-agreement-segments"
    />

    <!-- 操作行：无活跃段 → 添加；有活跃段 → 改价（沿用订阅价变语义） -->
    <NSpace v-if="mode === null" :size="8">
      <NButton
        v-if="!activeSegment"
        size="small"
        data-testid="policy-agreement-add"
        @click="openAdd"
      >
        {{ t('policies.agreement.add') }}
      </NButton>
      <NButton
        v-else
        size="small"
        data-testid="policy-agreement-rebuild-open"
        @click="openRebuild"
      >
        {{ t('policies.agreement.rebuild') }}
      </NButton>
    </NSpace>

    <!-- 添加/改价表单（同一字段组实例，reset 预填切换） -->
    <NSpace v-else vertical :size="12" data-testid="policy-agreement-form">
      <div v-if="mode === 'rebuild'" style="opacity: 0.7; font-size: 12px">
        {{ t('policies.agreement.rebuildHint') }}
      </div>
      <PolicyAgreementFields ref="fieldsRef" />
      <NSpace justify="end">
        <NButton size="small" data-testid="policy-agreement-form-cancel" @click="closeForm">
          {{ t('policies.agreement.cancel') }}
        </NButton>
        <NButton
          type="primary"
          size="small"
          data-testid="policy-agreement-submit"
          @click="submit"
        >
          {{ mode === 'rebuild' ? t('policies.agreement.rebuildConfirm') : t('policies.agreement.create') }}
        </NButton>
      </NSpace>
    </NSpace>
  </NSpace>
</template>
