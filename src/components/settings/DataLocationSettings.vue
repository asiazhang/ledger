<script setup lang="ts">
import { NAlert, NButton, NCard, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { onMounted, ref } from 'vue'
import { open, confirm } from '@tauri-apps/plugin-dialog'
import { api } from '@/api'
import type { DataLocationChangeOutcome, DataLocationInfo } from '@/types'

// 数据存储位置卡片（issue #134 / ADR-0018）：消费 #133 命令层契约。
// 显示值一律来自命令返回（设备本地偏好，前端不做持久化、不走 localStorage）；
// 卡片内不做任何文件系统操作——校验与意图落盘全部由命令层完成，
// 真实搬迁只发生在下次启动（引导内核），故文案必须讲清「下次启动生效」。

const message = useMessage()

const info = ref<DataLocationInfo | null>(null)
const loading = ref(false)
const submitting = ref(false)

async function refresh() {
  loading.value = true
  try {
    info.value = await api.getDataLocationInfo()
  } catch (e: any) {
    message.error(`读取数据存储位置失败: ${e}`)
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
    title: '选择数据存储位置',
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
        '目标目录已存在同名账本库（ledger.db）。\n\n' +
          '「确定」接管该库作为活动数据；「取消」放弃本次更改（不做任何改动）。' +
          '接管后原位置库文件仍会保留。',
        { title: '接管已有账本库？', kind: 'warning' },
      )
      if (!ok) {
        message.info('已取消更改，数据存储位置未变化')
        return
      }
      outcome = await submit(true)
    }
    if (outcome.committed) {
      message.success('更改已保存，将在下次启动时搬迁生效')
    }
    await refresh()
  } catch (e: any) {
    message.error(`更改数据存储位置失败: ${e}`)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <NCard title="数据存储位置" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        账本数据库所在目录。更改位置后，应用将在下次启动时自动把现有数据库完整搬迁到新位置，
        原位置库文件永久保留作为安全网；恢复默认走同一流程。
      </NText>

      <NAlert v-if="info?.fallback_reason" type="error" :show-icon="true" title="已回退到默认位置">
        配置的位置不可用：{{ info.fallback_reason }}。
        已自动回退到默认位置，原库仍在原地未动，可重新更改位置或保持现状。
      </NAlert>

      <NAlert
        v-if="info?.pending_restart && info.configured_dir"
        type="info"
        :show-icon="true"
        title="已更改，待重启生效"
      >
        新位置：{{ info.configured_dir }}。将在下次启动时自动搬迁生效，在此之前数据仍写入当前位置。
      </NAlert>

      <NSpin :show="loading">
        <NSpace align="center" :size="12">
          <NText>当前生效位置：</NText>
          <NText style="word-break: break-all">
            {{ info?.active_dir ?? '读取中…' }}
          </NText>
        </NSpace>
      </NSpin>

      <NSpace align="center" :size="12">
        <NButton type="primary" :loading="submitting" @click="pickAndSubmit">更改…</NButton>
        <NButton :disabled="submitting" @click="restoreDefault">恢复默认</NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
