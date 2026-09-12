<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { NCard, NSpace, NText, useMessage } from 'naive-ui'
import pkg from '@/../package.json'
import { gitShaFull, gitVersionLabel } from '@/utils/git-info'
import { t } from '@ledger/i18n'

const message = useMessage()
const gitVersion = gitVersionLabel()

// 「关于」Tab 是纯元信息展示（issue #930 / ADR-0022 修订）：仅承载应用名称、版本、
// Git 版本与构建平台；可调设置与日志运维入口已迁「通用」Tab 日志卡片（LogSettings）。
async function copyGitSha() {
  try {
    await navigator.clipboard.writeText(gitShaFull())
    message.success(t('settings.about.copyShaOk'))
  } catch (e: any) {
    message.error(t('settings.about.copyShaFailed', { msg: errorMessage(e) }))
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
    </NSpace>
  </NCard>
</template>
