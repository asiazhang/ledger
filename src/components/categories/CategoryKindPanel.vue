<script setup lang="ts">
import { NCard, NSpace } from 'naive-ui'
import CategoryAddForm from './CategoryAddForm.vue'
import CategoryTree from './CategoryTree.vue'
import type { Category, CategoryKind } from '@/types'

defineProps<{ kind: CategoryKind }>()
const emit = defineEmits<{ edit: [cat: Category] }>()
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增分类" size="small">
      <CategoryAddForm :kind="kind" />
    </NCard>

    <NCard title="分类列表" size="small">
      <CategoryTree :kind="kind" @edit="(c: Category) => emit('edit', c)" />
    </NCard>
  </NSpace>
</template>
