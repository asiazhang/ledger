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
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import type { ScheduledTransactionOccurrence } from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
} from '@/composables/useScheduledPlanList'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import SubscriptionSpendPanel from '@/components/scheduled/SubscriptionSpendPanel.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 订阅页签 = ScheduledPlanList 计划清单模块（ADR-0041 迁移步 2）的薄适配器：
 * 清单加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符与周期标签全在模块，
 * 生命周期变更（暂停/恢复/取消）后经 onStatusChanged 回调钩子刷新订阅花费面板；
 * 本组件只留订阅形态真差异——商户挂靠、编辑弹窗（仅金额以外字段可编辑，
 * ADR-0023 决策三，商户解析走表单接缝编辑分支）、花费面板与列/单元格渲染。
 * 本页签无 #309 显式可见变化项：列、操作、表单、提示、排序零变化。
 */

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 表单接缝（ADR-0041）：新建与编辑弹窗各持一份草稿实例——公共草稿字段、商户
// 解析（含重名兜底竞态）与公共 payload 组装全仓单点；金额、校验与提交编排留本页签。
// ---------------------------------------------------------------------------

const createForm = useScheduledPlanForm()
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
} = createForm
const editForm = useScheduledPlanForm()
const {
  note: editNote,
  accountId: editAccountId,
  categoryId: editCategoryId,
  merchantRef: editMerchantRef,
} = editForm

// 实际花费分析区（issue #160）：创建/编辑/生命周期变更/期次变更后同步刷新；
// 生命周期变更的刷新经清单模块 onStatusChanged 钩子进入此函数（订阅真差异）
const spendPanelRef = ref<InstanceType<typeof SubscriptionSpendPanel> | null>(null)
function refreshSpend() {
  void spendPanelRef.value?.reload()
}

// ---------------------------------------------------------------------------
// 清单编排（ADR-0041）：全部经 ScheduledPlanList 模块；确认弹层在本适配器渲染。
// 下期扣款 = 最早 pending 期次（模块公共扩展器，仅选取、不现场推算日期）；
// 清单加载失败/生命周期成功失败提示与重拉时序全部内化在模块。
// ---------------------------------------------------------------------------

/** 下期扣款扩展：最早 pending 期次（无则 null，占位「—」）。 */
interface SubscriptionExt {
  next: ScheduledTransactionOccurrence | null
}
type SubscriptionRow = ScheduledPlanRow<SubscriptionExt>

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

const list = useScheduledPlanList<SubscriptionExt>({
  kind: 'subscription',
  expandDetail: (_plan, detail) => ({
    next: detail ? earliestPendingOccurrence(detail) : null,
  }),
  loadErrorText: () => t('scheduled.pane.subscriptionLoadError'),
  cancelConfirmText: () => t('scheduled.pane.subscriptionCancelConfirm'),
  onStatusChanged: refreshSpend,
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
})
const { loading, statusFilter, statusFilterOptions, filteredRows } = list

// ---------------------------------------------------------------------------
// 新建订阅 = 模态对话框（issue #158）：不引入独立路由页面，
// 弹窗内完成填写与校验，提交成功后关闭并刷新列表
// ---------------------------------------------------------------------------

const showCreateModal = ref(false)

const amountYuan = ref('')

/** 重置新建表单到初始态：公共字段走接缝 reset，金额留本页签。 */
function resetCreateForm() {
  createForm.reset()
  amountYuan.value = ''
}

async function create() {
  if (!accountId.value) {
    message.warning(t('scheduled.form.selectAccount'))
    return
  }
  const amountCents = yuanToCents(amountYuan.value)
  if (amountCents === null || amountCents <= 0) {
    message.warning(t('scheduled.form.amountPositive'))
    return
  }
  try {
    const merchantId = await createForm.resolveMerchant()
    await api.createScheduledTransaction(
      createForm.buildCreateInput({ kind: 'subscription', amountCents, merchantId }),
    )
    message.success(t('scheduled.toast.subscriptionCreated'))
    showCreateModal.value = false
    resetCreateForm()
    await list.load()
    refreshSpend()
  } catch (e) {
    message.error(t('scheduled.toast.createFailed', { message: errorMessage(e) }))
  }
}

// ---------------------------------------------------------------------------
// 编辑订阅（issue #162，ADR-0023 决策三）：仅非金额字段（备注/账户/分类），
// 弹窗无金额输入；提交走订阅编辑命令，携带金额字段会被后端显式拒绝。
// 编辑不改已生成的期次与交易（期次执行时从计划读取这些字段），只影响未来。
// 草稿字段（备注/账户/分类/商户）复用表单接缝的第二实例，打开即回填。
// ---------------------------------------------------------------------------

const showEditModal = ref(false)
const editingId = ref<string | null>(null)
/** 被编辑计划的当前商户 id（供表单接缝 resolveMerchant 的软删兜底分支判定）。 */
const editCurrentMerchantId = ref<string | null>(null)

// 编辑商户下拉（issue #190）：在用商户 + 原商户软删且超出会话缓存时追加兜底选项
// 承载原 id——裸 uuid 不可读，提交时按「未改动」语义原样保留。
const editMerchantOptions = computed<{ label: string; value: string }[]>(() => {
  const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }))
  const current = editCurrentMerchantId.value
  if (current && !reference.merchantMap.has(current)) {
    base.unshift({ label: t('scheduled.form.deletedMerchant'), value: current })
  }
  return base
})

function openEdit(row: SubscriptionRow) {
  editingId.value = row.plan.core.id
  editNote.value = row.plan.core.note ?? ''
  editAccountId.value = row.plan.core.account_id
  editCategoryId.value = row.plan.core.category_id
  editCurrentMerchantId.value = row.plan.merchant_id
  editMerchantRef.value = row.plan.merchant_id
  showEditModal.value = true
}

async function saveEdit() {
  if (!editingId.value) return
  if (!editAccountId.value) {
    message.warning(t('scheduled.form.selectAccount'))
    return
  }
  try {
    await api.updateScheduledSubscription({
      id: editingId.value,
      account_id: editAccountId.value,
      category_id: editCategoryId.value,
      merchant_id: await editForm.resolveMerchant(editCurrentMerchantId.value),
      note: editNote.value.trim() || null,
    })
    message.success(t('scheduled.toast.saved'))
    showEditModal.value = false
    await list.load()
    refreshSpend()
  } catch (e) {
    message.error(t('scheduled.toast.saveFailed', { message: errorMessage(e) }))
  }
}

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：三页签通用组件，订阅页签同享；
// 弹窗内重试成功会发 changed，清单与花费面板随之刷新
// ---------------------------------------------------------------------------

async function onDetailChanged() {
  await list.load()
  refreshSpend()
}

// ---------------------------------------------------------------------------
// 展示助手：参考数据名称解析与单元格渲染留适配器；周期标签走模块单源
// ---------------------------------------------------------------------------

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

/** 周期下拉选项（computed 现取标签，切语言即时生效） */
const recurrenceOptions = computed(scheduledRecurrenceOptions)

const columns = computed<DataTableColumns<SubscriptionRow>>(() => [
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
    title: t('scheduled.column.amount'),
    key: 'amount',
    render: (row) => formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code)),
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
    title: t('scheduled.column.nextCharge'),
    key: 'next',
    // 测试锚点：无 pending 期次（预生成窗口之外）时断言占位，不现场推算日期
    render: (row) =>
      h(
        'span',
        { 'data-testid': `next-charge-${row.plan.core.id}` },
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
    // 行操作描述符（可用性矩阵/标签/run）由模块构建；此处按描述符渲染，
    // 含 confirm 文案的动作经 AppPopconfirm 二次确认（弹层纪律 ADR-0035）。
    // 订阅真差异「编辑」（仅非金额字段，ADR-0023 决策三）插在期次之后：
    // active/paused 可编辑、已取消不提供——模块描述符不含编辑，留本适配器。
    render: (row) => {
      const status = row.plan.core.status
      const buttons: VNode[] = []
      for (const action of list.rowActions(row)) {
        if (!action.available) continue
        if (action.confirm) {
          buttons.push(
            h(
              AppPopconfirm,
              { onPositiveClick: action.run },
              {
                default: () => action.confirm,
                trigger: () =>
                  h(
                    NButton,
                    {
                      size: 'tiny',
                      type: 'error',
                      quaternary: true,
                      'data-testid': `op-${action.key}-${row.plan.core.id}`,
                    },
                    () => action.label,
                  ),
              },
            ),
          )
        } else {
          buttons.push(
            h(
              NButton,
              {
                size: 'tiny',
                'data-testid': `op-${action.key}-${row.plan.core.id}`,
                onClick: action.run,
              },
              () => action.label,
            ),
          )
        }
        if (action.key === 'detail' && (status === 'active' || status === 'paused')) {
          buttons.push(
            h(
              NButton,
              {
                size: 'tiny',
                'data-testid': `op-edit-${row.plan.core.id}`,
                onClick: () => openEdit(row),
              },
              () => t('scheduled.action.edit'),
            ),
          )
        }
      }
      if (buttons.length === 0) return '—'
      return h(NSpace, { size: 4 }, () => buttons)
    },
  },
])

onMounted(() => {
  void list.load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('scheduled.pane.subscriptionList')" size="small">
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
            data-testid="sub-create-open"
            @click="showCreateModal = true"
          >
            {{ t('scheduled.pane.createSubscription') }}
          </NButton>
        </NSpace>
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

    <!-- 新建订阅弹窗：提交成功关闭并刷新列表（与记一笔弹窗同模式） -->
    <AppModal
      v-model:show="showCreateModal"
      :title="t('scheduled.pane.createSubscription')"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <!-- 布局对齐记一笔表单（CategoryForm）：NSpace 提供行距，宽度取 160-280 档；
           金额+币种、周期+间隔 各并一行减少行数 -->
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="sub-note"
              :placeholder="t('scheduled.form.subscriptionNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
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
              data-testid="sub-merchant"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.amount')">
            <NInput
              v-model:value="amountYuan"
              data-testid="sub-amount"
              :placeholder="t('scheduled.form.amountPerPeriodPlaceholder')"
              style="width: 160px"
            />
            <AppSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
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
            <NButton data-testid="sub-create-cancel" @click="showCreateModal = false">{{ t('scheduled.form.cancel') }}</NButton>
            <NButton type="primary" data-testid="sub-create" @click="create">{{ t('scheduled.pane.createSubscriptionSubmit') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 编辑订阅弹窗（issue #162）：仅非金额字段（备注/账户/分类），无金额输入 -->
    <AppModal
      v-model:show="showEditModal"
      :title="t('scheduled.pane.editSubscription')"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="editNote"
              data-testid="sub-edit-note"
              :placeholder="t('scheduled.form.subscriptionNoteEditPlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="editAccountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.category')">
            <AppTreeSelect
              v-model:value="editCategoryId"
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
              v-model:value="editMerchantRef"
              :options="editMerchantOptions"
              tag
              clearable
              :placeholder="t('scheduled.form.merchantPlaceholder')"
              style="width: 220px"
              data-testid="sub-edit-merchant"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="sub-edit-cancel" @click="showEditModal = false">{{ t('scheduled.form.cancel') }}</NButton>
            <NButton type="primary" data-testid="sub-edit-save" @click="saveEdit">{{ t('scheduled.form.save') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <SubscriptionSpendPanel ref="spendPanelRef" />

    <!-- 期次详情弹窗（issue #205）：三页签通用，订阅页签同享 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
