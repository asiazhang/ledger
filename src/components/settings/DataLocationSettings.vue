<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { NAlert, NButton, NCard, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { onMounted, ref } from 'vue'
import { open, confirm } from '@tauri-apps/plugin-dialog'
import { api } from '@/api'
import { t } from '@/i18n'
import type { DataLocationChangeOutcome, DataLocationInfo } from '@/types'

// 数据存储位置卡片（issue #134 / ADR-0018）：消费 #133 命令层契约。
// 显示值一律来自命令返回（设备本地偏好，前端不做持久化、不走 localStorage）；
// 卡片内不做任何文件系统操作——校验与意图落盘全部由命令层完成，
// 真实搬迁只发生在下次启动（引导内核），故文案必须讲清「下次启动生效」。

const message = useMessage()

const info = ref<DataLocationInfo | null>(null)
const loading = ref(false)
const loadError = ref('')
const submitting = ref(false)

async function refresh() {
  loading.value = true
  loadError.value = ''
  try {
    info.value = await api.getDataLocationInfo()
  } catch (e: any) {
    // 读取失败诚实呈现，不用「读取中…」假装一切正常。
    loadError.value = t('settings.data.msg.loadFailed', { msg: errorMessage(e) })
    message.error(loadError.value)
  } finally {
    loading.value = false
  }
}

onMounted(refresh)

/** 唤起系统目录选择对话框，选中后提交更改意图。取消选择则不动状态。 */
async function pickAndSubmit() {
  const dir = await open({
    directory: true,
    multiple: false,
    title: t('settings.data.location.pickTitle'),
  })
  if (typeof dir !== 'string' || !dir) return
  await submitWithChoice((adoptExisting) => api.submitDataLocationChange(dir, adoptExisting))
}

/** 恢复默认位置：与更改完全同一确认与反馈形态，目标由命令层决定。 */
async function restoreDefault() {
  await submitWithChoice((adoptExisting) => api.restoreDefaultDataLocation(adoptExisting))
}

/**
 * 提交更改意图并按命令结果分支呈现：
 * 校验通过 → 成功提示并刷新；目标已有同名库 → 二选一确认，
 * 接管则以 adopt_existing = true 二次提交，取消则状态保持不变；
 * 校验失败 → 错误反馈，状态不变。
 */
async function submitWithChoice(
  submit: (adoptExisting: boolean) => Promise<DataLocationChangeOutcome>,
) {
  submitting.value = true
  try {
    let outcome = await submit(false)
    if (outcome.requires_choice) {
      const ok = await confirm(
        t('settings.data.location.adoptBody'),
        {
          title: t('settings.data.location.adoptTitle'),
          kind: 'warning',
          // 二选一按钮文案显式化（spec：接管该库 / 取消换位），不靠「确定/取消」猜语义。
          okLabel: t('settings.data.location.adoptOk'),
          cancelLabel: t('settings.data.location.adoptCancel'),
        },
      )
      if (!ok) {
        message.info(t('settings.data.location.cancelled'))
        return
      }
      outcome = await submit(true)
    }
    if (outcome.committed) {
      message.success(t('settings.data.location.committed'))
    }
    await refresh()
  } catch (e: any) {
    message.error(t('settings.data.msg.changeFailed', { msg: errorMessage(e) }))
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <NCard :title="t('settings.data.location.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('settings.data.location.hint') }}
      </NText>

      <NAlert v-if="info?.fallback_reason" type="error" :show-icon="true" :title="t('settings.data.location.fallbackTitle')">
        {{ t('settings.data.location.fallbackBody', { reason: info.fallback_reason }) }}
      </NAlert>

      <NAlert
        v-if="info?.pending_restart && info.configured_dir"
        type="info"
        :show-icon="true"
        :title="t('settings.data.location.pendingTitle')"
      >
        {{ t('settings.data.location.pendingBody', { dir: info.configured_dir }) }}
      </NAlert>

      <NSpin :show="loading">
        <NSpace v-if="loadError" align="center" :size="12">
          <NText type="error">{{ loadError }}</NText>
          <NButton size="small" @click="refresh">{{ t('settings.data.location.retry') }}</NButton>
        </NSpace>
        <NSpace v-else align="center" :size="12">
          <NText>{{ t('settings.data.location.activeLabel') }}</NText>
          <NText style="word-break: break-all">
            {{ info?.active_dir ?? t('settings.data.location.reading') }}
          </NText>
        </NSpace>
      </NSpin>

      <NSpace align="center" :size="12">
        <NButton type="primary" :loading="submitting" @click="pickAndSubmit">{{ t('settings.data.location.change') }}</NButton>
        <!-- 未配置任何意图目录即处于默认位置，「恢复默认」无意义故禁用
             （契约不变量：configured_dir 非空 ⇔ 存在自定义位置意图）。 -->
        <NButton :disabled="submitting || !info?.configured_dir" @click="restoreDefault">
          {{ t('settings.data.location.restoreDefault') }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
