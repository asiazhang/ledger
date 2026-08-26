<script setup lang="ts">
import { computed } from 'vue'
import { NCard, NDataTable, NSelect, NSpace } from 'naive-ui'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'

const store = useAppStore()
const reference = useReferenceStore()

const currencyColumns = [
  { title: '代码', key: 'code', width: 80 },
  { title: '名称', key: 'name' },
  { title: '符号', key: 'symbol', width: 80 },
  { title: '小数位', key: 'decimal_places', width: 80 },
]

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
)
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="默认币种" size="small">
      <NSelect
        :value="store.defaultCurrency"
        :options="currencyOptions"
        @update:value="(val: string) => store.setDefaultCurrency(val)"
        style="max-width: 280px"
      />
    </NCard>

    <NCard title="支持币种" size="small">
      <NDataTable :columns="currencyColumns" :data="reference.currencies" :bordered="false" size="small" />
    </NCard>
  </NSpace>
</template>
