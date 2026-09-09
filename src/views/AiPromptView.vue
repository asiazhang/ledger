<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { onMounted, ref } from 'vue'
import { NCard, NButton, NSpace, NText, useMessage } from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
// 样式方案试点（issue #888 / ADR-0093）：样式住旁路样式文件，随根元素主题类亮暗换装
import { promptBody } from './AiPromptView.css.ts'

const message = useMessage()
const prompt = ref('')
const loading = ref(true)

onMounted(async () => {
  try {
    prompt.value = await api.getAiPrompt()
  } catch (e) {
    message.error(t('ai.msg.loadFailed', { msg: errorMessage(e) }))
  } finally {
    loading.value = false
  }
})

async function copyPrompt() {
  try {
    await navigator.clipboard.writeText(prompt.value)
    message.success(t('ai.msg.copied'))
  } catch (e) {
    message.error(t('ai.msg.copyFailed', { msg: errorMessage(e) }))
  }
}
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('ai.title')" :bordered="false">
      <template #header-extra>
        <NButton size="small" type="primary" :disabled="!prompt" @click="copyPrompt">
          {{ t('ai.copy') }}
        </NButton>
      </template>
      <NSpace vertical :size="8">
        <NText depth="3" style="font-size: 13px">
          {{ t('ai.description') }}
        </NText>
        <!-- class 来自旁路样式文件（issue #888 试点）：原 scoped 样式块已迁出删除 -->
        <pre
          :class="promptBody"
          data-testid="prompt-body"
        >{{ prompt || (loading ? t('ai.loading') : t('ai.loadFailed')) }}</pre>
      </NSpace>
    </NCard>
  </NSpace>
</template>
