<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { h, computed, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { formatAmount } from '@/types'
import { todayStr } from '@/utils/date'
import { policyStatAmountText } from '@/utils/policy-stats'
import type { Policy, PolicyStats } from '@/types'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import MerchantLink from '@/components/MerchantLink.vue'
import PolicyFormModal from '@/components/PolicyFormModal.vue'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import { t } from '@/i18n'

const reference = useReferenceStore()
const policiesStore = usePoliciesStore()
const message = useMessage()

// —— 新建/编辑弹窗（同一表单组件，editing 为 null 即新建）——
const formShow = ref(false)
const editing = ref<Policy | null>(null)

function openCreate() {
  editing.value = null
  formShow.value = true
}

function openEdit(row: Policy) {
  editing.value = row
  formShow.value = true
}

function closeForm() {
  formShow.value = false
}

// —— 软删除（issue #360 / ADR-0051 决策 5）：二次确认后 is_deleted=1，
// 列表自动过滤；库内行与历史引用保留不置空 ——
async function removePolicy(id: string) {
  try {
    await policiesStore.remove(id)
    message.success(t('policies.msg.deleted'))
  } catch (e) {
    message.error(t('policies.msg.deleteFailed', { msg: errorMessage(e) }))
  }
}

// —— 展示层到期推导（不持久化，可推导的状态不落库）：
// 优先消费统计命令的同源推导（issue #363，与 BDD 口径一致）；统计行未加载时
// 回落本地推导（同一规则：止日非空且早于今天 → 已到期；止日空 = 长期/终身）。
// 「今天」在行渲染时即时取（无日历事件源，写入触发的重拉顺带刷新快照）——
function isExpired(row: Policy): boolean {
  return row.end_date !== null && row.end_date < todayStr()
}

function expiredState(row: Policy): boolean {
  return policiesStore.statsById.get(row.id)?.is_expired ?? isExpired(row)
}

function periodText(row: Policy): string {
  return row.end_date
    ? t('policies.period.range', { start: row.start_date, end: row.end_date })
    : t('policies.period.lifetime', { start: row.start_date })
}

function coverageText(row: Policy): string {
  if (row.coverage_amount_cents === null || row.coverage_currency_code === null) return '—'
  // 保额纯展示：按自带币种原样格式化，不折算、不进任何金额口径（ADR-0051）
  const currency = reference.getCurrency(row.coverage_currency_code)
  return formatAmount(row.coverage_amount_cents, currency)
}

// —— 保单视角统计（issue #363）：实时推导，按行取 store 同源快照；
// 合计展示经共享辅助（与详情摘要同口径），统计行未加载时显示占位 ——
function statsAmountText(row: Policy, pick: (s: PolicyStats) => number): string {
  return policyStatAmountText(policiesStore.statsById.get(row.id), pick)
}

const columns: DataTableColumns<Policy> = [
  {
    title: () => t('policies.columns.merchant'),
    key: 'merchant_id',
    render: (row) => h(MerchantLink, { merchantId: row.merchant_id }),
  },
  { title: () => t('policies.columns.productName'), key: 'product_name' },
  { title: () => t('policies.columns.policyNumber'), key: 'policy_number' },
  { title: () => t('policies.columns.period'), key: 'period', render: (row) => periodText(row) },
  {
    title: () => t('policies.columns.expiry'),
    key: 'expiry',
    width: 90,
    render: (row) =>
      h(
        NTag,
        { size: 'small', type: expiredState(row) ? 'warning' : 'success', bordered: false },
        () => (expiredState(row) ? t('policies.expiry.expired') : t('policies.expiry.active')),
      ),
  },
  {
    title: () => t('policies.columns.coverage'),
    key: 'coverage_amount_cents',
    render: (row) => coverageText(row),
  },
  {
    title: () => t('policies.columns.paid'),
    key: 'total_paid_native_cents',
    width: 110,
    render: (row) => statsAmountText(row, (s) => s.total_paid_native_cents),
  },
  {
    title: () => t('policies.columns.inflow'),
    key: 'total_inflow_native_cents',
    width: 110,
    render: (row) => statsAmountText(row, (s) => s.total_inflow_native_cents),
  },
  {
    title: () => t('policies.columns.nextCharge'),
    key: 'next_charge_date',
    width: 110,
    render: (row) => policiesStore.statsById.get(row.id)?.next_charge_date ?? '—',
  },
  {
    title: () => t('policies.columns.actions'),
    key: 'actions',
    width: 140,
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(
          NButton,
          { size: 'tiny', 'data-testid': `policy-edit-${row.id}`, onClick: () => openEdit(row) },
          () => t('policies.rowActions.edit'),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => removePolicy(row.id) },
          {
            default: () => t('policies.deleteConfirm'),
            trigger: () =>
              h(
                NButton,
                {
                  size: 'tiny',
                  type: 'error',
                  quaternary: true,
                  'data-testid': `policy-delete-${row.id}`,
                },
                () => t('policies.rowActions.delete'),
              ),
          },
        ),
      ]),
  },
]

const listTitle = computed(() => t('policies.listTitle'))

onMounted(() => {
  // 保单 store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void policiesStore.refresh().catch(() => {
    /* 失败信号已由 status 承载 */
  })
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard size="small">
      <NSpace justify="end">
        <NButton type="primary" data-testid="policy-new" @click="openCreate">
          {{ t('policies.newButton') }}
        </NButton>
      </NSpace>
    </NCard>

    <NCard :title="listTitle" size="small">
      <NDataTable :columns="columns" :data="policiesStore.policies" :bordered="false" size="small">
        <template #empty>
          <span data-testid="policy-empty-guide">{{ t('policies.emptyGuide') }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建/编辑弹窗（遮罩点击不关：AppModal 默认语义，ADR-0035） -->
    <PolicyFormModal :show="formShow" :editing="editing" @update:show="closeForm" />
  </NSpace>
</template>
