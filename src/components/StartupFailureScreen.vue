<script setup lang="ts">
/**
 * 启动失败恢复屏（issue #601 / ADR-0075 决策 5 修订）：启动失败首屏。
 *
 * 启动期数据库打不开（明文库损坏等）时，本组件**整体替代**主界面（App.vue
 * 以 `v-if` 分支渲染，主界面与全部业务 IPC 消费方都不挂载——后端已登记启动
 * 失败门，占位连接不是业务库，业务读写被 IPC/HTTP 门禁拦截，触达不得）。
 *
 * 恢复通道有两个（issue #601 / #602）：
 * - 「重置为空库」——把打不开的旧库按既有重置命名语义保留为 `.bak` 副本后
 *   原位新建明文空库，成功即进入全新账本（后端原位换连、拉起调度，无需
 *   重启）。重置是破坏性操作，二次确认采用应用内弹窗（ADR-0078 error 级
 *   形态：红色警示块 + 加粗后果句 + 红色确认按钮；原生 confirm 自此不容
 *   新增）。
 * - 「从备份文件恢复…」（issue #602）——文件选择器选定备份 → 元数据校验 →
 *   恢复确认弹窗（跨模式警告与密文备份口令复用 #572 的恢复确认弹窗）→
 *   既有 Restore 全语义（安全备份字节副本 + 原子替换）→ 成功后自动重启
 *   进入恢复后的数据。编排收口在 useFailureRestore。
 *
 * 无需注册 Overlay Suppression：本屏挂载期间侧栏/视图/快捷键宿主全部
 * 不存在（与解锁屏同理，见 UnlockScreen 注释）。
 */
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import { ref } from 'vue'
import AppModal from '@/components/AppModal.vue'
import RestoreConfirmModal from '@/components/RestoreConfirmModal.vue'
import { t } from '@/i18n'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { useFailureRestore } from '@/composables/useFailureRestore'
import { errorMessage } from '@/utils/errors'

const { resetFromFailure } = useEncryptionGate()
const {
  restoreIntent,
  restoreSeq,
  closeRestore,
  confirmRestore,
  pickRestoreFromFailure,
} = useFailureRestore()
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

        <!-- 恢复通道①（issue #601）：重置为空库 -->
        <NSpace vertical :size="8" class="failure-channel">
          <NText strong>{{ t('startupFailure.resetChannelTitle') }}</NText>
          <NText depth="3">{{ t('startupFailure.resetChannelHint') }}</NText>
          <NButton type="error" block data-testid="failure-reset-open" @click="openConfirm">
            {{ t('startupFailure.resetButton') }}
          </NButton>
        </NSpace>

        <!-- 恢复通道②（issue #602）：从备份文件恢复；确认弹窗复用 #572 语义面 -->
        <NSpace vertical :size="8" class="failure-channel">
          <NText strong>{{ t('startupFailure.restoreChannelTitle') }}</NText>
          <NText depth="3">{{ t('startupFailure.restoreChannelHint') }}</NText>
          <NButton block data-testid="failure-restore-open" @click="pickRestoreFromFailure">
            {{ t('startupFailure.restoreButton') }}
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

    <!-- 备份恢复确认弹窗（issue #602）：跨模式警告 + 密文备份口令（#572 语义面） -->
    <RestoreConfirmModal
      :intent="restoreIntent"
      :seq="restoreSeq"
      :on-confirm="confirmRestore"
      @close="closeRestore"
    />
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
