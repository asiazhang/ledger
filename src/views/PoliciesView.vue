<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { h, computed, nextTick, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
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
import PolicyFormModal from '@/components/PolicyFormModal.vue'
import { useModalIntent } from '@/composables/useModalIntent'
import { useFocusParam } from '@/composables/useFocusParam'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import { t } from '@/i18n'

const reference = useReferenceStore()
const policiesStore = usePoliciesStore()
const message = useMessage()
const route = useRoute()

// —— 新建/编辑弹窗（同一表单组件双模式）——
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072，词汇表 ModalIntent）：
// 意图闭集双成员（新建无载荷；编辑携带目标保单行），显示由「意图非空」派生
// （无独立 show 布尔），序号随开启递增驱动表单重建（:key=formSeq），关闭
// （✕ / ESC / 取消 / 保存成功）统一经工厂清回 null 终态。现状无序号守卫，
// 迁移为缺陷修复（本票唯一声明的行为变化）：守卫从无到有，「弹窗开着时
// 目标行被替换回填旧行」缺陷消亡，同目标重开重回填等边缘语义细化同归此类；
// 此外等价。

/** 保单表单弹窗意图（新建/编辑双模式闭集）：编辑携带目标保单行。 */
type PolicyFormIntent = { mode: 'create' } | { mode: 'edit'; policy: Policy }

const {
  intent: formIntent,
  seq: formSeq,
  open: openFormIntent,
  close: closeForm,
} = useModalIntent<PolicyFormIntent>()

function openCreate() {
  openFormIntent({ mode: 'create' })
}

function openEdit(row: Policy) {
  openFormIntent({ mode: 'edit', policy: row })
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
    // 保司列纯文本（issue #713 / ADR-0082）：保司不是商户，无商户排行可下钻；
    // 名字经 reference store 保司显示映射解析（含软删保司——存量保单照常显示）。
    title: () => t('policies.columns.insurer'),
    key: 'insurer_id',
    render: (row) => reference.insurerMap.get(row.insurer_id)?.name ?? '—',
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

// —— 来源跳转落点（spec #704 / issue #706，词汇表「实体定位参数（focus 参数）」）：
// 挂载消费一次（读一次语义归 useFocusParam 单点）；保单行高亮属「先拿 id 后等
// 数据」——回调只暂存 id，列表渲染后滚动定位（读取时刻与生效时刻解耦）。
// 高亮保持到实例消亡：刷新/重进 = 新实例重定位，URL 在场即复现、可分享。
// 独立路由与收纳态（资产·更多保单页签，同一组件整体装载）共用本消费点。
const highlightedPolicyId = ref<string | null>(null)

/** 表格组件引用：滚动定位在其自身子树内查行锚点。 */
const tableRef = ref<{ $el?: HTMLElement } | null>(null)

const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (policyId) => {
    highlightedPolicyId.value = policyId
  },
})

/** 行锚点 + 高亮类：滚动定位的 data 属性与高亮样式同源一处。 */
function rowProps(row: Policy): Record<string, unknown> {
  return {
    'data-policy-id': row.id,
    class: row.id === highlightedPolicyId.value ? 'policy-row-focus' : undefined,
  }
}

/** 渲染完成后滚动到高亮行（列表数据到位后调用；无 focus 空转）。 */
async function scrollToHighlighted(): Promise<void> {
  if (!highlightedPolicyId.value) return
  // 行挂载链多跳（数据到位 → 表格行渲染），有界重试至锚点出现；在本表格子树内
  // 按 dataset 比对定位（不经属性选择器拼接实体 id，无注入面；组件自身子树查询，
  // 独立路由与「更多」页签装载同效）。
  for (let hop = 0; hop < 5; hop += 1) {
    await nextTick()
    const root = tableRef.value?.$el as HTMLElement | undefined
    const anchor = root
      ? Array.from(root.querySelectorAll<HTMLElement>('[data-policy-id]')).find(
          (el) => el.dataset.policyId === highlightedPolicyId.value,
        )
      : undefined
    if (anchor) {
      anchor.scrollIntoView({ block: 'center' })
      return
    }
  }
}

const listTitle = computed(() => t('policies.listTitle'))

onMounted(() => {
  // focus 读一次：先拿 id（消费闸门内化），列表到位后生效
  focusParam.consume()
  // 保单 store self-init + ledger:changed 信号兜底；mounted 重拉覆盖错误重试
  void policiesStore
    .refresh()
    .catch(() => {
      /* 失败信号已由 status 承载 */
    })
    .then(scrollToHighlighted)
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
      <NDataTable
        ref="tableRef"
        :columns="columns"
        :data="policiesStore.policies"
        :row-props="rowProps"
        :bordered="false"
        size="small"
      >
        <template #empty>
          <span data-testid="policy-empty-guide">{{ t('policies.emptyGuide') }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建/编辑弹窗（同一表单组件双模式，遮罩点击不关：AppModal 默认语义，
         ADR-0035）。显示由「意图非空」派生（无独立 show 布尔），关闭（✕ /
         ESC / 取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建
         （ADR-0072）。 -->
    <PolicyFormModal
      :key="formSeq"
      :show="formIntent !== null"
      :editing="formIntent?.mode === 'edit' ? formIntent.policy : null"
      @update:show="(v: boolean) => (v ? undefined : closeForm())"
    />
  </NSpace>
</template>

<style>
/* 来源跳转行高亮（spec #704 / issue #706）：rowProps 把类打在 NDataTable 内部
   <tr> 上，scoped 选择器够不到，故用全局规则 + 视图专名类（不被复用为通用名）。 */
tr.policy-row-focus > td {
  background-color: rgba(245, 158, 11, 0.16);
}
</style>

