<script setup lang="ts">
import { computed } from 'vue'
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'

const store = useAppStore()
const reference = useReferenceStore()

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
)
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="主题模式" size="small">
      <NSpace align="center" :size="12">
        <NText>深色模式</NText>
        <NSwitch
          :value="store.theme === 'dark'"
          @update:value="(val: boolean) => store.setTheme(val ? 'dark' : 'light')"
        />
      </NSpace>
    </NCard>

    <NCard title="默认币种" size="small">
      <AppSelect
        :value="store.defaultCurrency"
        :options="currencyOptions"
        @update:value="(val: string) => store.setDefaultCurrency(val)"
        style="max-width: 280px"
      />
    </NCard>
  </NSpace>
</template>
