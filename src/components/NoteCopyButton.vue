<script setup lang="ts">
import { NButton, NIcon, useMessage } from 'naive-ui'
import { CopyOutline } from '@vicons/ionicons5'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'

/**
 * 交易备注复制按钮（显式复制通道，见 CONTEXT-ui-interaction「界面文本不可选」）：
 * 全局文本不可选，备注复制经本按钮 + clipboard API 显式完成，复制完整备注而非
 * 截断显示文本；成功/失败 toast 与无障碍标签均经 i18n。悬停显现样式收口
 * global.css（.note-copy-btn：指针轴悬停行/键盘聚焦显现，触控轴常显），组件不持显隐状态。
 */
const props = defineProps<{ note: string }>()
const message = useMessage()

async function copyNote() {
  try {
    await navigator.clipboard.writeText(props.note)
    message.success(t('transactions.list.copyNoteOk'))
  } catch (e) {
    message.error(t('transactions.list.copyNoteFailed', { msg: errorMessage(e) }))
  }
}
</script>

<template>
  <NButton
    size="tiny"
    quaternary
    circle
    class="note-copy-btn"
    :title="t('transactions.list.copyNote')"
    :aria-label="t('transactions.list.copyNote')"
    @click="copyNote"
  >
    <template #icon>
      <NIcon :size="14"><CopyOutline /></NIcon>
    </template>
  </NButton>
</template>
