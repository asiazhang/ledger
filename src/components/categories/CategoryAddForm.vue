<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSelect, useMessage } from 'naive-ui'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { CategoryInput, CategoryKind } from '@/types'

const props = defineProps<{ kind: CategoryKind }>()

const store = useAppStore()
const message = useMessage()

const name = ref('')
const parentId = ref<string>('')
const icon = ref('')

const parentOptions = computed(() => [
  { label: '无（顶级）', value: '' },
  ...store.rootCategories
    .filter((c) => c.kind === props.kind)
    .map((c) => ({ label: c.name, value: c.id })),
])

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
    if (parent.kind !== props.kind) {
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
    kind: props.kind,
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
</script>

<template>
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
</template>
