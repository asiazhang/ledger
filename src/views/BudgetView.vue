<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInputNumber,
  NModal,
  NSpace,
  NPopconfirm,
  NProgress,
  NTag,
  NSpin,
  NText,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useReferenceStore } from '@/stores/reference'
import { errorMessage } from '@/utils/errors'
import { formatAmount, centsToYuan } from '@/types'
import { BUDGET_PERIOD_LABELS } from '@/types/budget'
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
    message.warning('请填写分类和金额')
    return
  }
  if (amount.value <= 0) {
    message.warning('预算金额必须为正数')
    return
  }
  const input: BudgetInput = {
    category_id: categoryId.value,
    amount_cents: Math.round(amount.value * 100),
    // start_date 已退化为记录字段（永久滚动预算，进度与日期无关），传创建当日即可
    start_date: new Date().toISOString().slice(0, 10),
  }
  try {
    await api.createBudget(input)
    message.success('已创建预算')
    categoryId.value = null
    amount.value = null
    await refresh()
  } catch (e) {
    // 后端拒绝（金额非正/收入分类/同分类同周期重复）时把中文错误清晰呈现给用户；
    // 查重提示自带「可编辑该预算的金额」引导
    message.error(`创建失败: ${errorMessage(e)}`)
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
    message.warning('预算金额必须为正数')
    return
  }
  try {
    await api.updateBudget(editing.value.budget.id, {
      amount_cents: Math.round(editAmount.value * 100),
    })
    message.success('已更新预算')
    showEdit.value = false
    await refresh()
  } catch (e) {
    message.error(`更新失败: ${errorMessage(e)}`)
  }
}

async function remove(id: string) {
  try {
    await api.deleteBudget(id)
    message.success('已删除')
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${errorMessage(e)}`)
  }
}

const columns: DataTableColumns<BudgetProgress> = [
  { title: '分类', key: 'category_name' },
  { title: '周期', key: 'budget.period' },
  {
    title: '预算',
    key: 'budget.amount_cents',
    render: (row) => formatAmount(row.budget.amount_cents),
  },
  {
    title: '已支出',
    key: 'spent_cents',
    render: (row) => formatAmount(row.spent_cents),
  },
  {
    title: '进度',
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
    title: '状态',
    key: 'over_budget',
    width: 80,
    render: (row) =>
      row.over_budget ? h(NTag, { type: 'error' }, () => '超支') : h(NTag, { type: 'success' }, () => '正常'),
  },
  {
    title: '操作',
    key: 'actions',
    width: 130,
    render: (row) =>
      h(NSpace, { size: 4, wrap: false }, () => [
        h(
          NButton,
          { size: 'tiny', type: 'primary', quaternary: true, onClick: () => openEdit(row) },
          () => '编辑',
        ),
        h(
          NPopconfirm,
          { onPositiveClick: () => remove(row.budget.id) },
          {
            default: () => '确认删除？',
            trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
          },
        ),
      ]),
  },
]

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NCard title="新增预算" size="small">
        <NForm label-placement="left" :show-feedback="false" inline size="small">
          <NFormItem label="分类">
            <PinyinSelect
              v-model:value="categoryId"
              :options="categoryOptions()"
              placeholder="支出分类"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem label="金额">
            <NInputNumber v-model:value="amount" :precision="2" style="width: 140px" />
          </NFormItem>
          <NButton type="primary" @click="create">添加</NButton>
        </NForm>
      </NCard>

      <NCard title="预算执行" size="small">
        <NDataTable :columns="columns" :data="list" :bordered="false" size="small" />
      </NCard>
    </NSpace>

    <!-- 编辑弹窗（issue #184）：仅金额可改，分类/周期只读 -->
    <NModal
      v-model:show="showEdit"
      title="编辑预算"
      preset="card"
      display-directive="if"
      style="width: 380px"
      :bordered="false"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NFormItem label="分类">
          <NText>{{ editing?.category_name }}</NText>
        </NFormItem>
        <NFormItem label="周期">
          <NText>{{
            editing ? BUDGET_PERIOD_LABELS[editing.budget.period] : ''
          }}</NText>
        </NFormItem>
        <NFormItem label="金额">
          <NInputNumber v-model:value="editAmount" :precision="2" style="width: 100%" />
        </NFormItem>
      </NForm>
      <template #footer>
        <NSpace justify="end">
          <NButton size="small" @click="showEdit = false">取消</NButton>
          <NButton size="small" type="primary" @click="saveEdit">保存</NButton>
        </NSpace>
      </template>
    </NModal>
  </NSpin>
</template>
