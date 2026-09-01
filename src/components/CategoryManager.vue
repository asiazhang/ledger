<script setup lang="ts">
import { ref } from 'vue'
import { NTabs, NTabPane } from 'naive-ui'
import CategoryKindPanel from '@/components/categories/CategoryKindPanel.vue'
import CategoryEditModal from '@/components/categories/CategoryEditModal.vue'
import { t } from '@/i18n'
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
    <NTabPane name="expense" :tab="t('settings.categories.kindExpense')">
      <CategoryKindPanel kind="expense" @edit="openEdit" />
    </NTabPane>
    <NTabPane name="income" :tab="t('settings.categories.kindIncome')">
      <CategoryKindPanel kind="income" @edit="openEdit" />
    </NTabPane>
  </NTabs>

  <CategoryEditModal v-model:show="showEditModal" :category="editingCategory" />
</template>
