<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
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
  NModal,
  NColorPicker,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { Category, CategoryInput, CategoryUpdateInput, CategoryKind } from '@/types'
import { categoryChildren } from '@/types/category'

const store = useAppStore()
const message = useMessage()

// ── 添加表单 ──
const name = ref('')
const kind = ref<CategoryKind>('expense')
const parentId = ref<string>('')
const icon = ref('')
const color = ref('')

const kindOptions = [
  { label: '支出', value: 'expense' },
  { label: '收入', value: 'income' },
]

const parentOptions = computed(() => [
  { label: '无（顶级）', value: '' },
  ...store.rootCategories
    .filter((c) => c.kind === kind.value)
    .map((c) => ({ label: c.name, value: c.id })),
])

watch(kind, () => {
  parentId.value = ''
})

async function addCategory() {
  if (!name.value.trim()) {
    message.warning('请输入分类名称')
    return
  }
  const parent_id: string | null = parentId.value || null
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
    icon: icon.value || null,
    color: color.value || null,
  }
  try {
    await api.createCategory(input)
    message.success('已添加分类')
    name.value = ''
    parentId.value = ''
    icon.value = ''
    color.value = ''
    await store.loadCategories()
  } catch (e) {
    message.error(`添加失败: ${e}`)
  }
}

// ── 编辑弹窗 ──
const showEditModal = ref(false)
const editingCategory = ref<Category | null>(null)
const editName = ref('')
const editIcon = ref('')
const editColor = ref('')
const editParentId = ref<string>('')

function openEdit(cat: Category) {
  editingCategory.value = cat
  editName.value = cat.name
  editIcon.value = cat.icon ?? ''
  editColor.value = cat.color ?? ''
  editParentId.value = cat.parent_id ?? ''
  showEditModal.value = true
}

async function saveEdit() {
  const cat = editingCategory.value
  if (!cat) return
  if (!editName.value.trim()) {
    message.warning('请输入分类名称')
    return
  }
  const input: CategoryUpdateInput = {
    name: editName.value,
    icon: editIcon.value || null,
    color: editColor.value || null,
    parent_id: editParentId.value || null,
  }
  try {
    await api.updateCategory(cat.id, input)
    message.success('已更新分类')
    showEditModal.value = false
    await store.loadCategories()
  } catch (e) {
    message.error(`更新失败: ${e}`)
  }
}

// ── 删除 ──
async function removeCategory(id: string) {
  try {
    await api.deleteCategory(id)
    message.success('已删除')
    await store.loadCategories()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

// ── 树形展平数据 ──
interface CategoryRow extends Category {
  depth: number
  hasChildren: boolean
}

const treeData = computed<CategoryRow[]>(() => {
  const result: CategoryRow[] = []
  const cats = store.categories
  for (const root of cats.filter((c) => c.parent_id == null).sort((a, b) => a.sort_order - b.sort_order)) {
    result.push({ ...root, depth: 0, hasChildren: false })
    const children = categoryChildren(cats, root.id).sort((a, b) => a.sort_order - b.sort_order)
    for (const child of children) {
      result.push({ ...child, depth: 1, hasChildren: false })
    }
  }
  return result
})

const draggedRow = ref<CategoryRow | null>(null)
const dragOverRowId = ref<string | null>(null)

async function handleDrop(target: CategoryRow) {
  const source = draggedRow.value
  if (!source || source.id === target.id) return

  if (source.depth !== target.depth) return
  if (source.kind !== target.kind) return
  if (source.parent_id !== target.parent_id) return

  const groupItems = treeData.value.filter(
    (r) => r.depth === source.depth && r.kind === source.kind && r.parent_id === source.parent_id,
  )

  const sorted = [...groupItems]
  const sourceIdx = sorted.findIndex((r) => r.id === source.id)
  const targetIdx = sorted.findIndex((r) => r.id === target.id)
  if (sourceIdx === -1 || targetIdx === -1) return

  const [item] = sorted.splice(sourceIdx, 1)
  sorted.splice(targetIdx, 0, item)

  const items = sorted.map((r, i) => ({
    id: r.id,
    sort_order: i,
  }))

  try {
    await api.reorderCategories(items)
    await store.loadCategories()
  } catch (e) {
    message.error(`排序失败: ${e}`)
  }
}

function getRowProps(row: CategoryRow) {
  return {
    draggable: 'true' as const,
    'data-category-id': row.id,
    class: dragOverRowId.value === row.id ? 'drag-over' : undefined,
    onDragstart: (e: DragEvent) => {
      draggedRow.value = row
      if (e.dataTransfer) {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData('text/plain', row.id)
      }
    },
    onDragover: (e: DragEvent) => {
      e.preventDefault()
      if (e.dataTransfer) {
        e.dataTransfer.dropEffect = 'move'
      }
      dragOverRowId.value = row.id
    },
    onDragleave: () => {
      if (dragOverRowId.value === row.id) {
        dragOverRowId.value = null
      }
    },
    onDrop: (e: DragEvent) => {
      e.preventDefault()
      dragOverRowId.value = null
      handleDrop(row)
    },
    onDragend: () => {
      draggedRow.value = null
      dragOverRowId.value = null
    },
  }
}

// ── 表格列定义 ──
const categoryColumns: DataTableColumns<CategoryRow> = [
  {
    title: '',
    key: 'drag-handle',
    width: 32,
    render: () => h('span', { style: { cursor: 'grab', fontSize: '16px', userSelect: 'none' } }, '☰'),
  },
  {
    title: '',
    key: 'icon',
    width: 44,
    render: (row) =>
      h('span', {
        style: { marginLeft: `${row.depth * 20}px`, fontSize: '18px' },
      }, row.icon ?? '📁'),
  },
  {
    title: '',
    key: 'color',
    width: 32,
    render: (row) =>
      row.color
        ? h('span', {
            style: {
              display: 'inline-block',
              width: '14px',
              height: '14px',
              borderRadius: '3px',
              backgroundColor: row.color,
              verticalAlign: 'middle',
            },
          })
        : null,
  },
  {
    title: '名称',
    key: 'name',
    render: (row) => row.name,
  },
  {
    title: '类型',
    key: 'kind',
    width: 80,
    render: (row) =>
      row.kind === 'income'
        ? h(NTag, { type: 'success', size: 'tiny' }, () => '收入')
        : h(NTag, { type: 'warning', size: 'tiny' }, () => '支出'),
  },
  {
    title: '操作',
    key: 'actions',
    width: 120,
    render: (row) =>
      h(NSpace, { size: 'small' }, () => [
        h(NButton, {
          size: 'tiny',
          quaternary: true,
          onClick: () => openEdit(row),
        }, () => '编辑'),
        h(NPopconfirm, {
          onPositiveClick: () => removeCategory(row.id),
        }, {
          default: () => '确认删除？',
          trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => '删除'),
        }),
      ]),
  },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增分类" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="分类名称" style="width: 140px" />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect v-model:value="kind" :options="kindOptions" style="width: 100px" />
        </NFormItem>
        <NFormItem label="父分类">
          <NSelect v-model:value="parentId" :options="parentOptions" style="width: 140px" />
        </NFormItem>
        <NFormItem label="图标">
          <NInput v-model:value="icon" placeholder="emoji" style="width: 80px" />
        </NFormItem>
        <NFormItem label="颜色">
          <NColorPicker v-model:value="color" size="small" style="width: 100px" />
        </NFormItem>
        <NButton type="primary" @click="addCategory">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="分类列表" size="small">
      <NDataTable :columns="categoryColumns" :data="treeData" :row-props="getRowProps" :bordered="false" size="small" :single-line="false" />
    </NCard>

    <NModal v-model:show="showEditModal" title="编辑分类" preset="card" style="width: 420px" :bordered="false">
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NFormItem label="名称">
          <NInput v-model:value="editName" placeholder="分类名称" />
        </NFormItem>
        <NFormItem label="图标">
          <NInput v-model:value="editIcon" placeholder="emoji" style="width: 120px" />
        </NFormItem>
        <NFormItem label="颜色">
          <NColorPicker v-model:value="editColor" size="small" style="width: 140px" />
        </NFormItem>
        <NFormItem label="父分类">
          <NSelect
            v-model:value="editParentId"
            :options="parentOptions"
            placeholder="选择父分类"
            clearable
            style="width: 200px"
          />
        </NFormItem>
        <NButton type="primary" block @click="saveEdit">保存</NButton>
      </NForm>
    </NModal>
  </NSpace>
</template>

<style scoped>
:deep(tr.drag-over) {
  outline: 2px dashed #18a058;
  outline-offset: -2px;
  background-color: rgba(24, 160, 58, 0.04);
}
</style>
