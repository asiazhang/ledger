<script setup lang="ts">
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import { useAppStore } from '@/stores/app'

// 定时计划域设置（issue #308 / ADR-0042）：设备级「自动执行」开关。
// 真源在本机 localStorage（应用设置 store），后端运行时镜像由
// useDevicePreferenceSync 在应用启动/变更时推送，本组件只改 store。
const store = useAppStore()
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="自动执行" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" :size="12">
          <NSwitch :value="store.autoExecutionEnabled" @update:value="store.setAutoExecutionEnabled" />
          <NText>开启后自动落账到期期次</NText>
        </NSpace>
        <NText depth="3">
          开启后，应用运行期间每 10 分钟检查一次，把已到期的待执行期次自动落账，交易日期忠实取期次计划日期；失败期次保持手动重试，暂停或取消的计划不会被碰。自动执行只应在一台机器开启。
        </NText>
        <NText depth="3">
          开关属本机设备偏好：不随备份或恢复迁移，换新机器或恢复备份后保持默认关闭。
        </NText>
      </NSpace>
    </NCard>
  </NSpace>
</template>
