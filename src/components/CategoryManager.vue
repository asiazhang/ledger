<script setup lang="ts">
import { ref } from 'vue'
import { NTabs, NTabPane } from 'naive-ui'
import CategoryKindPanel from '@/components/categories/CategoryKindPanel.vue'
import CategoryEditModal from '@/components/categories/CategoryEditModal.vue'
import type { Category, CategoryKind } from '@/types'

const activeKind = ref<CategoryKind>('expense')
const showEditModal = ref(false)
const editingCategory = ref<Category | null>(null)

function openEdit(cat: Category) {
  editingCategory.value = cat
  showEditModal.value = true
}
</script>

<template>
  <NTabs type="line" :value="activeKind" @update:value="(v) => activeKind = v as CategoryKind">
    <NTabPane name="expense" tab="支出">
      <CategoryKindPanel kind="expense" @edit="openEdit" />
    </NTabPane>
    <NTabPane name="income" tab="收入">
      <CategoryKindPanel kind="income" @edit="openEdit" />
    </NTabPane>
  </NTabs>

  <CategoryEditModal v-model:show="showEditModal" :category="editingCategory" />
</template>
