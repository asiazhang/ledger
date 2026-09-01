<script setup lang="ts">
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import { useAppStore } from '@/stores/app'
import { t } from '@/i18n'

// 定时计划域设置（issue #308 / ADR-0042）：设备级「自动执行」开关。
// 真源在本机 localStorage（应用设置 store），后端运行时镜像由
// useDevicePreferenceSync 在应用启动/变更时推送，本组件只改 store。
const store = useAppStore()
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.scheduled.title')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" :size="12">
          <NSwitch :value="store.autoExecutionEnabled" @update:value="store.setAutoExecutionEnabled" />
          <NText>{{ t('settings.scheduled.switchLabel') }}</NText>
        </NSpace>
        <NText depth="3">
          {{ t('settings.scheduled.body1') }}
        </NText>
        <NText depth="3">
          {{ t('settings.scheduled.body2') }}
        </NText>
      </NSpace>
    </NCard>
  </NSpace>
</template>
