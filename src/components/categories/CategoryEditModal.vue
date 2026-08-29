<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NModal, useMessage } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import type { Category, CategoryUpdateInput } from '@/types'

const props = defineProps<{
  show: boolean
  category: Category | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const reference = useReferenceStore()
const message = useMessage()

const editName = ref('')
const editIcon = ref('')
const editParentId = ref<string>('')

const editParentOptions = computed(() => [
  { label: '无（顶级）', value: '' },
  ...reference.rootCategories
    .filter((c) => c.kind === props.category?.kind)
    .map((c) => ({ label: c.name, value: c.id })),
])

// 打开弹窗时同步待编辑字段（弹窗内容在关闭后仍保留在 DOM，需在打开时回填）
watch(
  () => [props.show, props.category],
  () => {
    if (!props.show || !props.category) return
    editName.value = props.category.name
    editIcon.value = props.category.icon ?? ''
    editParentId.value = props.category.parent_id ?? ''
  },
)

async function saveEdit() {
  const cat = props.category
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
    emit('update:show', false)
    // 参考数据由 ledger:changed 信号自动重拉，分类树随之更新
  } catch (e) {
    message.error(`更新失败: ${errorMessage(e)}`)
  }
}
</script>

<template>
  <NModal
    :show="show"
    title="编辑分类"
    preset="card"
    style="width: 420px"
    :bordered="false"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem label="名称">
        <NInput v-model:value="editName" placeholder="分类名称" />
      </NFormItem>
      <NFormItem label="图标">
        <NInput v-model:value="editIcon" placeholder="图标名" style="width: 120px" />
      </NFormItem>
      <NFormItem label="父分类">
        <PinyinSelect
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
