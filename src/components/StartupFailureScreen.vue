<script setup lang="ts">
/**
 * 启动失败恢复屏（issue #601 / ADR-0075 决策 5 修订）：启动失败首屏。
 *
 * 启动期数据库打不开（明文库损坏等）时，本组件**整体替代**主界面（App.vue
 * 以 `v-if` 分支渲染，主界面与全部业务 IPC 消费方都不挂载——后端已登记启动
 * 失败门，占位连接不是业务库，业务读写被 IPC/HTTP 门禁拦截，触达不得）。
 *
 * 首版恢复通道只有一个：「重置为空库」——把打不开的旧库按既有重置命名语义
 * 保留为 `.bak` 副本后原位新建明文空库，成功即进入全新账本（后端原位换连、
 * 拉起调度，无需重启）。重置是破坏性操作，二次确认采用应用内弹窗（ADR-0078
 * error 级形态：红色警示块 + 加粗后果句 + 红色确认按钮；原生 confirm 自此
 * 不容新增）。从备份文件恢复的通道由 issue #602 交付，本屏不出现该入口。
 *
 * 无需注册 Overlay Suppression：本屏挂载期间侧栏/视图/快捷键宿主全部
 * 不存在（与解锁屏同理，见 UnlockScreen 注释）。
 */
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import { ref } from 'vue'
import AppModal from '@/components/AppModal.vue'
import { t } from '@/i18n'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { errorMessage } from '@/utils/errors'

const { resetFromFailure } = useEncryptionGate()
const message = useMessage()

const confirmVisible = ref(false)
const submitting = ref(false)
const errorText = ref('')

/** 二次确认：取消或失败都留在失败恢复屏，可再次进入。 */
function openConfirm() {
  errorText.value = ''
  confirmVisible.value = true
}

function cancelConfirm() {
  if (submitting.value) return
  confirmVisible.value = false
}

async function confirmReset() {
  if (submitting.value) return
  errorText.value = ''
  submitting.value = true
  try {
    await resetFromFailure()
    confirmVisible.value = false
    message.success(t('startupFailure.resetOk'))
  } catch (e) {
    errorText.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="failure-screen">
    <NCard class="failure-card" :bordered="false">
      <NSpace vertical :size="16" align="center" :style="{ width: '100%' }">
        <NText class="failure-title">{{ t('startupFailure.title') }}</NText>
        <NText depth="3" class="failure-hint">{{ t('startupFailure.hint') }}</NText>

        <!-- 首版恢复通道（issue #601）：重置为空库；备份恢复通道由 #602 交付 -->
        <NSpace vertical :size="8" class="failure-channel">
          <NText strong>{{ t('startupFailure.resetChannelTitle') }}</NText>
          <NText depth="3">{{ t('startupFailure.resetChannelHint') }}</NText>
          <NButton type="error" block data-testid="failure-reset-open" @click="openConfirm">
            {{ t('startupFailure.resetButton') }}
          </NButton>
        </NSpace>

        <NText v-if="errorText" type="error" class="failure-error">{{ errorText }}</NText>
      </NSpace>
    </NCard>

    <!-- 二次确认（ADR-0078 error 级）：红色警示块 + 加粗后果句 + 红色确认按钮 -->
    <AppModal
      :show="confirmVisible"
      preset="card"
      :title="t('startupFailure.resetTitle')"
      style="width: 480px"
      :bordered="false"
      :mask-closable="false"
      @update:show="cancelConfirm"
    >
      <NSpace vertical :size="12">
        <NAlert type="error" :show-icon="true" :title="t('startupFailure.resetWarnTitle')">
          <NText strong type="error">{{ t('startupFailure.resetConsequence') }}</NText>
        </NAlert>
        <NText depth="3">{{ t('startupFailure.resetBody') }}</NText>
        <NSpace justify="end">
          <NButton :disabled="submitting" data-testid="failure-reset-cancel" @click="cancelConfirm">
            {{ t('startupFailure.resetCancel') }}
          </NButton>
          <NButton
            type="error"
            :loading="submitting"
            :disabled="submitting"
            data-testid="failure-reset-confirm"
            @click="confirmReset"
          >
            {{ t('startupFailure.resetConfirm') }}
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>
  </div>
</template>

<style scoped>
.failure-screen {
  position: fixed;
  inset: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.failure-card {
  width: 420px;
}

.failure-title {
  font-size: 18px;
  font-weight: 600;
}

.failure-hint,
.failure-error {
  align-self: flex-start;
}

.failure-channel {
  width: 100%;
  align-self: stretch;
}
</style>
