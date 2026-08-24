<script setup lang="ts">
import { NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import { invoke } from '@tauri-apps/api/core'
import pkg from '@/../package.json'

const message = useMessage()

async function openLogDir() {
  try {
    await invoke('plugin:log|open_log_dir')
  } catch (e: any) {
    message.error(`打开日志目录失败: ${e}`)
  }
}
</script>

<template>
  <NCard title="关于 Ledger" size="small">
    <NSpace vertical :size="8">
      <NText>应用名称：Ledger</NText>
      <NText>版本号：{{ pkg.version }}</NText>
      <NText>构建平台：Tauri + Vue 3 + TypeScript</NText>
      <NButton size="small" @click="openLogDir">打开日志目录</NButton>
    </NSpace>
  </NCard>
</template>
