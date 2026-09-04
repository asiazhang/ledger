<script setup lang="ts">
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import pkg from '@/../package.json'
import { gitShaFull, gitVersionLabel } from '@/utils/git-info'
import { t } from '@/i18n'

const message = useMessage()
const gitVersion = gitVersionLabel()

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
      <NButton size="small" @click="openLogDir">{{ t('settings.about.openLogDir') }}</NButton>
    </NSpace>
  </NCard>
</template>
