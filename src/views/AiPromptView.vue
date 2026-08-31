<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { onMounted, ref } from 'vue'
import { NCard, NButton, NSpace, NText, useMessage } from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'

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
        <pre
          class="prompt-body"
          data-testid="prompt-body"
        >{{ prompt || (loading ? t('ai.loading') : t('ai.loadFailed')) }}</pre>
      </NSpace>
    </NCard>
  </NSpace>
</template>

<style scoped>
.prompt-body {
  margin: 0;
  padding: 12px 16px;
  border-radius: 6px;
  background: rgba(128, 128, 128, 0.08);
  font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
  font-size: 13px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 60vh;
  overflow: auto;
}
</style>
