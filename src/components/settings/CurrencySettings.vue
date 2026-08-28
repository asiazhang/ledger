<script setup lang="ts">
import { NCard, NDataTable, NSpace } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'

const reference = useReferenceStore()

const currencyColumns = [
  { title: '代码', key: 'code', width: 80 },
  { title: '名称', key: 'name' },
  { title: '符号', key: 'symbol', width: 80 },
  { title: '小数位', key: 'decimal_places', width: 80 },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="支持币种" size="small">
      <NDataTable :columns="currencyColumns" :data="reference.currencies" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
