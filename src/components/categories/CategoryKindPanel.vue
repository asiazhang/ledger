<script setup lang="ts">
import { NCard, NSpace } from 'naive-ui'
import CategoryAddForm from './CategoryAddForm.vue'
import CategoryTree from './CategoryTree.vue'
import { t } from '@/i18n'
import type { Category, CategoryKind } from '@/types'

defineProps<{ kind: CategoryKind }>()
const emit = defineEmits<{ edit: [cat: Category] }>()
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.categories.addTitle')" size="small">
      <CategoryAddForm :kind="kind" />
    </NCard>

    <NCard :title="t('settings.categories.listTitle')" size="small">
      <CategoryTree :kind="kind" @edit="(c: Category) => emit('edit', c)" />
    </NCard>
  </NSpace>
</template>
