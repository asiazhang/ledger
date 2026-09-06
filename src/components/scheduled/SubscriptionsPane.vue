<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
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
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import type { ScheduledTransactionOccurrence } from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useModalIntent } from '@/composables/useModalIntent'
import { useScheduledPlanForm } from '@/composables/useScheduledPlanForm'
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
  type ScheduledPlanRowAction,
} from '@/composables/useScheduledPlanList'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PlanRowActions from '@/components/scheduled/PlanRowActions.vue'
import SubscriptionSpendPanel from '@/components/scheduled/SubscriptionSpendPanel.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { usePlanFocusLanding } from '@/composables/usePlanFocusLanding'
import { scheduledStatusLabel } from '@/utils/scheduled'

/**
 * 订阅页签 = ScheduledPlanList 计划清单模块（ADR-0041 迁移步 2）的薄适配器：
 * 清单加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符与周期标签全在模块，
 * 生命周期变更（暂停/恢复/取消）后经 onStatusChanged 回调钩子刷新订阅花费面板。
 * 行操作经共享渲染组件 PlanRowActions 渲染（确认弹层/锚点/空占位只此一份，
 * ADR-0041 决策 7 注），描述符由本页签自组——模块产出在详情动作后插入自建
 * 编辑描述符（同形状、无确认文案；编辑弹窗开启逻辑留本页签，spec #520）。
 * 本组件只留订阅形态真差异——商户挂靠、编辑弹窗（仅金额以外字段可编辑，
 * ADR-0023 决策三，商户解析走表单接缝编辑分支）、花费面板与列/单元格渲染。
 * 本页签无 #309 显式可见变化项：列、操作、表单、提示、排序零变化。
 */

const reference = useReferenceStore()
const message = useMessage()

// ---------------------------------------------------------------------------
// 表单接缝（ADR-0041）：新建与编辑弹窗各持一份草稿实例——公共草稿字段、商户
// 解析（含重名兜底竞态）与公共 payload 组装全仓单点。新建实例的提交流程编排
// 沉入接缝 submitCreate（见下方「新建订阅表单接缝与提交」段，spec #520）。
// ---------------------------------------------------------------------------

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
// 弹窗内完成填写与校验，提交成功后关闭并刷新列表。
// 开启/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072，词汇表 ModalIntent）：
// 纯新建布尔方言收编为单成员意图闭集（type: create，无目标载荷），显示由
// 「意图非空」派生（无独立 show 布尔），关闭（提交成功 / ✕ / ESC / 取消）统一
// 经工厂清回 null 终态，不存在「布尔弹窗」方言的中间态；表单重置等关闭后副作用留视图。
// ---------------------------------------------------------------------------

/** 新建订阅弹窗意图（单成员闭集）：纯新建，无目标载荷。 */
interface SubscriptionCreateIntent {
  type: 'create'
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<SubscriptionCreateIntent>()

const amountYuan = ref('')

// ---------------------------------------------------------------------------
// 新建订阅表单接缝与提交（spec #520）：提交流程编排沉入接缝 submitCreate——
// 商户解析进编排 → 公共 payload → 创建命令 → 提示 → 公共草稿重置 → 成功后回调；
// 成功后回调注入本页签原子动作：关窗 + 金额重置 + 清单刷新 + 订阅花费刷新
// （花费刷新为订阅真差异追加，时序与现状一致：清单刷新之后）。金额校验
// （账户必选、金额 > 0）与元转分留本页签。
// ---------------------------------------------------------------------------

const createForm = useScheduledPlanForm({
  onSubmitted: async () => {
    // 提交成功后原子动作（公共草稿已由接缝重置）
    closeCreateIntent()
    amountYuan.value = ''
    await list.load()
    refreshSpend()
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
} = createForm

/** 新建提交：金额校验留页签，提交流程编排由接缝 submitCreate 持有（spec #520）。 */
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
  await createForm.submitCreate({ kind: 'subscription', amountCents })
}

// ---------------------------------------------------------------------------
// 编辑订阅（issue #162，ADR-0023 决策三）：仅非金额字段（备注/账户/分类），
// 弹窗无金额输入；提交走订阅编辑命令，携带金额字段会被后端显式拒绝。
// 编辑不改已生成的期次与交易（期次执行时从计划读取这些字段），只影响未来。
// 草稿字段（备注/账户/分类/商户）复用表单接缝的第二实例，打开即回填。
//
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072，词汇表 ModalIntent）：
// 意图闭集单成员（携带目标计划行），显示由「意图非空」派生（无独立 show 布尔），
// 序号随开启递增驱动表单重建（:key=editSeq），关闭（✕ / ESC / 取消 / 保存成功）
// 统一经工厂清回 null 终态；行入口 openEdit 不变。editCurrentMerchantId 等
// 表单伴随状态属表单接缝/视图侧，不进工厂。
// ---------------------------------------------------------------------------

/** 编辑订阅弹窗意图（单成员闭集）：携带目标计划行。 */
interface SubscriptionEditIntent {
  row: SubscriptionRow
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<SubscriptionEditIntent>()

/** 被编辑计划的当前商户 id（供表单接缝 resolveMerchant 的软删兜底分支判定）；
 * 表单伴随状态属表单接缝/视图侧，不进意图工厂（ADR-0072 决策 4）。 */
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
  editNote.value = row.plan.core.note ?? ''
  editAccountId.value = row.plan.core.account_id
  editCategoryId.value = row.plan.core.category_id
  editCurrentMerchantId.value = row.plan.merchant_id
  editMerchantRef.value = row.plan.merchant_id
  openEditIntent({ row })
}

async function saveEdit() {
  const target = editIntent.value
  if (!target) return
  if (!editAccountId.value) {
    message.warning(t('scheduled.form.selectAccount'))
    return
  }
  try {
    await api.updateScheduledSubscription({
      id: target.row.plan.core.id,
      account_id: editAccountId.value,
      category_id: editCategoryId.value,
      merchant_id: await editForm.resolveMerchant(editCurrentMerchantId.value),
      note: editNote.value.trim() || null,
    })
    message.success(t('scheduled.toast.saved'))
    closeEdit()
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
    // 行操作描述符由模块构建，本页签自组数组透传共享渲染组件（确认弹层/测试锚点/
    // 空占位只此一份，ADR-0041 决策 7 注）：订阅真差异「编辑」（仅非金额字段，
    // ADR-0023 决策三）以同形状描述符紧随详情动作之后——active/paused 可编辑、
    // 已取消不提供、无确认文案；渲染组件不识形态，编辑弹窗开启逻辑留本页签。
    render: (row) => {
      const status = row.plan.core.status
      const actions: ScheduledPlanRowAction[] = []
      for (const action of list.rowActions(row)) {
        if (action.key === 'detail') {
          actions.push(action, {
            key: 'edit',
            label: t('scheduled.action.edit'),
            available: status === 'active' || status === 'paused',
            confirm: null,
            run: () => openEdit(row),
          })
        } else {
          actions.push(action)
        }
      }
      return h(PlanRowActions, { actions, rowId: row.plan.core.id })
    },
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
            @click="openCreateIntent({ type: 'create' })"
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

    <!-- 新建订阅弹窗：提交成功关闭并刷新列表（与记一笔弹窗同模式）；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createSubscription')"
      preset="card"
      display-directive="if"
      card-size="md"
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
            <NButton data-testid="sub-create-cancel" @click="closeCreateIntent">{{ t('scheduled.form.cancel') }}</NButton>
            <NButton type="primary" data-testid="sub-create" @click="create">{{ t('scheduled.pane.createSubscriptionSubmit') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 编辑订阅弹窗（issue #162）：仅非金额字段（备注/账户/分类），无金额输入。
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态；
         序号作 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      :title="t('scheduled.pane.editSubscription')"
      preset="card"
      display-directive="if"
      card-size="md"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
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
          <!-- 挂保单的缴费协议不显示商户（issue #713 / ADR-0082 决策 2）：付款对象
               语义由保单的保司承担，计划行不挂商户（后端对非空提交显式拒绝） -->
          <NFormItem v-if="!editIntent?.row.plan.policy_id" :label="t('scheduled.form.merchant')">
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
            <NButton data-testid="sub-edit-cancel" @click="closeEdit">{{ t('scheduled.form.cancel') }}</NButton>
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
