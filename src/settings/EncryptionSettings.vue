<script setup lang="ts">
import {
  NAlert,
  NButton,
  NCard,
  NCheckbox,
  NCollapse,
  NCollapseItem,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  NSpin,
  NText,
  NTooltip,
} from 'naive-ui'
import { t } from '@ledger/i18n'
import AppDangerConfirmModal from '@ledger/ui-kit/AppDangerConfirmModal.vue'
import AppModal from '@ledger/ui-kit/AppModal.vue'
import PassphraseStrengthMeter from '@/settings/PassphraseStrengthMeter.vue'
import { PASSPHRASE_MIN_LENGTH, useEncryptionTransitions } from '@/backup/useEncryptionTransitions'

// 加密卡片（issue #570/#571 / #574 / ADR-0075；#654 重排）：数据文件管理域的加密模式开关。
// 三条形态转换流（开启 / 修改主口令 / 关闭）与自动解锁启用的编排住
// useEncryptionTransitions 深模块（issue #1396，备份与数据文件域，与
// useEncryptionGate / restart 同区）；本组件是它的薄 adapter：只留设置页签面——
// 状态渲染、四个确认弹窗（AppDangerConfirmModal / AppModal）的模板与 i18n 文案
// props（「确认弹层文案留调用方」，ADR-0078）。
// 已加密形态为日常视图：「已开启」标识 + 自动解锁；修改主口令、关闭加密两个
// 低频流程收进默认收起的折叠区（展开后流程与分级确认不变，ADR-0078）。
const {
  // 状态加载
  status,
  statusLoading,
  statusError,
  refresh,
  // 开启流
  passphrase,
  confirmPassphrase,
  enableRemember,
  passphraseStrength,
  passphraseTooShort,
  mismatch,
  enableConfirmShow,
  requestEnable,
  confirmEnable,
  submitting,
  // 修改流
  changeOld,
  changeNew,
  changeConfirm,
  changeRemember,
  changeNewStrength,
  changeMismatch,
  changeUnchanged,
  changeNewTooShort,
  changeReady,
  changeConfirmShow,
  requestChange,
  confirmChange,
  submittingChange,
  // 关闭流
  disablePassphrase,
  disableConfirmShow,
  requestDisable,
  confirmDisable,
  submittingDisable,
  // 自动解锁
  rememberSupport,
  autoUnlockOn,
  autoUnlockModalShow,
  autoUnlockPass,
  autoUnlockError,
  autoUnlockSubmitting,
  openAutoUnlockModal,
  confirmAutoUnlock,
  disableAutoUnlock,
} = useEncryptionTransitions()
</script>

<template>
  <NCard :title="t('settings.data.encryption.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">{{ t('settings.data.encryption.hint') }}</NText>

      <NSpin :show="statusLoading">
        <NSpace v-if="statusError" align="center" :size="12">
          <NText type="error">{{ statusError }}</NText>
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
            <!-- 提示按运行形态区分（issue #687）：dev 回退形态不宣称 Touch ID，
                 免与上方 warning 提示同屏矛盾；发布形态保留生物门表述。 -->
            <NText depth="3" class="remember-hint">
              {{
                rememberSupport?.mode === 'dev-fallback'
                  ? t('settings.data.encryption.rememberToggleDevFallbackHint')
                  : t('settings.data.encryption.rememberToggleHint')
              }}
            </NText>
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
                <!-- 同 issue #687：tooltip 按运行形态区分，dev 回退不宣称 Touch ID。 -->
                {{
                  rememberSupport?.mode === 'dev-fallback'
                    ? t('settings.data.encryption.rememberCheckboxDevFallbackHint')
                    : t('settings.data.encryption.rememberCheckboxHint')
                }}
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
