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
import { todayStr } from '@/utils/date'
import type { RecurrenceType, ScheduledTransactionOccurrence } from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  SCHEDULED_RECURRENCE_OPTIONS,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from '@/composables/useScheduledPlanList'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 定时转账页签 = ScheduledPlanList 计划清单模块（ADR-0041）的薄适配器：
 * 清单加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符与周期标签全在模块；
 * 本组件只留转账形态真差异——同币种转入过滤、币种跟随与跨币种清空、
 * 一次性/周期期数语义（总期数三态）、取消确认文案与列/单元格渲染。
 * 转出 / 转入账户必须同币种（词汇表 ScheduledTransfer 边界，后端行为层强制）。
 */

const reference = useReferenceStore()
const appStore = useAppStore()
const { accountOptions, currencyOptions } = useFormShared()
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
  loadErrorText: '加载定时转账失败',
  cancelConfirmText: '取消后不再自动转账，已生成的交易与历史期次保留。确认取消？',
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
})
const { loading, statusFilter, statusFilterOptions, filteredRows } = list

/** 期次详情弹窗内重试成功会发 changed，清单随之刷新。 */
async function onDetailChanged() {
  await list.load()
}

// ---------------------------------------------------------------------------
// 新建定时转账 = 模态对话框（与订阅页签同模式）
// ---------------------------------------------------------------------------

const showCreateModal = ref(false)

const note = ref('')
const fromAccountId = ref<string | null>(null)
const toAccountId = ref<string | null>(null)
const amountYuan = ref('')
const currencyCode = ref(appStore.defaultCurrency)
const recurrenceType = ref<RecurrenceType>('monthly')
const recurrenceInterval = ref(1)
/** 总期数：null = 无限循环（留空），1 = 一次性，N = 有限期数 */
const totalOccurrences = ref<number | null>(null)
const startDate = ref(todayStr())

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

/** 重置新建表单到初始态：模态语义下每次打开应是全新表单。 */
function resetCreateForm() {
  note.value = ''
  fromAccountId.value = null
  toAccountId.value = null
  amountYuan.value = ''
  currencyCode.value = appStore.defaultCurrency
  recurrenceType.value = 'monthly'
  recurrenceInterval.value = 1
  totalOccurrences.value = null
  startDate.value = todayStr()
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
    await api.createScheduledTransaction({
      kind: 'scheduled_transfer',
      account_id: fromAccountId.value,
      to_account_id: toAccountId.value,
      category_id: null,
      merchant_id: null,
      amount_cents: amountCents,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: null,
      start_date: startDate.value,
      note: note.value.trim() || null,
      total_occurrences: totalOccurrences.value,
    })
    message.success('已创建定时转账')
    showCreateModal.value = false
    resetCreateForm()
    await list.load()
  } catch (e) {
    message.error(`创建失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 展示助手：参考数据名称解析与单元格渲染留适配器；周期标签走模块单源
// ---------------------------------------------------------------------------

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
  {
    title: '周期',
    key: 'recurrence',
    render: (row) =>
      scheduledRecurrenceLabel(row.plan.core.recurrence_type, row.plan.core.recurrence_interval),
  },
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
        row.ext.next
          ? `${row.ext.next.scheduled_date} · ${formatAmount(row.ext.next.amount_cents, reference.getCurrency(row.plan.core.currency_code))}`
          : row.detailFailed
            ? '加载失败'
            : '—',
      ),
  },
  {
    title: '操作',
    key: 'actions',
    // 行操作描述符（可用性矩阵/标签/run）由模块构建；此处按描述符渲染，
    // 含 confirm 文案的动作经 AppPopconfirm 二次确认（弹层纪律 ADR-0035）
    render: (row) => {
      const buttons: VNode[] = list
        .rowActions(row)
        .filter((a) => a.available)
        .map((a) =>
          a.confirm
            ? h(
                AppPopconfirm,
                { onPositiveClick: a.run },
                {
                  default: () => a.confirm,
                  trigger: () =>
                    h(
                      NButton,
                      {
                        size: 'tiny',
                        type: 'error',
                        quaternary: true,
                        'data-testid': `op-${a.key}-${row.plan.core.id}`,
                      },
                      () => a.label,
                    ),
                },
              )
            : h(
                NButton,
                {
                  size: 'tiny',
                  'data-testid': `op-${a.key}-${row.plan.core.id}`,
                  onClick: a.run,
                },
                () => a.label,
              ),
        )
      if (buttons.length === 0) return '—'
      return h(NSpace, { size: 4 }, () => buttons)
    },
  },
]

onMounted(() => {
  void list.load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="定时转账清单" size="small">
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
                :options="[...SCHEDULED_RECURRENCE_OPTIONS]"
                data-testid="transfer-recurrence"
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
