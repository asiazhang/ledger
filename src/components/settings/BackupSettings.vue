<script setup lang="ts">
import {
  NButton,
  NCard,
  NDataTable,
  NInputNumber,
  NSpace,
  NSwitch,
  NText,
} from 'naive-ui'
import { useAppStore } from '@/stores/app'
import { useBackup } from '@/composables/useBackup'
import { t } from '@/i18n'

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
  autoBackupEnabled,
  autoBackupLastText,
  toggleAutoBackup,
} = useBackup()

const backupColumns = [
  { title: () => t('settings.data.backup.columns.fileName'), key: 'file_name' },
  { title: () => t('settings.data.backup.columns.source'), key: 'source_text', width: 70 },
  { title: () => t('settings.data.backup.columns.size'), key: 'size_text', width: 100 },
  { title: () => t('settings.data.backup.columns.time'), key: 'created_at', width: 160 },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.data.backup.dirTitle')" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          {{ t('settings.data.backup.dirHint') }}
        </NText>
        <NSpace align="center" :size="12">
          <NText style="word-break: break-all">
            {{ store.backupDir || t('settings.data.backup.dirUnset') }}
          </NText>
          <NButton size="small" @click="pickBackupDir">
            {{ store.backupDir ? t('settings.data.backup.changeDir') : t('settings.data.backup.chooseDir') }}
          </NButton>
          <NButton v-if="store.backupDir" size="small" quaternary type="error" @click="clearBackupDir">
            {{ t('settings.data.backup.clear') }}
          </NButton>
        </NSpace>
        <NSpace align="center" :size="12">
          <NText>{{ t('settings.data.backup.keepLimitLabel') }}</NText>
          <NInputNumber
            :value="store.backupMaxCount"
            :min="1"
            :max="100"
            :update-value-on-input="false"
            style="max-width: 120px"
            @update:value="onBackupMaxCountChange"
          />
          <NText depth="3">{{ t('settings.data.backup.keepLimitSuffix') }}</NText>
        </NSpace>
      </NSpace>
    </NCard>

    <NCard :title="t('settings.data.backup.autoTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" :size="12">
          <NSwitch
            :value="autoBackupEnabled"
            @update:value="toggleAutoBackup"
          />
          <NText>{{ t('settings.data.backup.autoSwitchLabel') }}</NText>
        </NSpace>
        <NText depth="3">{{ t('settings.data.backup.autoLast') }}{{ autoBackupLastText }}</NText>
        <NText v-if="!store.backupDir" type="warning">
          {{ t('settings.data.backup.autoNeedDir') }}
        </NText>
      </NSpace>
    </NCard>

    <NCard :title="t('settings.data.backup.backupTitle')" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          {{ t('settings.data.backup.backupHint') }}
        </NText>
        <NSpace align="center" :size="12">
          <NButton type="primary" :loading="backingUp" @click="backupOnce">{{ t('settings.data.backup.backupOnce') }}</NButton>
          <NButton :loading="backingUp" @click="backupAs">{{ t('settings.data.backup.backupAs') }}</NButton>
        </NSpace>
        <NText v-if="lastBackup" type="success" style="word-break: break-all">
          {{ t('settings.data.backup.lastBackup') }}{{ lastBackup }}
        </NText>
      </NSpace>
    </NCard>

    <NCard :title="t('settings.data.backup.listTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" justify="space-between" style="width: 100%">
          <NText depth="3">{{ t('settings.data.backup.count', { n: backups.length, max: store.backupMaxCount }) }}</NText>
          <NButton
            size="small"
            :disabled="backups.length === 0 || pruning"
            :loading="pruning"
            @click="manualPrune"
          >
            {{ t('settings.data.backup.pruneNow') }}
          </NButton>
        </NSpace>
        <NDataTable
          :columns="backupColumns"
          :data="backupRows"
          :bordered="false"
          size="small"
          :empty="store.backupDir ? t('settings.data.backup.emptyWithDir') : t('settings.data.backup.emptyNoDir')"
        />
      </NSpace>
    </NCard>

    <NCard :title="t('settings.data.backup.restoreTitle')" size="small">
      <NSpace vertical :size="12">
        <NText type="warning" depth="3">
          {{ t('settings.data.backup.restoreHintBefore') }}<strong>{{ t('settings.data.backup.restoreHintStrong') }}</strong>{{ t('settings.data.backup.restoreHintAfter') }}
        </NText>
        <NButton type="error" :loading="restoring" @click="pickRestore">{{ t('settings.data.backup.restoreButton') }}</NButton>
      </NSpace>
    </NCard>
  </NSpace>
</template>
