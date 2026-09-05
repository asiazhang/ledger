<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { NAlert, NButton, NCard, NForm, NFormItem, NInput, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { api } from '@/api'
import { t } from '@/i18n'
import { restartAppShortly } from '@/utils/restart'
import type { EncryptionStatus } from '@/types'

// 加密卡片（issue #570/#571 / ADR-0075）：数据文件管理域的加密模式开关。
// 形态对标 DataLocationSettings——命令往返、组件内状态。转换由后端完成
// （三形态同一套整库转换机制、失败原库原样保留），成功后应用重启：
// 开启/修改主口令由启动解锁屏接管，关闭后不再出现解锁屏。

const message = useMessage()

const status = ref<EncryptionStatus | null>(null)
const loading = ref(false)
const loadError = ref('')
const submitting = ref(false)
const submittingChange = ref(false)
const submittingDisable = ref(false)

const passphrase = ref('')
const confirmPassphrase = ref('')

/** 两次输入一致才允许提交（确认输错的即时反馈）。 */
const mismatch = computed(
  () => confirmPassphrase.value.length > 0 && confirmPassphrase.value !== passphrase.value,
)

// 修改主口令（已加密形态）：旧口令验证 + 新口令（含确认、须不同于旧口令）。
const changeOld = ref('')
const changeNew = ref('')
const changeConfirm = ref('')
const changeMismatch = computed(
  () => changeConfirm.value.length > 0 && changeConfirm.value !== changeNew.value,
)
const changeUnchanged = computed(
  () => changeNew.value.length > 0 && changeNew.value === changeOld.value,
)
const changeReady = computed(
  () => changeOld.value.length > 0 && changeNew.value.length > 0 && !changeMismatch.value && !changeUnchanged.value,
)

// 关闭加密（已加密形态）：需当前主口令——文件级转换凭口令读取密文库。
const disablePassphrase = ref('')

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

/** 修改主口令：旧口令验证通过后转入新口令的新库，完成后重启以新口令解锁。 */
async function changePassphrase() {
  if (submittingChange.value || !changeReady.value) return
  const ok = await confirm(t('settings.data.encryption.changeConfirmBody'), {
    title: t('settings.data.encryption.changeConfirmTitle'),
    kind: 'warning',
  })
  if (!ok) return
  submittingChange.value = true
  try {
    await api.changeEncryptionPassphrase(changeOld.value, changeNew.value)
    changeOld.value = ''
    changeNew.value = ''
    changeConfirm.value = ''
    message.success(t('settings.data.encryption.changeOkToast'))
    restartAppShortly()
  } catch (e: any) {
    message.error(errorMessage(e))
  } finally {
    submittingChange.value = false
  }
}

/** 关闭加密：整库转回明文库，完成后重启，不再出现解锁屏。 */
async function disable() {
  if (submittingDisable.value || !disablePassphrase.value) return
  const ok = await confirm(t('settings.data.encryption.disableConfirmBody'), {
    title: t('settings.data.encryption.disableConfirmTitle'),
    kind: 'warning',
  })
  if (!ok) return
  submittingDisable.value = true
  try {
    await api.disableEncryption(disablePassphrase.value)
    disablePassphrase.value = ''
    message.success(t('settings.data.encryption.disableOkToast'))
    restartAppShortly()
  } catch (e: any) {
    message.error(errorMessage(e))
  } finally {
    submittingDisable.value = false
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

        <NSpace v-else-if="status?.file_encrypted" vertical :size="16">
          <NAlert type="success" :show-icon="true" :title="t('settings.data.encryption.enabledTitle')">
            {{ t('settings.data.encryption.enabledBody') }}
          </NAlert>

          <NForm label-placement="top">
            <NSpace vertical :size="12">
              <NText depth="3">{{ t('settings.data.encryption.changeHint') }}</NText>
              <NFormItem :label="t('settings.data.encryption.oldPassphraseLabel')">
                <NInput
                  v-model:value="changeOld"
                  type="password"
                  show-password-on="click"
                  :placeholder="t('settings.data.encryption.oldPassphrasePlaceholder')"
                  :disabled="submittingChange"
                />
              </NFormItem>
              <NFormItem :label="t('settings.data.encryption.newPassphraseLabel')">
                <NInput
                  v-model:value="changeNew"
                  type="password"
                  show-password-on="click"
                  :placeholder="t('settings.data.encryption.newPassphrasePlaceholder')"
                  :disabled="submittingChange"
                />
              </NFormItem>
              <NFormItem
                :label="t('settings.data.encryption.confirmNewLabel')"
                :validation-status="changeMismatch ? 'error' : changeUnchanged ? 'warning' : undefined"
                :feedback="
                  changeMismatch
                    ? t('settings.data.encryption.mismatch')
                    : changeUnchanged
                      ? t('settings.data.encryption.unchanged')
                      : undefined
                "
              >
                <NInput
                  v-model:value="changeConfirm"
                  type="password"
                  show-password-on="click"
                  :placeholder="t('settings.data.encryption.confirmNewPlaceholder')"
                  :disabled="submittingChange"
                  @keyup.enter="changePassphrase"
                />
              </NFormItem>
              <NSpace>
                <NButton
                  type="primary"
                  :loading="submittingChange"
                  :disabled="!changeReady"
                  @click="changePassphrase"
                >
                  {{ t('settings.data.encryption.change') }}
                </NButton>
              </NSpace>
            </NSpace>
          </NForm>

          <NForm label-placement="top">
            <NSpace vertical :size="12">
              <NAlert type="warning" :show-icon="true" :title="t('settings.data.encryption.disableWarnTitle')">
                {{ t('settings.data.encryption.disableWarnBody') }}
              </NAlert>
              <NFormItem :label="t('settings.data.encryption.disablePassphraseLabel')">
                <NInput
                  v-model:value="disablePassphrase"
                  type="password"
                  show-password-on="click"
                  :placeholder="t('settings.data.encryption.disablePassphrasePlaceholder')"
                  :disabled="submittingDisable"
                />
              </NFormItem>
              <NSpace>
                <NButton
                  type="warning"
                  :loading="submittingDisable"
                  :disabled="!disablePassphrase"
                  @click="disable"
                >
                  {{ t('settings.data.encryption.disable') }}
                </NButton>
              </NSpace>
            </NSpace>
          </NForm>
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
