<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NAlert, NButton, NFormItem, NInput, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import { t } from '@/i18n'
import { errorMessage, errorCodeOf } from '@/utils/errors'
import {
  restoreCrossModeWarningKey,
  type RestoreIntent,
} from '@/composables/useBackup'

// 恢复确认弹窗（issue #572 / ADR-0075 决策 7）：从原生 confirm 对话框升级为
// 应用内弹窗，承载两件加密语义面——
// - 跨模式警告：当前库模式与备份模式不一致时显著警告（恢复后加密模式随文件走，
//   重启由启动探测接管实际模式）；
// - 密文备份主口令：备份为密文时输入备份所在库的主口令以校验并恢复；口令错误
//   不关弹窗、可就地重试（解锁同语义，ADR-0075 决策 5）。
// 弹层纪律：AppModal 收口关闭语义（遮罩不关）并接入弹层注册表（快捷键抑制）。
const props = defineProps<{
  /** 弹窗意图（null = 关闭终态，ADR-0072：非空即显示）。 */
  intent: RestoreIntent | null
  /** 意图序号（重开触发表单重置的凭据）。 */
  seq: number
  /** 确认回调（useBackup.confirmRestore）：成功 resolve（父层关意图 + 重启），失败 reject（弹窗内展示、可重试）。 */
  onConfirm: (passphrase: string) => Promise<void>
}>()

const emit = defineEmits<{ close: [] }>()

const passphrase = ref('')
const submitting = ref(false)
/** 弹窗内错误提示（口令错误等）：保持弹窗打开供修改重试。 */
const error = ref<string | null>(null)
/** 后端探测实际密文而元数据未标记（异常产物）时，按需显出口令输入——以后端探测为准。 */
const passphraseRevealedByError = ref(false)

// 每次意图落位（新对象）重置输入与错误，迟到的旧错误不残留。
watch(
  () => props.seq,
  () => {
    passphrase.value = ''
    error.value = null
    passphraseRevealedByError.value = false
  },
)

/** 跨模式警告文案：同模式为空（不渲染警告位）。 */
const warningText = computed(() => {
  if (!props.intent) return ''
  const key = restoreCrossModeWarningKey(props.intent.backupEncrypted, props.intent.currentEncrypted)
  return key ? t(key) : ''
})

/** 密文备份需主口令；口令未输时确认禁用。后端探测报需口令错误时按需显出。 */
const needsPassphrase = computed(() => props.intent?.backupEncrypted ?? false)
const showPassphrase = computed(() => needsPassphrase.value || passphraseRevealedByError.value)
const canSubmit = computed(() => !submitting.value && (!showPassphrase.value || passphrase.value.length > 0))

async function confirm() {
  if (!canSubmit.value || !props.intent) return
  submitting.value = true
  error.value = null
  try {
    await props.onConfirm(passphrase.value)
  } catch (e) {
    error.value = errorMessage(e)
    // 元数据谎报明文而实库为密文（异常产物）：后端拒绝并要求口令，
    // 显出口令输入让用户就地补救，而不是卡死在无输入框的错误提示上。
    if (errorCodeOf(e) === 'backup.passphrase-required') {
      passphraseRevealedByError.value = true
    }
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <AppModal
    :show="intent !== null"
    preset="card"
    :title="t('settings.data.msg.restoreConfirmTitle')"
    style="width: 480px"
    :bordered="false"
    @update:show="(v: boolean) => { if (!v) emit('close') }"
  >
    <NSpace vertical :size="12">
      <NText depth="3" style="white-space: pre-line">
        {{ t('settings.data.msg.restoreConfirm') }}
      </NText>

      <NAlert
        v-if="warningText"
        type="warning"
        :show-icon="true"
        :title="t('settings.data.msg.restoreCrossModeWarnTitle')"
      >
        {{ warningText }}
      </NAlert>

      <NFormItem v-if="showPassphrase" :label="t('settings.data.msg.restorePassphraseLabel')">
        <NInput
          v-model:value="passphrase"
          type="password"
          show-password-on="click"
          :placeholder="t('settings.data.msg.restorePassphraseHint')"
          :disabled="submitting"
          data-testid="restore-passphrase"
          @keyup.enter="confirm"
        />
      </NFormItem>

      <NText v-if="error" type="error">{{ error }}</NText>

      <NSpace justify="end">
        <NButton :disabled="submitting" data-testid="restore-cancel" @click="emit('close')">
          {{ t('settings.data.msg.restoreCancel') }}
        </NButton>
        <NButton
          type="error"
          :loading="submitting"
          :disabled="!canSubmit"
          data-testid="restore-confirm"
          @click="confirm"
        >
          {{ t('settings.data.msg.restoreConfirmAction') }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
