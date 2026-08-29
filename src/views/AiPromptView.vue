<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { onMounted, ref } from 'vue'
import { NCard, NButton, NSpace, NText, useMessage } from 'naive-ui'
import { api } from '@/api'

const message = useMessage()
const prompt = ref('')
const loading = ref(true)

onMounted(async () => {
  try {
    prompt.value = await api.getAiPrompt()
  } catch (e) {
    message.error(`获取提示词失败: ${errorMessage(e)}`)
  } finally {
    loading.value = false
  }
})

async function copyPrompt() {
  try {
    await navigator.clipboard.writeText(prompt.value)
    message.success('已复制到剪贴板')
  } catch (e) {
    message.error(`复制失败: ${errorMessage(e)}`)
  }
}
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="AI 系统提示词" :bordered="false" style="max-width: 900px">
      <template #header-extra>
        <NButton size="small" type="primary" :disabled="!prompt" @click="copyPrompt">
          复制
        </NButton>
      </template>
      <NSpace vertical :size="8">
        <NText depth="3" style="font-size: 13px">
          将以下提示词复制给 AI 编程助手（如 Cursor、Claude Code），
          它会据此通过本地 HTTP API（127.0.0.1:9527）读取与写入账本数据。
        </NText>
        <pre
          class="prompt-body"
          data-testid="prompt-body"
        >{{ prompt || (loading ? '加载中…' : '获取失败') }}</pre>
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
