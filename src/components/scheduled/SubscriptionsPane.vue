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
  NInputNumber,
  NSpace,
  useMessage,
  type DataTableColumns,
  type TreeSelectOption,
} from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import { formatAmount } from '@/types'
import { yuanToCents } from '@/utils/money'
import { todayStr } from '@/utils/date'
import type {
  RecurrenceType,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  UpdateStatusInput,
} from '@/types'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import SubscriptionSpendPanel from '@/components/scheduled/SubscriptionSpendPanel.vue'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import { scheduledStatusLabel } from '@/utils/scheduled'

const reference = useReferenceStore()
const appStore = useAppStore()
const { accountOptions, currencyOptions } = useFormShared()
const message = useMessage()

// 实际花费分析区（issue #160）：创建/暂停/取消后同步刷新
const spendPanelRef = ref<InstanceType<typeof SubscriptionSpendPanel> | null>(null)
function refreshSpend() {
  void spendPanelRef.value?.reload()
}

// ---------------------------------------------------------------------------
// 新建订阅 = 模态对话框（issue #158）：不引入独立路由页面，
// 弹窗内完成填写与校验，提交成功后关闭并刷新列表
// ---------------------------------------------------------------------------

const showCreateModal = ref(false)

const note = ref('')
const accountId = ref<string | null>(null)
const categoryId = ref<string | null>(null)
const merchantRef = ref<string | null>(null)
const amountYuan = ref('')
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

/** 订阅扣款为支出，分类候选仅支出类（树形）。 */
const categoryTreeOptions = computed(
  () => reference.treeCategoryOptions('expense') as unknown as TreeSelectOption[],
)

// 商户下拉选项（issue #190 / ADR-0028）：在用商户（与交易表单同款补全，未命中即建）。
const merchantOptions = computed<{ label: string; value: string }[]>(() =>
  reference.merchants.map((m) => ({ label: m.name, value: m.id })),
)

/**
 * 商户解析（保存时单点收口，issue #190）：「输入即建」交互——
 * 1. 空 → null（无商户）；
 * 2. 选中已有商户（value 为 id）→ 原样携带；
 * 3. 编辑未改动原商户（软删且超出会话缓存）→ 原样携带（后端保持历史引用语义）；
 * 4. 输入文本精确命中在用商户名 → 按名复用；
 * 5. 未命中 → `create_merchant` 即建；重名错误（store 陈旧竞态）先强制重拉
 *    按名复用，仍失败才向上抛。
 * `source` 为表单的商户 ref（新建/编辑弹窗各持一份）。
 */
async function resolveMerchantId(
  source: Ref<string | null>,
  editingMerchantId: string | null = null,
): Promise<string | null> {
  const ref = source.value
  if (!ref) return null
  if (reference.merchantMap.has(ref)) return ref
  if (editingMerchantId && ref === editingMerchantId) return ref
  const name = ref.trim()
  if (!name) return null
  const existing = reference.merchantByName.get(name)
  if (existing) return existing.id
  try {
    return await api.createMerchant({ name })
  } catch (e) {
    // 重名兜底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
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

/** 重置新建表单到初始态：模态语义下每次打开应是全新表单。 */
function resetCreateForm() {
  note.value = ''
  accountId.value = null
  categoryId.value = null
  merchantRef.value = null
  amountYuan.value = ''
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
  const amountCents = yuanToCents(amountYuan.value)
  if (amountCents === null || amountCents <= 0) {
    message.warning('请输入大于 0 的金额')
    return
  }
  try {
    await api.createScheduledTransaction({
      kind: 'subscription',
      account_id: accountId.value,
      category_id: categoryId.value,
      merchant_id: await resolveMerchantId(merchantRef),
      amount_cents: amountCents,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: null,
      start_date: startDate.value,
      note: note.value.trim() || null,
    })
    message.success('已创建订阅')
    showCreateModal.value = false
    resetCreateForm()
    await load()
    refreshSpend()
  } catch (e) {
    message.error(`创建失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 编辑订阅（issue #162，ADR-0023 决策三）：仅非金额字段（备注/账户/分类），
// 弹窗无金额输入；提交走订阅编辑命令，携带金额字段会被后端显式拒绝。
// 编辑不改已生成的期次与交易（期次执行时从计划读取这些字段），只影响未来。
// ---------------------------------------------------------------------------

const showEditModal = ref(false)
const editingId = ref<string | null>(null)
const editNote = ref('')
const editAccountId = ref<string | null>(null)
const editCategoryId = ref<string | null>(null)
const editMerchantRef = ref<string | null>(null)
/** 被编辑计划的当前商户 id（供 resolveMerchantId 保持历史引用判定）。 */
const editCurrentMerchantId = ref<string | null>(null)

// 编辑商户下拉（issue #190）：在用商户 + 原商户软删且超出会话缓存时追加兜底选项
// 承载原 id——裸 uuid 不可读，提交时按「未改动」语义原样保留。
const editMerchantOptions = computed<{ label: string; value: string }[]>(() => {
  const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }))
  const current = editCurrentMerchantId.value
  if (current && !reference.merchantMap.has(current)) {
    base.unshift({ label: '（已删除商户）', value: current })
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
    message.warning('请选择扣款账户')
    return
  }
  try {
    await api.updateScheduledSubscription({
      id: editingId.value,
      account_id: editAccountId.value,
      category_id: editCategoryId.value,
      merchant_id: await resolveMerchantId(editMerchantRef, editCurrentMerchantId.value),
      note: editNote.value.trim() || null,
    })
    message.success('已保存')
    showEditModal.value = false
    await load()
    refreshSpend()
  } catch (e) {
    message.error(`保存失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 清单：list_scheduled_transactions 过滤 subscription + 状态过滤；
// 下期扣款取 get_scheduled_transaction_detail 的最早 pending 期次（窗口外显示 —）
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 下期 pending 期次（无则为 null，占位「—」）。 */
interface SubscriptionRow {
  plan: ScheduledTransactionWithExt
  next: ScheduledTransactionOccurrence | null
  /** 详情命令失败：与「无 pending 期次」区分，不静默显示「—」。 */
  nextFailed?: boolean
}

const rows = ref<SubscriptionRow[]>([])
const loading = ref(false)
/** 清单状态过滤：默认只看进行中（issue #159 验收）。 */
const statusFilter = ref<'active' | 'paused' | 'cancelled'>('active')

const filteredRows = computed(() =>
  rows.value.filter((r) => r.plan.core.status === statusFilter.value),
)

async function load() {
  loading.value = true
  try {
    const plans = (await api.listScheduledTransactions()).filter(
      (p) => p.core.kind === 'subscription',
    )
    // 下期扣款来自既有详情命令的 pending 期次（ASC 排序，取首条）；
    // 预生成窗口之外不现场推算日期（避免第三套日期口径）
    const details = await Promise.all(
      plans.map(async (p) => {
        try {
          const d = await api.getScheduledTransactionDetail(p.core.id)
          // 后端按 scheduled_date ASC 返回；这里再取最早一条作为「下期」（仅选取，不推算）
          const next =
            [...d.pending_occurrences].sort((a, b) =>
              a.scheduled_date.localeCompare(b.scheduled_date),
            )[0] ?? null
          return { plan: p, next } satisfies SubscriptionRow
        } catch {
          return { plan: p, next: null, nextFailed: true } satisfies SubscriptionRow
        }
      }),
    )
    rows.value = details
  } catch (e) {
    message.error(`加载订阅失败: ${errorMessage(e)}`)
  } finally {
    loading.value = false
  }
}

// ---------------------------------------------------------------------------
// 状态操作：暂停 / 恢复 / 取消（走既有 update_scheduled_transaction_status）
// ---------------------------------------------------------------------------

async function changeStatus(id: string, newStatus: UpdateStatusInput['new_status']) {
  try {
    await api.updateScheduledTransactionStatus({ id, new_status: newStatus })
    message.success(newStatus === 'paused' ? '已暂停' : newStatus === 'active' ? '已恢复' : '已取消')
    await load()
    refreshSpend()
  } catch (e) {
    message.error(`操作失败: ${errorMessage(e)}`)
  }
}

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：三页签通用组件，订阅页签同享；
// 弹窗内重试成功会发 changed，清单与花费面板随之刷新
// ---------------------------------------------------------------------------

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null)

function openDetail(row: SubscriptionRow) {
  void planDetailRef.value?.open(row.plan.core.id)
}

async function onDetailChanged() {
  await load()
  refreshSpend()
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

function recurrenceLabel(row: SubscriptionRow): string {
  const { recurrence_type, recurrence_interval } = row.plan.core
  const unit = recurrenceUnit[recurrence_type as RecurrenceType] ?? recurrence_type
  return recurrence_interval > 1 ? `每${recurrence_interval}${unit}` : `每${unit}`
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status)
}

const filterOptions = [
  { key: 'active' as const, label: '进行中' },
  { key: 'paused' as const, label: '已暂停' },
  { key: 'cancelled' as const, label: '已取消' },
]

const columns: DataTableColumns<SubscriptionRow> = [
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
    title: '金额',
    key: 'amount',
    render: (row) => formatAmount(row.plan.core.amount_cents, reference.getCurrency(row.plan.core.currency_code)),
  },
  { title: '周期', key: 'recurrence', render: recurrenceLabel },
  { title: '开始日', key: 'start_date', render: (row) => row.plan.core.start_date },
  { title: '状态', key: 'status', render: (row) => statusLabel(row.plan.core.status) },
  {
    title: '下期扣款',
    key: 'next',
    // 测试锚点：无 pending 期次（预生成窗口之外）时断言占位，不现场推算日期
    render: (row) =>
      h(
        'span',
        { 'data-testid': `next-charge-${row.plan.core.id}` },
        row.next
          ? `${row.next.scheduled_date} · ${formatAmount(row.next.amount_cents, reference.getCurrency(row.plan.core.currency_code))}`
          : row.nextFailed
            ? '加载失败'
            : '—',
      ),
  },
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
      if (status === 'active' || status === 'paused') {
        // 编辑仅非金额字段（issue #162，ADR-0023 决策三）；已取消不提供编辑
        buttons.push(
          h(
            NButton,
            {
              size: 'tiny',
              'data-testid': `op-edit-${row.plan.core.id}`,
              onClick: () => openEdit(row),
            },
            () => '编辑',
          ),
        )
      }
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
        // 取消不删已生成交易与历史期次（后端行为），二次确认防误触
        buttons.push(
          h(
            AppPopconfirm,
            { onPositiveClick: () => changeStatus(row.plan.core.id, 'cancelled') },
            {
              default: () => '取消后不再扣款，已生成的交易与历史期次保留。确认取消？',
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
  void load()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="订阅清单" size="small">
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
            data-testid="sub-create-open"
            @click="showCreateModal = true"
          >
            新建订阅
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
      title="新建订阅"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <!-- 布局对齐记一笔表单（CategoryForm）：NSpace 提供行距，宽度取 160-280 档；
           金额+币种、周期+间隔 各并一行减少行数 -->
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem label="备注">
            <NInput
              v-model:value="note"
              data-testid="sub-note"
              placeholder="服务名称，如：视频会员"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem label="扣款账户">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              placeholder="选择账户"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem label="分类">
            <AppTreeSelect
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
              data-testid="sub-merchant"
            />
          </NFormItem>
          <NFormItem label="金额">
            <NInput
              v-model:value="amountYuan"
              data-testid="sub-amount"
              placeholder="每期金额"
              style="width: 160px"
            />
            <AppSelect
              v-model:value="currencyCode"
              :options="currencyOptions"
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
                :options="recurrenceOptions"
                style="width: 100px"
              />
            </NSpace>
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
            <NButton data-testid="sub-create-cancel" @click="showCreateModal = false">取消</NButton>
            <NButton type="primary" data-testid="sub-create" @click="create">创建订阅</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 编辑订阅弹窗（issue #162）：仅非金额字段（备注/账户/分类），无金额输入 -->
    <AppModal
      v-model:show="showEditModal"
      title="编辑订阅"
      preset="card"
      display-directive="if"
      style="width: 480px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem label="备注">
            <NInput
              v-model:value="editNote"
              data-testid="sub-edit-note"
              placeholder="服务名称"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem label="扣款账户">
            <PinyinSelect
              v-model:value="editAccountId"
              :options="accountOptions"
              placeholder="选择账户"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem label="分类">
            <AppTreeSelect
              v-model:value="editCategoryId"
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
              v-model:value="editMerchantRef"
              :options="editMerchantOptions"
              tag
              clearable
              placeholder="选择商户，可直接输入新名称"
              style="width: 220px"
              data-testid="sub-edit-merchant"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="sub-edit-cancel" @click="showEditModal = false">取消</NButton>
            <NButton type="primary" data-testid="sub-edit-save" @click="saveEdit">保存</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <SubscriptionSpendPanel ref="spendPanelRef" />

    <!-- 期次详情弹窗（issue #205）：三页签通用，订阅页签同享 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
