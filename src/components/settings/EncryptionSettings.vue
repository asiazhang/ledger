<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { judgeMinLengthText } from '@/utils/field-error'
import {
  assessPassphraseStrength,
  type PassphraseStrengthAssessment,
} from '@/utils/passphrase-strength'
import { NAlert, NButton, NCard, NCheckbox, NCollapse, NCollapseItem, NForm, NFormItem, NInput, NSpace, NSpin, NText, NTooltip, useMessage } from 'naive-ui'
import { computed, onMounted, ref, watch, type Ref } from 'vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { restartAppShortly } from '@/utils/restart'
import { useAppStore } from '@/stores/app'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import AppDangerConfirmModal from '@/components/AppDangerConfirmModal.vue'
import AppModal from '@/components/AppModal.vue'
import PassphraseStrengthMeter from '@/components/settings/PassphraseStrengthMeter.vue'
import type { EncryptionStatus } from '@/types'

// 加密卡片（issue #570/#571 / #574 / ADR-0075；#654 重排）：数据文件管理域的加密模式开关。
// 形态对标 DataLocationSettings——命令往返、组件内状态。转换由后端完成
// （三形态同一套整库转换机制、失败原库原样保留），成功后应用重启：
// 开启/修改主口令由启动解锁屏接管，关闭后不再出现解锁屏。
// 已加密形态为日常视图：「已开启」标识 + 自动解锁；修改主口令、关闭加密两个
// 低频流程收进默认收起的折叠区（展开后流程与分级确认不变，ADR-0078）。
// 自动解锁（issue #654 重做）：偏好是前端 localStorage 轻量设置项（app store），
// 钥匙串缓存内容为主口令本身；平台不支持（v1 非 macOS）时隐藏该区块。

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

// 口令强度实时显示（issue #685，词汇表「口令强度」）：纯信息反馈，不拦截提交、
// 不改提交可用性；只接新设主口令两框（开启加密「主口令」+ 修改主口令「新主口令」），
// 确认字段与已存在口令的输入场景一律不接。判定与映射收口在
// src/utils/passphrase-strength.ts，此处只消费（最后一次胜出守卫保证逐键刷新不串档）。
function trackPassphraseStrength(source: Ref<string>) {
  const assessment = ref<PassphraseStrengthAssessment | null>(null)
  let latest = 0
  watch(source, (value) => {
    const seq = ++latest
    void assessPassphraseStrength(value).then((result) => {
      if (seq === latest) assessment.value = result
    })
  })
  return assessment
}

const passphraseStrength = trackPassphraseStrength(passphrase)

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
const changeNewStrength = trackPassphraseStrength(changeNew)
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

// 自动解锁（issue #654 重做）：状态唯一事实源 = store.rememberPassphrase（偏好与
// 钥匙串缓存同批建立/清除，无本地开关镜像）。「开关开着但未生效」的可持续中间态
// 从形态上消灭：启用 = 「启用自动解锁…」按钮弹小窗，凭当前主口令建立缓存、成功才
// 置偏好；关闭 = 立即清缓存恢复手输并提示。
const enableRemember = ref(false)
const changeRemember = ref(store.rememberPassphrase)
const autoUnlockModalShow = ref(false)
const autoUnlockPass = ref('')
const autoUnlockError = ref('')
const autoUnlockSubmitting = ref(false)

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

/** 自动解锁是否已启用（唯一事实源 = 偏好，与钥匙串缓存同批建立/清除）。 */
const autoUnlockOn = computed(() => store.rememberPassphrase)

/** 启用自动解锁第一步：打开小弹窗（清上次输入与错误）。 */
function openAutoUnlockModal() {
  autoUnlockPass.value = ''
  autoUnlockError.value = ''
  autoUnlockModalShow.value = true
}

/** 启用自动解锁确认：凭当前主口令建立缓存，成功才置偏好并提示；
 *  口令错误就地报错（弹窗保持打开可重试）、不启用——不存在中间态。 */
async function confirmAutoUnlock() {
  if (autoUnlockSubmitting.value || !autoUnlockPass.value) return
  autoUnlockSubmitting.value = true
  autoUnlockError.value = ''
  try {
    await api.setRememberPassphrase(autoUnlockPass.value)
    store.setRememberPassphrase(true)
    autoUnlockModalShow.value = false
    autoUnlockPass.value = ''
    message.success(t('settings.data.encryption.rememberEnabled'))
  } catch (e: any) {
    autoUnlockError.value = errorMessage(e)
  } finally {
    autoUnlockSubmitting.value = false
  }
}

/** 关闭自动解锁：立即清缓存恢复手输并提示（清缓存幂等，无失败悬挂态）。 */
async function disableAutoUnlock() {
  await clearRememberCache()
  message.success(t('settings.data.encryption.rememberDisabledToast'))
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
    await clearRememberCache()
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

          <!-- 自动解锁（issue #654 重做）：日常视图常驻；未启用 = 「启用自动解锁…」单按钮
               入口（小弹窗凭当前主口令建立缓存），已启用 = 「关闭自动解锁」立即清缓存恢复
               手输。平台不支持（v1 非 macOS）时整块隐藏。
               开发回退形态（issue #662）：提示当前为无门缓存，区别于发布生物门。 -->
          <NSpace v-if="rememberSupport?.supported" vertical :size="8">
            <NText depth="3">{{ t('settings.data.encryption.rememberSectionLabel') }}</NText>
            <NText depth="3" class="remember-hint">
              {{
                autoUnlockOn
                  ? t('settings.data.encryption.rememberStatusOn')
                  : t('settings.data.encryption.rememberStatusOff')
              }}
            </NText>
            <NButton v-if="!autoUnlockOn" size="small" @click="openAutoUnlockModal">
              {{ t('settings.data.encryption.rememberEnableButton') }}
            </NButton>
            <NButton v-else size="small" :disabled="autoUnlockSubmitting" @click="disableAutoUnlock">
              {{ t('settings.data.encryption.rememberDisableButton') }}
            </NButton>
            <NText
              v-if="rememberSupport?.mode === 'dev-fallback'"
              type="warning"
              class="remember-hint"
            >
              {{ t('settings.data.encryption.rememberDevFallbackHint') }}
            </NText>
            <NText depth="3" class="remember-hint">{{ t('settings.data.encryption.rememberToggleHint') }}</NText>
          </NSpace>

          <!-- 低频高危流程折叠区（issue #654）：默认收起，减少误触面；展开后流程与
               分级确认不变（修改主口令 = error 级、关闭加密 = warning 级，ADR-0078）。 -->
          <NCollapse>
            <NCollapseItem :title="t('settings.data.encryption.change')" name="change">
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
                  <div>
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
                    <!-- 口令强度（issue #685）：同开启表单，仅新设口令框显示 -->
                    <PassphraseStrengthMeter v-if="changeNewStrength" :assessment="changeNewStrength" />
                  </div>
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
            </NCollapseItem>
            <NCollapseItem :title="t('settings.data.encryption.disable')" name="disable">
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
            </NCollapseItem>
          </NCollapse>
        </NSpace>

        <NForm v-else label-placement="top">
          <NSpace vertical :size="12">
            <NAlert type="warning" :show-icon="true" :title="t('settings.data.encryption.warnTitle')">
              {{ t('settings.data.encryption.warnBody') }}
            </NAlert>
            <div>
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
              <!-- 口令强度（issue #685）：初始为空不显示；与字段错误红显并存互不替代 -->
              <PassphraseStrengthMeter v-if="passphraseStrength" :assessment="passphraseStrength" />
            </div>
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

    <!-- 启用自动解锁小弹窗（issue #654）：输入当前主口令 → 确认 → 成功提示；
         口令错误就地报错、弹窗保持打开可重试，偏好不置位（无中间态）。
         弹层纪律：AppModal 收口关闭语义（遮罩不关）并接入弹层注册表（快捷键抑制）。 -->
    <AppModal
      v-model:show="autoUnlockModalShow"
      preset="card"
      card-size="sm"
      :title="t('settings.data.encryption.rememberEnableModalTitle')"
    >
      <NSpace vertical :size="12">
        <NText depth="3">{{ t('settings.data.encryption.rememberEnableModalLead') }}</NText>
        <NInput
          v-model:value="autoUnlockPass"
          type="password"
          show-password-on="click"
          :placeholder="t('settings.data.encryption.rememberEnableModalPlaceholder')"
          :disabled="autoUnlockSubmitting"
          @keyup.enter="confirmAutoUnlock"
        />
        <NText v-if="autoUnlockError" type="error">{{ autoUnlockError }}</NText>
        <NSpace justify="end">
          <NButton
            :disabled="autoUnlockSubmitting"
            data-testid="auto-unlock-cancel"
            @click="autoUnlockModalShow = false"
          >
            {{ t('settings.data.encryption.rememberEnableModalCancel') }}
          </NButton>
          <NButton
            type="primary"
            :loading="autoUnlockSubmitting"
            :disabled="!autoUnlockPass || autoUnlockSubmitting"
            data-testid="auto-unlock-confirm"
            @click="confirmAutoUnlock"
          >
            {{ t('settings.data.encryption.rememberEnableModalOk') }}
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>
  </NCard>
</template>

<style scoped>
.remember-hint {
  font-size: 12px;
  line-height: 1.6;
}
</style>
