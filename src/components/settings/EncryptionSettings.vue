<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { judgeMinLengthText } from '@/utils/field-error'
import { NAlert, NButton, NCard, NCheckbox, NForm, NFormItem, NInput, NSpace, NSpin, NSwitch, NText, NTooltip, useMessage } from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { restartAppShortly } from '@/utils/restart'
import { useAppStore } from '@/stores/app'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import AppDangerConfirmModal from '@/components/AppDangerConfirmModal.vue'
import type { EncryptionStatus } from '@/types'

// 加密卡片（issue #570/#571 / #574 / ADR-0075）：数据文件管理域的加密模式开关。
// 形态对标 DataLocationSettings——命令往返、组件内状态。转换由后端完成
// （三形态同一套整库转换机制、失败原库原样保留），成功后应用重启：
// 开启/修改主口令由启动解锁屏接管，关闭后不再出现解锁屏。
// 「本机记住主口令」（issue #574）：偏好是前端 localStorage 轻量设置项（app store），
// 钥匙串缓存内容为主口令本身；平台不支持（v1 非 macOS）时隐藏该选项、回退手输。

const message = useMessage()
const store = useAppStore()
const { rememberSupport, loadRememberSupport, syncRememberCache, clearRememberCache } =
  useEncryptionGate()

const status = ref<EncryptionStatus | null>(null)
const loading = ref(false)
const loadError = ref('')
const submitting = ref(false)
const submittingChange = ref(false)
const submittingDisable = ref(false)

// 开启加密确认弹窗（issue #650 / ADR-0078）：迁入共享危险确认封装（error 级），
// 承载无后门后果说明（忘记主口令 = 数据不可恢复）。`enableConfirmShow` 为受控显示开关。
const enableConfirmShow = ref(false)
// 修改主口令确认弹窗（issue #650 / ADR-0078）：从系统原生 confirm 升级为同封装的
// error 级应用内弹窗——与开启加密风险同级（遗忘新口令同样数据不可读）。
const changeConfirmShow = ref(false)
// 关闭加密确认弹窗（issue #652 / ADR-0078）：warning 级——破坏性但有兜底
// （既有密文副本保留、可再开启），原生 confirm 退役。
const disableConfirmShow = ref(false)

// 主口令最小长度（issue #650）：≥8，仅前端判定（后端契约不动）；走字段错误态
// 既有口径（ADR-0058）：短口令即时红显、提交禁用，不拦截键入。
const PASSPHRASE_MIN_LENGTH = 8

const passphrase = ref('')
const confirmPassphrase = ref('')

/** 新设主口令过短（字段错误态：格式类即时红，空值不在此列、走既有禁用逻辑）。 */
const passphraseTooShort = computed(
  () => judgeMinLengthText(passphrase.value, PASSPHRASE_MIN_LENGTH).kind === 'too-short',
)

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
/** 轮换后的新主口令同样受最小长度约束（issue #650），不弱于初始要求。 */
const changeNewTooShort = computed(
  () => judgeMinLengthText(changeNew.value, PASSPHRASE_MIN_LENGTH).kind === 'too-short',
)
const changeReady = computed(
  () =>
    changeOld.value.length > 0 &&
    changeNew.value.length > 0 &&
    !changeMismatch.value &&
    !changeUnchanged.value &&
    !changeNewTooShort.value,
)

// 关闭加密（已加密形态）：需当前主口令——文件级转换凭口令读取密文库。
const disablePassphrase = ref('')

// 本机记住主口令（issue #574）：`enableRemember`/`changeRemember` 是开启/修改主口令
// 表单的复选项（修改表单默认反映当前偏好）；`rememberSwitch` 是已加密状态下的
// 独立开关（关闭即清缓存恢复手输；开启需再次输入当前主口令以缓存）。
const enableRemember = ref(false)
const changeRemember = ref(store.rememberPassphrase)
const rememberSwitch = ref(store.rememberPassphrase)
const rememberPassInput = ref('')
const rememberEnabling = ref(false)

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

onMounted(() => {
  void refresh()
  void loadRememberSupport()
})

/** 开启加密第一步：弹确认弹窗（无后门后果说明），确认后执行整库转换。 */
function requestEnable() {
  if (submitting.value || mismatch.value || !passphrase.value || passphraseTooShort.value) return
  enableConfirmShow.value = true
}

/** 开启加密确认：转换 → 提示重启（Restore 同型：成功 toast 后延迟重启）。 */
async function confirmEnable() {
  enableConfirmShow.value = false
  if (submitting.value || mismatch.value || !passphrase.value) return
  submitting.value = true
  try {
    await api.enableEncryption(passphrase.value)
    const cached = await syncRememberCache(passphrase.value, enableRemember.value)
    if (!cached) message.warning(t('settings.data.encryption.rememberFailed'))
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

/** 清除「本机记住」的开关/输入态，并委托 composable 清缓存与偏好（关闭加密 /
 *  忘记口令重置 / 关闭开关共用）。 */
async function clearRemember() {
  rememberSwitch.value = false
  rememberPassInput.value = ''
  await clearRememberCache()
}

/** 独立开关（已加密状态）：开→需再输入当前主口令以缓存；关→立即清缓存恢复手输。 */
function onRememberToggle(value: boolean) {
  rememberSwitch.value = value
  if (!value) {
    void clearRemember()
  }
}

/** 开启「记住」确认：凭输入的主口令入缓存。失败回退开关为关并提示。 */
async function enableRememberNow() {
  if (rememberEnabling.value || !rememberPassInput.value) return
  rememberEnabling.value = true
  try {
    await api.setRememberPassphrase(rememberPassInput.value)
    store.setRememberPassphrase(true)
    rememberPassInput.value = ''
    message.success(t('settings.data.encryption.rememberEnabled'))
  } catch (e: any) {
    rememberSwitch.value = false
    message.error(errorMessage(e))
  } finally {
    rememberEnabling.value = false
  }
}

/** 修改主口令第一步：弹 error 级确认弹窗（无后门后果说明，ADR-0078），确认后执行转换。 */
function requestChange() {
  if (submittingChange.value || !changeReady.value) return
  changeConfirmShow.value = true
}

/** 修改主口令确认：旧口令验证通过后转入新口令的新库，完成后重启以新口令解锁。 */
async function confirmChange() {
  changeConfirmShow.value = false
  if (submittingChange.value || !changeReady.value) return
  submittingChange.value = true
  try {
    await api.changeEncryptionPassphrase(changeOld.value, changeNew.value)
    const cached = await syncRememberCache(changeNew.value, changeRemember.value)
    if (!cached) message.warning(t('settings.data.encryption.rememberFailed'))
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

/** 关闭加密第一步：弹 warning 级确认弹窗（兜底说明，ADR-0078），确认后执行转换。 */
function requestDisable() {
  if (submittingDisable.value || !disablePassphrase.value) return
  disableConfirmShow.value = true
}

/** 关闭加密确认：整库转回明文库，完成后重启，不再出现解锁屏。 */
async function confirmDisable() {
  disableConfirmShow.value = false
  if (submittingDisable.value || !disablePassphrase.value) return
  submittingDisable.value = true
  try {
    await api.disableEncryption(disablePassphrase.value)
    await clearRemember()
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

          <!-- 本机记住主口令（issue #574）：平台不支持（v1 非 macOS）时隐藏；
               关闭即清缓存恢复手输，开启需再次输入当前主口令以缓存。
               开发回退形态（issue #662）：提示当前为无门缓存，区别于发布生物门。 -->
          <NSpace v-if="rememberSupport?.supported" vertical :size="8">
            <NText depth="3">{{ t('settings.data.encryption.rememberToggleLabel') }}</NText>
            <NSwitch
              :value="rememberSwitch"
              @update:value="onRememberToggle"
              :disabled="rememberEnabling"
            />
            <NText depth="3" class="remember-hint">{{ t('settings.data.encryption.rememberToggleHint') }}</NText>
            <NText
              v-if="rememberSupport?.mode === 'dev-fallback'"
              type="warning"
              class="remember-hint"
            >
              {{ t('settings.data.encryption.rememberDevFallbackHint') }}
            </NText>
            <template v-if="rememberSwitch && !store.rememberPassphrase">
              <NInput
                v-model:value="rememberPassInput"
                type="password"
                show-password-on="click"
                :placeholder="t('settings.data.encryption.rememberPassLabel')"
                :disabled="rememberEnabling"
              />
              <NButton
                size="small"
                :loading="rememberEnabling"
                :disabled="!rememberPassInput"
                @click="enableRememberNow"
              >
                {{ t('settings.data.encryption.rememberConfirm') }}
              </NButton>
            </template>
          </NSpace>

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
              <NFormItem
                :label="t('settings.data.encryption.newPassphraseLabel')"
                :validation-status="changeNewTooShort ? 'error' : undefined"
                :feedback="
                  changeNewTooShort
                    ? t('settings.data.encryption.tooShort', { min: PASSPHRASE_MIN_LENGTH })
                    : undefined
                "
              >
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
                  @keyup.enter="requestChange"
                />
              </NFormItem>
              <NCheckbox
                v-if="rememberSupport?.supported"
                v-model:checked="changeRemember"
                :disabled="submittingChange"
              >
                <NText depth="3">{{ t('settings.data.encryption.rememberCheckbox') }}</NText>
              </NCheckbox>
              <NSpace>
                <NButton
                  type="primary"
                  :loading="submittingChange"
                  :disabled="!changeReady"
                  @click="requestChange"
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
                  @click="requestDisable"
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
            <NFormItem
              :label="t('settings.data.encryption.passphraseLabel')"
              :validation-status="passphraseTooShort ? 'error' : undefined"
              :feedback="
                passphraseTooShort
                  ? t('settings.data.encryption.tooShort', { min: PASSPHRASE_MIN_LENGTH })
                  : undefined
              "
            >
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
                @keyup.enter="requestEnable"
              />
            </NFormItem>
            <NCheckbox
              v-if="rememberSupport?.supported"
              v-model:checked="enableRemember"
              :disabled="submitting"
            >
              <NTooltip placement="top" :style="{ maxWidth: '320px' }">
                <template #trigger>
                  <NText depth="3">{{ t('settings.data.encryption.rememberCheckbox') }}</NText>
                </template>
                {{ t('settings.data.encryption.rememberCheckboxHint') }}
              </NTooltip>
            </NCheckbox>
            <NSpace>
              <NButton
                type="primary"
                :loading="submitting"
                :disabled="!passphrase || mismatch || passphraseTooShort"
                @click="requestEnable"
              >
                {{ t('settings.data.encryption.enable') }}
              </NButton>
            </NSpace>
          </NSpace>
        </NForm>
      </NSpin>
    </NSpace>

    <!-- 开启加密确认（issue #650 / ADR-0078）：共享危险确认封装 error 级，
         无后门后果说明、转换期间勿关应用、完成后自动重启语义不回退 -->
    <AppDangerConfirmModal
      level="error"
      v-model:show="enableConfirmShow"
      :title="t('settings.data.encryption.confirmTitle')"
      :lead="t('settings.data.encryption.enableConfirmLead')"
      :alert-title="t('settings.data.encryption.warnTitle')"
      :strong-warning="t('settings.data.encryption.enableConfirmStrong')"
      :detail="t('settings.data.encryption.enableConfirmRest')"
      :confirm-text="t('settings.data.encryption.enableConfirmOk')"
      :cancel-text="t('settings.data.encryption.enableConfirmCancel')"
      :submitting="submitting"
      :on-confirm="confirmEnable"
      :on-cancel="() => (enableConfirmShow = false)"
    />

    <!-- 修改主口令确认（issue #650 / ADR-0078）：error 级（与开启加密同级），
         承载无后门后果说明（新主口令遗忘即数据不可读）；确认后整库转换与自动重启流程不变 -->
    <AppDangerConfirmModal
      level="error"
      v-model:show="changeConfirmShow"
      :title="t('settings.data.encryption.changeConfirmTitle')"
      :lead="t('settings.data.encryption.changeConfirmLead')"
      :alert-title="t('settings.data.encryption.warnTitle')"
      :strong-warning="t('settings.data.encryption.changeConfirmStrong')"
      :detail="t('settings.data.encryption.changeConfirmRest')"
      :confirm-text="t('settings.data.encryption.changeConfirmOk')"
      :cancel-text="t('settings.data.encryption.changeConfirmCancel')"
      :submitting="submittingChange"
      :on-confirm="confirmChange"
      :on-cancel="() => (changeConfirmShow = false)"
    />

    <!-- 关闭加密确认（issue #652 / ADR-0078）：warning 级——破坏性但有兜底
         （既有密文备份保留、可再开启），确认后整库转换与自动重启流程不变 -->
    <AppDangerConfirmModal
      level="warning"
      v-model:show="disableConfirmShow"
      :title="t('settings.data.encryption.disableConfirmTitle')"
      :lead="t('settings.data.encryption.disableConfirmLead')"
      :alert-title="t('settings.data.encryption.disableWarnTitle')"
      :strong-warning="t('settings.data.encryption.disableConfirmStrong')"
      :detail="t('settings.data.encryption.disableConfirmRest')"
      :confirm-text="t('settings.data.encryption.disableConfirmOk')"
      :cancel-text="t('settings.data.encryption.disableConfirmCancel')"
      :submitting="submittingDisable"
      :on-confirm="confirmDisable"
      :on-cancel="() => (disableConfirmShow = false)"
    />
  </NCard>
</template>

<style scoped>
.remember-hint {
  font-size: 12px;
  line-height: 1.6;
}
</style>
