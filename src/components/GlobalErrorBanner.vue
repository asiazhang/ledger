<script setup lang="ts">
// 全局渲染错误提示条（issue #926）：窗口顶部错误带，非阻断环境指示。
// 与 GlobalBusyBar 同形态学——非模态、不接弹层注册表、不抑制快捷键
// （ADR-0035 豁免同口径）；可见性唯一来源是渲染错误 store 的 message，
// 本组件不持状态。文案前缀经 i18n（ADR-0049），错误摘要本身是技术文本
// 原样展示（帮用户反馈时说清错误）；关闭按钮即清除展示（store dismiss）。
import { NButton } from 'naive-ui'
import { useThemeVars } from 'naive-ui'
import { useRenderErrorsStore } from '@/stores/render-errors'
import { t } from '@ledger/i18n'
import { banner, text, closeButton } from './global-error-banner.css.ts'

const store = useRenderErrorsStore()
const themeVars = useThemeVars()
</script>

<template>
  <Transition name="global-error-banner">
    <div
      v-if="store.message"
      :class="banner"
      role="alert"
      :style="{ '--error-color': themeVars.errorColor }"
    >
      <span :class="text">{{ t('common.globalErrorBanner.prefix') }}{{ store.message }}</span>
      <NButton
        size="tiny"
        quaternary
        :class="closeButton"
        :aria-label="t('common.globalErrorBanner.dismiss')"
        @click="store.dismiss()"
      >
        {{ t('common.globalErrorBanner.dismiss') }}
      </NButton>
    </div>
  </Transition>
</template>
