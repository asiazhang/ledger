<script setup lang="ts">
import {
  NButton,
  NCard,
  NDataTable,
  NInputNumber,
  NSpace,
  NText,
} from 'naive-ui'
import { useAppStore } from '@/stores/app'
import { useBackup } from '@/composables/useBackup'

const store = useAppStore()

const {
  backingUp,
  restoring,
  lastBackup,
  backups,
  pruning,
  backupRows,
  pickBackupDir,
  clearBackupDir,
  onBackupMaxCountChange,
  manualPrune,
  backupOnce,
  backupAs,
  pickRestore,
} = useBackup()

const backupColumns = [
  { title: '文件名', key: 'file_name' },
  { title: '大小', key: 'size_text', width: 100 },
  { title: '备份时间', key: 'created_at', width: 160 },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="备份目录" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          配置默认备份目录后，“一键备份”将直接写入该目录，无需每次选择位置。
        </NText>
        <NSpace align="center" :size="12">
          <NText style="word-break: break-all">
            {{ store.backupDir || '未设置（备份时将弹出位置选择）' }}
          </NText>
          <NButton size="small" @click="pickBackupDir">
            {{ store.backupDir ? '更改目录' : '选择目录' }}
          </NButton>
          <NButton v-if="store.backupDir" size="small" quaternary type="error" @click="clearBackupDir">
            清除
          </NButton>
        </NSpace>
        <NSpace align="center" :size="12">
          <NText>备份保留上限</NText>
          <NInputNumber
            :value="store.backupMaxCount"
            :min="1"
            :max="100"
            :update-value-on-input="false"
            style="max-width: 120px"
            @update:value="onBackupMaxCountChange"
          />
          <NText depth="3">个（1–100）。超出上限的最旧备份会在备份后自动清理。</NText>
        </NSpace>
      </NSpace>
    </NCard>

    <NCard title="备份" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          将当前账本（账户、交易、预算、投资持仓、定时交易等全部数据）打包备份为一个文件。
          备份不含主题、默认币种等界面偏好。
        </NText>
        <NSpace align="center" :size="12">
          <NButton type="primary" :loading="backingUp" @click="backupOnce">一键备份</NButton>
          <NButton :loading="backingUp" @click="backupAs">另存为…</NButton>
        </NSpace>
        <NText v-if="lastBackup" type="success" style="word-break: break-all">
          最近备份：{{ lastBackup }}
        </NText>
      </NSpace>
    </NCard>

    <NCard title="备份文件列表" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" justify="space-between" style="width: 100%">
          <NText depth="3">当前共 {{ backups.length }} 个备份，上限 {{ store.backupMaxCount }} 个。</NText>
          <NButton
            size="small"
            :disabled="backups.length === 0 || pruning"
            :loading="pruning"
            @click="manualPrune"
          >
            立即清理
          </NButton>
        </NSpace>
        <NDataTable
          :columns="backupColumns"
          :data="backupRows"
          :bordered="false"
          size="small"
          :empty="store.backupDir ? '备份目录中暂无备份文件' : '未设置备份目录'"
        />
      </NSpace>
    </NCard>

    <NCard title="恢复" size="small">
      <NSpace vertical :size="12">
        <NText type="warning" depth="3">
          从备份文件恢复将<strong>替换当前全部数据</strong>（破坏性操作）。
          恢复前系统会自动备份当前数据到应用数据目录；恢复成功后应用将自动重启。
        </NText>
        <NButton type="error" :loading="restoring" @click="pickRestore">从备份恢复…</NButton>
      </NSpace>
    </NCard>
  </NSpace>
</template>
