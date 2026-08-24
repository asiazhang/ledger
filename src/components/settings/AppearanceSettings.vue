<script setup lang="ts">
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import { useAppStore } from '@/stores/app'

const store = useAppStore()
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
  </NSpace>
</template>
