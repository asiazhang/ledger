<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
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
// -1 表示无父分类（创建顶级）
const parentId = ref<number>(-1)

const kindOptions = [
  { label: '支出', value: 'expense' },
  { label: '收入', value: 'income' },
]

const parentOptions = computed(() => [
  { label: '无（顶级）', value: -1 },
  ...store.rootCategories
    .filter((c) => c.kind === kind.value)
    .map((c) => ({ label: c.name, value: c.id })),
])

// 顶级在前、二级紧跟其父的排序，便于查看层级
const sortedCategories = computed<Category[]>(() => {
  const result: Category[] = []
  const roots = store.rootCategories.slice().sort((a, b) => a.id - b.id)
  for (const root of roots) {
    result.push(root)
    result.push(...store.categoryChildren(root.id).sort((a, b) => a.id - b.id))
  }
  return result
})

watch(kind, () => {
  parentId.value = -1
})

async function addCategory() {
  if (!name.value.trim()) {
    message.warning('请输入分类名称')
    return
  }
  // 选了父分类时校验：父必须存在、同 kind、本身为顶级（防三级嵌套）
  const parent_id: number | null = parentId.value === -1 ? null : parentId.value
  if (parent_id != null) {
    const parent = store.categoryMap.get(parent_id)
    if (!parent) {
      message.warning('父分类不存在')
      return
    }
    if (parent.kind !== kind.value) {
      message.warning('父分类类型需一致')
      return
    }
    if (parent.parent_id != null) {
      message.warning('父分类必须为顶级')
      return
    }
  }
  const input: CategoryInput = {
    name: name.value,
    kind: kind.value,
    parent_id,
  }
  try {
    await api.createCategory(input)
    message.success('已添加分类')
    name.value = ''
    parentId.value = -1
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
    title: '父分类',
    key: 'parent_id',
    width: 120,
    render: (row) =>
      row.parent_id == null
        ? '—'
        : (store.categoryMap.get(row.parent_id)?.name ?? '-'),
  },
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
        <NFormItem label="父分类">
          <NSelect v-model:value="parentId" :options="parentOptions" style="width: 160px" />
        </NFormItem>
        <NButton type="primary" @click="addCategory">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="分类列表" size="small">
      <NDataTable :columns="categoryColumns" :data="sortedCategories" :bordered="false" size="small" />
    </NCard>

    <NCard title="币种" size="small">
      <NDataTable :columns="currencyColumns" :data="store.currencies" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
