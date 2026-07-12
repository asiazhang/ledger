<script setup lang="ts">
import { onMounted } from 'vue'
import { NCard, NDataTable, NSpace } from 'naive-ui'
import { useAppStore } from '@/stores/app'
import CategoryManager from '@/components/CategoryManager.vue'

const store = useAppStore()

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
    <CategoryManager />

    <NCard title="币种" size="small">
      <NDataTable :columns="currencyColumns" :data="store.currencies" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
