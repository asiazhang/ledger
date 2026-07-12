<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import {
  NCard,
  NButton,
  NTree,
  NIcon,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NSpace,
  NPopconfirm,
  NModal,
  NTabs,
  NTabPane,
  useMessage,
  type TreeOption,
  type TreeDropInfo,
} from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { Category, CategoryInput, CategoryUpdateInput, CategoryKind } from '@/types'
import { getIconComponent } from '@/types/icon'

const store = useAppStore()
const message = useMessage()

interface TreeCategoryNode extends TreeOption {
  category: Category
}

const activeKind = ref<CategoryKind>('expense')

// ── 树形数据（按 activeKind 过滤）─
const treeData = computed<TreeCategoryNode[]>(() => {
  const roots = store.categories
    .filter((c) => c.parent_id == null && c.kind === activeKind.value)
    .sort((a, b) => a.sort_order - b.sort_order)
  return roots.map((root) => {
    const children = store.categories
      .filter((c) => c.parent_id === root.id)
      .sort((a, b) => a.sort_order - b.sort_order)
    return {
      key: root.id,
      label: root.name,
      category: root,
      children: children.length > 0
        ? children.map((c) => ({ key: c.id, label: c.name, category: c }))
        : undefined,
    }
  })
})

// ── 自定义渲染 ──
function renderIcon(name: string | null) {
  const Comp = getIconComponent(name)
  if (!Comp) return null
  return h(NIcon, { size: 18, style: { marginRight: '6px', verticalAlign: 'middle' } }, { default: () => h(Comp) })
}

function renderPrefix(info: { option: TreeOption }) {
  const cat = (info.option as TreeCategoryNode).category
  return renderIcon(cat.icon)
}

function renderSuffix(info: { option: TreeOption }) {
  const cat = (info.option as TreeCategoryNode).category
  return h(NSpace, { size: 'small', align: 'center' }, () => [
    h(NButton, {
      size: 'tiny',
      quaternary: true,
      onClick: (e: MouseEvent) => {
        e.stopPropagation()
        openEdit(cat)
      },
    }, () => '编辑'),
    h(NPopconfirm, {
      onPositiveClick: () => removeCategory(cat.id),
    }, {
      default: () => '确认删除？',
      trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true, onClick: (e: MouseEvent) => e.stopPropagation() }, () => '删除'),
    }),
  ])
}

// ── 拖拽排序 ──
async function handleDrop(info: TreeDropInfo) {
  const { node: targetNode, dragNode, dropPosition } = info
  if (dropPosition === 'inside') return

  const dragCat = (dragNode as TreeCategoryNode).category
  const targetCat = (targetNode as TreeCategoryNode).category
  if (dragCat.parent_id !== targetCat.parent_id) return
  if (dragCat.kind !== targetCat.kind) return

  const siblings = store.categories
    .filter((c) => c.parent_id === dragCat.parent_id && c.kind === dragCat.kind)
    .sort((a, b) => a.sort_order - b.sort_order)

  const fromIdx = siblings.findIndex((c) => c.id === dragCat.id)
  let toIdx = siblings.findIndex((c) => c.id === targetCat.id)
  if (fromIdx === -1 || toIdx === -1) return
  if (dropPosition === 'after') toIdx++

  const [moved] = siblings.splice(fromIdx, 1)
  if (fromIdx < toIdx) toIdx--
  siblings.splice(toIdx, 0, moved)

  try {
    await api.reorderCategories(siblings.map((c, i) => ({ id: c.id, sort_order: i })))
    await store.loadCategories()
  } catch (e) {
    message.error(`排序失败: ${e}`)
  }
}

// ── 添加表单 ──
const name = ref('')
const parentId = ref<string>('')
const icon = ref('')

const parentOptions = computed(() => [
  { label: '无（顶级）', value: '' },
  ...store.rootCategories
    .filter((c) => c.kind === activeKind.value)
    .map((c) => ({ label: c.name, value: c.id })),
])

watch(activeKind, () => {
  name.value = ''
  parentId.value = ''
  icon.value = ''
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
    if (parent.kind !== activeKind.value) {
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
    kind: activeKind.value,
    parent_id,
    icon: icon.value || null,
  }
  try {
    await api.createCategory(input)
    message.success('已添加分类')
    name.value = ''
    parentId.value = ''
    icon.value = ''
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
const editParentId = ref<string>('')

const editParentOptions = computed(() => [
  { label: '无（顶级）', value: '' },
  ...store.rootCategories
    .filter((c) => c.kind === editingCategory.value?.kind)
    .map((c) => ({ label: c.name, value: c.id })),
])

function openEdit(cat: Category) {
  editingCategory.value = cat
  editName.value = cat.name
  editIcon.value = cat.icon ?? ''
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
</script>

<template>
  <NTabs type="line" :value="activeKind" @update:value="(v) => activeKind = v as CategoryKind">
    <NTabPane name="expense" tab="支出">
      <NSpace vertical :size="16">
        <NCard title="新增分类" size="small">
          <NForm label-placement="left" :show-feedback="false" inline size="small">
            <NFormItem label="名称">
              <NInput v-model:value="name" placeholder="分类名称" style="width: 140px" />
            </NFormItem>
            <NFormItem label="父分类">
              <NSelect v-model:value="parentId" :options="parentOptions" style="width: 140px" />
            </NFormItem>
            <NFormItem label="图标">
              <NInput v-model:value="icon" placeholder="图标名" style="width: 140px" />
            </NFormItem>
            <NButton type="primary" @click="addCategory">添加</NButton>
          </NForm>
        </NCard>

        <NCard title="分类列表" size="small">
          <NTree
            :data="treeData"
            :render-prefix="renderPrefix"
            :render-suffix="renderSuffix"
            draggable
            block-line
            :default-expand-all="true"
            @drop="handleDrop"
          />
        </NCard>
      </NSpace>
    </NTabPane>
    <NTabPane name="income" tab="收入">
      <NSpace vertical :size="16">
        <NCard title="新增分类" size="small">
          <NForm label-placement="left" :show-feedback="false" inline size="small">
            <NFormItem label="名称">
              <NInput v-model:value="name" placeholder="分类名称" style="width: 140px" />
            </NFormItem>
            <NFormItem label="父分类">
              <NSelect v-model:value="parentId" :options="parentOptions" style="width: 140px" />
            </NFormItem>
            <NFormItem label="图标">
              <NInput v-model:value="icon" placeholder="图标名" style="width: 140px" />
            </NFormItem>
            <NButton type="primary" @click="addCategory">添加</NButton>
          </NForm>
        </NCard>

        <NCard title="分类列表" size="small">
          <NTree
            :data="treeData"
            :render-prefix="renderPrefix"
            :render-suffix="renderSuffix"
            draggable
            block-line
            :default-expand-all="true"
            @drop="handleDrop"
          />
        </NCard>
      </NSpace>
    </NTabPane>
  </NTabs>

  <NModal v-model:show="showEditModal" title="编辑分类" preset="card" style="width: 420px" :bordered="false">
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem label="名称">
        <NInput v-model:value="editName" placeholder="分类名称" />
      </NFormItem>
      <NFormItem label="图标">
        <NInput v-model:value="editIcon" placeholder="图标名" style="width: 120px" />
      </NFormItem>
      <NFormItem label="父分类">
        <NSelect
          v-model:value="editParentId"
          :options="editParentOptions"
          placeholder="选择父分类"
          clearable
          style="width: 200px"
        />
      </NFormItem>
      <NButton type="primary" block @click="saveEdit">保存</NButton>
    </NForm>
  </NModal>
</template>
