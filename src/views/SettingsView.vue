<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NSpace,
  NPopconfirm,
  NTag,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { Category, CategoryInput, CategoryKind } from '@/types'

const store = useAppStore()
const message = useMessage()

const name = ref('')
const kind = ref<CategoryKind>('expense')

const kindOptions = [
  { label: '支出', value: 'expense' },
  { label: '收入', value: 'income' },
]

async function addCategory() {
  if (!name.value.trim()) {
    message.warning('请输入分类名称')
    return
  }
  const input: CategoryInput = { name: name.value, kind: kind.value }
  try {
    await api.createCategory(input)
    message.success('已添加分类')
    name.value = ''
    await store.loadCategories()
  } catch (e) {
    message.error(`添加失败: ${e}`)
  }
}

async function removeCategory(id: number) {
  try {
    await api.deleteCategory(id)
    message.success('已删除')
    await store.loadCategories()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

const categoryColumns: DataTableColumns<Category> = [
  { title: '名称', key: 'name' },
  {
    title: '类型',
    key: 'kind',
    width: 80,
    render: (row) =>
      row.kind === 'income'
        ? h(NTag, { type: 'success' }, () => '收入')
        : h(NTag, { type: 'warning' }, () => '支出'),
  },
  {
    title: '操作',
    key: 'actions',
    width: 80,
    render: (row) =>
      h(
        NPopconfirm,
        { onPositiveClick: () => removeCategory(row.id) },
        {
          default: () => '确认删除？',
          trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        },
      ),
  },
]

const currencyColumns = [
  { title: '代码', key: 'code', width: 80 },
  { title: '名称', key: 'name' },
  { title: '符号', key: 'symbol', width: 80 },
  { title: '小数位', key: 'decimal_places', width: 80 },
]

onMounted(async () => {
  await store.loadAll()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增分类" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="分类名称" style="width: 160px" />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect v-model:value="kind" :options="kindOptions" style="width: 120px" />
        </NFormItem>
        <NButton type="primary" @click="addCategory">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="分类列表" size="small">
      <NDataTable :columns="categoryColumns" :data="store.categories" :bordered="false" size="small" />
    </NCard>

    <NCard title="币种" size="small">
      <NDataTable :columns="currencyColumns" :data="store.currencies" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
