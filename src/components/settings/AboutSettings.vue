<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import pkg from '@/../package.json'
import { gitShaFull, gitVersionLabel } from '@/utils/git-info'
import { t } from '@/i18n'

const message = useMessage()
const gitVersion = gitVersionLabel()

// 日志等级（spec #611）：后端消费、存 `app_settings`、随备份迁移，故走 IPC 命令而非
// 前端 localStorage store。闭集五档、默认 info；改动立即生效、跨启动保留；
// 显式 RUST_LOG 环境变量在本次启动内优先且不写库——界面展示的是持久化档位。
const currentLogLevel = ref<string>('info')
const savingLogLevel = ref(false)
const logLevelOptions = computed(() =>
  ['error', 'warn', 'info', 'debug', 'trace'].map((v) => ({
    label: t(`settings.about.logLevel.${v}`),
    value: v,
  })),
)

async function loadLogLevel() {
  try {
    const s = await api.getLogLevel()
    currentLogLevel.value = s.level
  } catch (e) {
    message.error(t('settings.about.logLevel.loadFailed', { msg: errorMessage(e) }))
  }
}

async function handleLevelChange(level: string) {
  savingLogLevel.value = true
  try {
    await api.setLogLevel(level)
    currentLogLevel.value = level
  } catch (e) {
    // 失败不回写 currentLogLevel，下拉回显保持原档位；错误透传后端可读信息
    message.error(t('settings.about.logLevel.saveFailed', { msg: errorMessage(e) }))
  } finally {
    savingLogLevel.value = false
  }
}

onMounted(loadLogLevel)

async function copyGitSha() {
  try {
    await navigator.clipboard.writeText(gitShaFull())
    message.success(t('settings.about.copyShaOk'))
  } catch (e: any) {
    message.error(t('settings.about.copyShaFailed', { msg: errorMessage(e) }))
  }
}

async function openLogDir() {
  try {
    await api.openLogDir()
  } catch (e) {
    // 后端错误已带「打开日志目录失败：」中文前缀，此处原样透传不叠加
    message.error(errorMessage(e))
  }
}
</script>

<template>
  <NCard :title="t('settings.about.title')" size="small">
    <NSpace vertical :size="8">
      <NText>{{ t('settings.about.appName') }}{{ t('common.app.name') }}</NText>
      <NText>{{ t('settings.about.version') }}{{ pkg.version }}</NText>
      <NText
        v-if="gitVersion"
        data-testid="git-version"
        :title="t('settings.about.gitVersionTitle')"
        style="cursor: pointer"
        @click="copyGitSha"
      >
        {{ t('settings.about.gitVersion') }}{{ gitVersion }}
      </NText>
      <NText>{{ t('settings.about.platform') }}Tauri + Vue 3 + TypeScript</NText>
      <NSpace vertical :size="4" style="max-width: 280px">
        <NText>{{ t('settings.about.logLevel.label') }}</NText>
        <AppSelect
          :value="currentLogLevel"
          :options="logLevelOptions"
          :disabled="savingLogLevel"
          @update:value="handleLevelChange"
          :data-testid="'log-level-select'"
        />
        <NText depth="3">{{ t('settings.about.logLevel.hint') }}</NText>
      </NSpace>
      <NButton size="small" @click="openLogDir">{{ t('settings.about.openLogDir') }}</NButton>
    </NSpace>
  </NCard>
</template>
