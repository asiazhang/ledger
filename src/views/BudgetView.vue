<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInputNumber,
  NDatePicker,
  NSpace,
  NPopconfirm,
  NProgress,
  NTag,
  NSpin,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useReferenceStore } from '@/stores/reference'
import { errorMessage } from '@/utils/errors'
import { formatAmount } from '@/types'
import type { BudgetInput, BudgetProgress } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const list = ref<BudgetProgress[]>([])
const loading = ref(false)

const categoryId = ref<string | null>(null)
const amount = ref<number | null>(null)
const startDate = ref(Date.now())

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
    start_date: new Date(startDate.value).toISOString().slice(0, 7) + '-01',
  }
  try {
    await api.createBudget(input)
    message.success('已创建预算')
    categoryId.value = null
    amount.value = null
    await refresh()
  } catch (e) {
    // 后端拒绝（金额非正/收入分类/同分类同周期重复）时把中文错误清晰呈现给用户
    message.error(`创建失败: ${errorMessage(e)}`)
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
    width: 80,
    render: (row) =>
      h(
        NPopconfirm,
        { onPositiveClick: () => remove(row.budget.id) },
        {
          default: () => '确认删除？',
          trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        },
      ),
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
          <NFormItem label="起始">
            <NDatePicker v-model:value="startDate" type="month" style="width: 160px" />
          </NFormItem>
          <NButton type="primary" @click="create">添加</NButton>
        </NForm>
      </NCard>

      <NCard title="预算执行" size="small">
        <NDataTable :columns="columns" :data="list" :bordered="false" size="small" />
      </NCard>
    </NSpace>
  </NSpin>
</template>
