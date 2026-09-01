<script setup lang="ts">
import { h, computed, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInputNumber,
  NSpace,
  NProgress,
  NTag,
  NSpin,
  NText,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
import AppModal from '@/components/AppModal.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useReferenceStore } from '@/stores/reference'
import { errorMessage } from '@/utils/errors'
import { yuanToCents } from '@/utils/money'
import { todayStr } from '@/utils/date'
import { formatAmount, centsToYuan } from '@/types'
import type { BudgetInput, BudgetProgress } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const list = ref<BudgetProgress[]>([])
const loading = ref(false)

const categoryId = ref<string | null>(null)
const amount = ref<number | null>(null)

// 编辑弹窗（issue #184）：仅金额可改，分类/周期不可改（改法为删旧建新）
const showEdit = ref(false)
const editing = ref<BudgetProgress | null>(null)
const editAmount = ref<number | null>(null)

const categoryOptions = () =>
  reference.rootCategories
    .filter((c) => c.kind === 'expense')
    .map((c) => ({ label: c.name, value: c.id }))

async function refresh() {
  loading.value = true
  try {
    list.value = await api.budgetProgress()
  } finally {
    loading.value = false
  }
}

async function create() {
  if (!categoryId.value || amount.value == null) {
    message.warning(t('budget.message.required'))
    return
  }
  if (amount.value <= 0) {
    message.warning(t('budget.message.positive'))
    return
  }
  const input: BudgetInput = {
    category_id: categoryId.value,
    amount_cents: yuanToCents(amount.value) ?? 0,
    // start_date 已退化为记录字段（永久滚动预算，进度与日期无关），传创建当日（本地日历日）即可
    start_date: todayStr(),
  }
  try {
    await api.createBudget(input)
    message.success(t('budget.message.created'))
    categoryId.value = null
    amount.value = null
    await refresh()
  } catch (e) {
    // 后端拒绝（金额非正/收入分类/同分类同周期重复）时把错误信息清晰呈现给用户；
    // 查重提示自带「可编辑该预算的金额」引导
    message.error(t('budget.message.createFailed', { message: errorMessage(e) }))
  }
}

function openEdit(row: BudgetProgress) {
  editing.value = row
  editAmount.value = centsToYuan(row.budget.amount_cents)
  showEdit.value = true
}

async function saveEdit() {
  if (!editing.value) return
  if (editAmount.value == null || editAmount.value <= 0) {
    message.warning(t('budget.message.positive'))
    return
  }
  try {
    await api.updateBudget(editing.value.budget.id, {
      amount_cents: yuanToCents(editAmount.value) ?? 0,
    })
    message.success(t('budget.message.updated'))
    showEdit.value = false
    await refresh()
  } catch (e) {
    message.error(t('budget.message.updateFailed', { message: errorMessage(e) }))
  }
}

async function remove(id: string) {
  try {
    await api.deleteBudget(id)
    message.success(t('budget.message.deleted'))
    await refresh()
  } catch (e) {
    message.error(t('budget.message.deleteFailed', { message: errorMessage(e) }))
  }
}

const columns = computed<DataTableColumns<BudgetProgress>>(() => [
  { title: t('budget.list.colCategory'), key: 'category_name' },
  { title: t('budget.list.colPeriod'), key: 'budget.period' },
  {
    title: t('budget.list.colAmount'),
    key: 'budget.amount_cents',
    render: (row) => formatAmount(row.budget.amount_cents),
  },
  {
    title: t('budget.list.colSpent'),
    key: 'spent_cents',
    render: (row) => formatAmount(row.spent_cents),
  },
  {
    title: t('budget.list.colProgress'),
    key: 'progress',
    render: (row) => {
      const pct = row.budget.amount_cents > 0
        ? Math.min(100, Math.round((row.spent_cents / row.budget.amount_cents) * 100))
        : 0
      return h(NProgress, {
        type: 'line',
        percentage: pct,
        status: row.over_budget ? 'error' : 'success',
      })
    },
  },
  {
    title: t('budget.list.colStatus'),
    key: 'over_budget',
    width: 80,
    render: (row) =>
      row.over_budget
        ? h(NTag, { type: 'error' }, () => t('budget.status.over'))
        : h(NTag, { type: 'success' }, () => t('budget.status.normal')),
  },
  {
    title: t('budget.list.colActions'),
    key: 'actions',
    width: 130,
    render: (row) =>
      h(NSpace, { size: 4, wrap: false }, () => [
        h(
          NButton,
          { size: 'tiny', type: 'primary', quaternary: true, onClick: () => openEdit(row) },
          () => t('budget.actions.edit'),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => remove(row.budget.id) },
          {
            default: () => t('budget.actions.confirmDelete'),
            trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => t('budget.actions.delete')),
          },
        ),
      ]),
  },
])

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NCard :title="t('budget.create.title')" size="small">
        <NForm label-placement="left" :show-feedback="false" inline size="small">
          <NFormItem :label="t('budget.create.category')">
            <PinyinSelect
              v-model:value="categoryId"
              :options="categoryOptions()"
              :placeholder="t('budget.create.categoryPlaceholder')"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('budget.create.amount')">
            <NInputNumber v-model:value="amount" :precision="2" style="width: 140px" />
          </NFormItem>
          <NButton type="primary" @click="create">{{ t('budget.create.add') }}</NButton>
        </NForm>
      </NCard>

      <NCard :title="t('budget.list.title')" size="small">
        <NDataTable :columns="columns" :data="list" :bordered="false" size="small" />
      </NCard>
    </NSpace>

    <!-- 编辑弹窗（issue #184）：仅金额可改，分类/周期只读 -->
    <AppModal
      v-model:show="showEdit"
      :title="t('budget.edit.title')"
      preset="card"
      display-directive="if"
      style="width: 380px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NFormItem :label="t('budget.edit.category')">
          <NText>{{ editing?.category_name }}</NText>
        </NFormItem>
        <NFormItem :label="t('budget.edit.period')">
          <NText>{{
            editing ? t(`budget.period.${editing.budget.period}`) : ''
          }}</NText>
        </NFormItem>
        <NFormItem :label="t('budget.edit.amount')">
          <NInputNumber v-model:value="editAmount" :precision="2" style="width: 100%" />
        </NFormItem>
      </NForm>
      <template #footer>
        <NSpace justify="end">
          <NButton size="small" @click="showEdit = false">{{ t('budget.edit.cancel') }}</NButton>
          <NButton size="small" type="primary" @click="saveEdit">{{ t('budget.edit.save') }}</NButton>
        </NSpace>
      </template>
    </AppModal>
  </NSpin>
</template>
