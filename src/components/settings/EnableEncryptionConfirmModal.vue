<script setup lang="ts">
import { NAlert, NButton, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import { t } from '@/i18n'

/**
 * 开启加密确认弹窗（issue #570 / ADR-0075 决策 2）：把开启加密的确认从系统
 * confirm 弹窗升级为应用内弹窗，承载「忘记主口令即数据不可恢复」的无后门
 * 后果说明——开启加密是**不可逆的关键决策**，用红色确认按钮 + 警示块强化
 * 视觉重量，与 RestoreConfirmModal（恢复备份确认）同为「危险/不可逆操作」
 * 的分级确认面。
 *
 * 纯确认、不带口令输入（口令已在开启表单录入并经弹窗外的提交建立）：本弹窗
 * 只在 `show` 为 true 时显示，确认/取消走 `onConfirm`/`onCancel` 回调，由
 * 父组件（EncryptionSettings）承接实际的整库转换与重启编排。
 *
 * 弹层纪律：AppModal 收口关闭语义（点遮罩不关）并接入弹层注册表（快捷键抑制）。
 */
const props = defineProps<{
  /** 是否显示（受控）；false 卸载态不渲染内容。 */
  show: boolean
  /** 是否正在提交（开启加密转换中），期间禁用按钮与关闭。 */
  submitting: boolean
  /** 确认（继续开启加密）：父组件执行整库转换。 */
  onConfirm: () => void
  /** 取消：父组件关闭弹窗。 */
  onCancel: () => void
}>()

const emit = defineEmits<{ 'update:show': [value: boolean] }>()

/** 点 ✕ / ESC（NModal 关闭意图）时上报 update:show(false)，由父组件判断是否可关。 */
function onUpdateShow(value: boolean) {
  if (!value) emit('update:show', false)
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('settings.data.encryption.confirmTitle')"
    style="width: 480px"
    :bordered="false"
    :mask-closable="false"
    @update:show="onUpdateShow"
  >
    <NSpace vertical :size="12">
      <NText depth="3">{{ t('settings.data.encryption.enableConfirmLead') }}</NText>

      <!-- 无后门后果说明：红色警示块强调不可逆（忘记主口令 = 数据无法再打开） -->
      <NAlert type="error" :show-icon="true" :title="t('settings.data.encryption.warnTitle')">
        <NText strong type="error">{{ t('settings.data.encryption.enableConfirmStrong') }}</NText>
      </NAlert>

      <NText depth="3">{{ t('settings.data.encryption.enableConfirmRest') }}</NText>

      <NSpace justify="end">
        <NButton :disabled="submitting" data-testid="encrypt-cancel" @click="onCancel">
          {{ t('settings.data.encryption.enableConfirmCancel') }}
        </NButton>
        <NButton
          type="error"
          :loading="submitting"
          :disabled="submitting"
          data-testid="encrypt-confirm"
          @click="onConfirm"
        >
          {{ t('settings.data.encryption.enableConfirmOk') }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
