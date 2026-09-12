<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { t } from '@ledger/i18n'

const message = useMessage()

// 日志卡片（issue #930）：日志运维无自有业务域页签，按 ADR-0022 修订（「通用」＝
// 不归属业务域页签的应用级偏好）落「通用」Tab 末位。日志等级（spec #611）后端消费、
// 存 `app_settings`、随备份迁移，故走 IPC 命令而非前端 localStorage store。闭集五档、
// 默认 info；改动立即生效、跨启动保留；显式 RUST_LOG 环境变量在本次启动内优先且不写库
// ——界面展示的是持久化档位。「打开日志目录」是动作而非设置，作为卡片内例外收纳，
// 与等级下拉同卡保住排查流程（调档 → 复现 → 取文件）不跨页签。
const currentLogLevel = ref<string>('info')
const savingLogLevel = ref(false)
const logLevelOptions = computed(() =>
  ['error', 'warn', 'info', 'debug', 'trace'].map((v) => ({
    label: t(`settings.logs.${v}`),
    value: v,
  })),
)

async function loadLogLevel() {
  try {
    const s = await api.getLogLevel()
    currentLogLevel.value = s.level
  } catch (e) {
    message.error(t('settings.logs.loadFailed', { msg: errorMessage(e) }))
  }
}

async function handleLevelChange(level: string) {
  savingLogLevel.value = true
  try {
    await api.setLogLevel(level)
    currentLogLevel.value = level
  } catch (e) {
    // 失败不回写 currentLogLevel，下拉回显保持原档位；错误透传后端可读信息
    message.error(t('settings.logs.saveFailed', { msg: errorMessage(e) }))
  } finally {
    savingLogLevel.value = false
  }
}

onMounted(loadLogLevel)

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
  <NCard :title="t('settings.logs.card')" size="small">
    <NSpace vertical :size="8">
      <NText>{{ t('settings.logs.label') }}</NText>
      <AppSelect
        :value="currentLogLevel"
        :options="logLevelOptions"
        :disabled="savingLogLevel"
        @update:value="handleLevelChange"
        :data-testid="'log-level-select'"
      />
      <NText depth="3">{{ t('settings.logs.hint') }}</NText>
      <NButton size="small" @click="openLogDir">{{ t('settings.logs.openLogDir') }}</NButton>
    </NSpace>
  </NCard>
</template>
