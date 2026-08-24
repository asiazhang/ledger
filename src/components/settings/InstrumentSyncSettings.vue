<script setup lang="ts">
import { NButton, NCard, NProgress, NSpace, NText } from 'naive-ui'
import type { SyncResult } from '@/types'

defineProps<{
  status: 'idle' | 'syncing' | 'done'
  progress: number
  result: SyncResult | null
}>()
defineEmits<{ start: [] }>()
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="股票标的全量同步" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          从东方财富 API 一键拉取沪市、深市、港股的股票标的信息和最新价格。
          已存在的标的名称或市场变更时会自动更新，不会删除已有数据。
        </NText>
        <NSpace align="center" :size="12">
          <NButton
            type="primary"
            :disabled="status === 'syncing'"
            :loading="status === 'syncing'"
            @click="$emit('start')"
          >
            {{ status === 'syncing' ? '正在同步...' : '开始同步' }}
          </NButton>
          <NProgress
            v-if="status === 'syncing'"
            style="flex: 1; max-width: 300px"
            :percentage="progress"
            :show-indicator="true"
            :indicator-placement="'inside'"
            status="success"
            :height="28"
          />
        </NSpace>
        <NText v-if="result" type="success">
          同步完成：新增 {{ result.inserted }} 只，更新 {{ result.updated }} 只
        </NText>
      </NSpace>
    </NCard>
  </NSpace>
</template>
