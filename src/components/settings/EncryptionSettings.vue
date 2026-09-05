<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { NAlert, NButton, NCard, NForm, NFormItem, NInput, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { api } from '@/api'
import { t } from '@/i18n'
import { restartAppShortly } from '@/utils/restart'
import type { EncryptionStatus } from '@/types'

// 加密卡片（issue #570 / ADR-0075）：数据文件管理域的加密模式开关。
// 形态对标 DataLocationSettings——命令往返、组件内状态；「关闭加密」「修改
// 主口令」由后续票交付，本票只有「开启加密」一个方向。转换由后端完成
// （整库一次性转换、失败原库原样保留），成功后应用重启，由启动解锁屏接管。

const message = useMessage()

const status = ref<EncryptionStatus | null>(null)
const loading = ref(false)
const loadError = ref('')
const submitting = ref(false)

const passphrase = ref('')
const confirmPassphrase = ref('')

/** 两次输入一致才允许提交（确认输错的即时反馈）。 */
const mismatch = computed(
  () => confirmPassphrase.value.length > 0 && confirmPassphrase.value !== passphrase.value,
)

async function refresh() {
  loading.value = true
  loadError.value = ''
  try {
    status.value = await api.getEncryptionStatus()
  } catch (e: any) {
    loadError.value = errorMessage(e)
    message.error(loadError.value)
  } finally {
    loading.value = false
  }
}

onMounted(refresh)

/** 开启加密：确认 → 转换 → 提示重启（Restore 同型：成功 toast 后延迟重启）。 */
async function enable() {
  if (submitting.value || mismatch.value || !passphrase.value) return
  const ok = await confirm(t('settings.data.encryption.confirmBody'), {
    title: t('settings.data.encryption.confirmTitle'),
    kind: 'warning',
  })
  if (!ok) return
  submitting.value = true
  try {
    await api.enableEncryption(passphrase.value)
    passphrase.value = ''
    confirmPassphrase.value = ''
    message.success(t('settings.data.encryption.okToast'))
    // 转换已落盘，重启以凭新口令重新打开（toast 先落地，Restore 先例）。
    restartAppShortly()
  } catch (e: any) {
    message.error(errorMessage(e))
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <NCard :title="t('settings.data.encryption.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">{{ t('settings.data.encryption.hint') }}</NText>

      <NSpin :show="loading">
        <NSpace v-if="loadError" align="center" :size="12">
          <NText type="error">{{ loadError }}</NText>
          <NButton size="small" @click="refresh">{{ t('settings.data.encryption.retry') }}</NButton>
        </NSpace>

        <NSpace v-else-if="status?.file_encrypted" vertical :size="8">
          <NAlert type="success" :show-icon="true" :title="t('settings.data.encryption.enabledTitle')">
            {{ t('settings.data.encryption.enabledBody') }}
          </NAlert>
        </NSpace>

        <NForm v-else label-placement="top">
          <NSpace vertical :size="12">
            <NAlert type="warning" :show-icon="true" :title="t('settings.data.encryption.warnTitle')">
              {{ t('settings.data.encryption.warnBody') }}
            </NAlert>
            <NFormItem :label="t('settings.data.encryption.passphraseLabel')">
              <NInput
                v-model:value="passphrase"
                type="password"
                show-password-on="click"
                :placeholder="t('settings.data.encryption.passphrasePlaceholder')"
                :disabled="submitting"
              />
            </NFormItem>
            <NFormItem
              :label="t('settings.data.encryption.confirmLabel')"
              :validation-status="mismatch ? 'error' : undefined"
              :feedback="mismatch ? t('settings.data.encryption.mismatch') : undefined"
            >
              <NInput
                v-model:value="confirmPassphrase"
                type="password"
                show-password-on="click"
                :placeholder="t('settings.data.encryption.confirmPlaceholder')"
                :disabled="submitting"
                @keyup.enter="enable"
              />
            </NFormItem>
            <NSpace>
              <NButton
                type="primary"
                :loading="submitting"
                :disabled="!passphrase || mismatch"
                @click="enable"
              >
                {{ t('settings.data.encryption.enable') }}
              </NButton>
            </NSpace>
          </NSpace>
        </NForm>
      </NSpin>
    </NSpace>
  </NCard>
</template>
