<script setup lang="ts">
import { computed, useAttrs, watch } from 'vue'
import { NModal } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'
import { useWindowTier } from '@/composables/useWindowTier'
import { MOBILE_CARD_CLASS } from './app-modal.css.ts'

// AppModal（issue #251）：薄封装 NModal，收口弹层关闭语义——
// 默认 maskClosable=false，全部弹窗点遮罩不再关闭；点 ✕ / ESC 照常关闭。
// 其余 props/attrs/slots 原样透传（透传依赖单根节点 attrs fallthrough：
// attrs 合并到根 NModal，:show / v-model:show / @update:show 用法均不变；
// 若改多根模板需显式 v-bind="$attrs"）。maskClosable 收口值仍可被调用方
// 显式传 mask-closable 覆盖，属预期逃逸门（先例：PinyinSelect）。
//
// 卡牌弹窗视觉规范（issue #631，spec #630）：cardSize 把对话框宽度分档
// （各档像素值见 CARD_WIDTH_PX，常量是分档宽度的唯一事实源）内化为本模块
// 的单一声明，调用点不再散落 style 宽度 magic number；卡片默认无边框
// （bordered=false），把卡片外观默认值钉在封装层而非依赖 naive-ui 的上游
// 默认。两者均可被调用方显式覆盖：仍传 style 宽度或 :bordered 的既有调用
// 点行为不变（style 冲突时调用方显式值胜出）。
//
// 另接入弹层注册表（ADR-0035）：开/关状态实时上报，驱动快捷键抑制。刻意
// 不声明 show prop（原因见 AppSelect 注释）：非受控/受控内部触发的开合经根上
// 的 update:show 监听上报，受控调用方直接改 :show prop 的开合由 attrs watch
// 兜底。
//
// 弹窗移动档（issue #844 / ADR-0088 决策 8）：窗口分级落移动档（<840）时统一
// 走全屏化分支——近全屏卡片、标签上置、按钮行底部固定（后两者为纯 CSS，收口
// 旁路样式文件 app-modal.css.ts 的移动钩子类下）；cardSize 三档宽度是桌面档
// 口径，移动档不获宽度。调用点零改动：全部弹窗经本封装自动获得移动档形态。
// 尺寸走内联 style（胜过 naive 运行时注入的 .n-card { width: 100% } 类规则，
// 与桌面 cardSize 同一机制）；弹层关闭语义、注册表上报、ModalIntent 编排
// （ADR-0072）零变化。near-fullscreen 留 32px 边距维持卡片观感，高度扣除上下
// 安全区（viewport-fit=cover 下 env 生效，桌面档恒 0）避开状态栏/手势条。

/** 卡牌弹窗宽度分档：sm 编辑类 / md 表单类 / lg 详情类（spec #630）。 */
type CardSize = 'sm' | 'md' | 'lg'

const CARD_WIDTH_PX: Record<CardSize, number> = { sm: 420, md: 480, lg: 560 }

/** 移动档近全屏卡片尺寸（宽 32px 边距；高度另扣上下安全区）。 */
const MOBILE_CARD_STYLE = {
  width: 'calc(100vw - 32px)',
  height: 'calc(100dvh - 32px - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px))',
}

const props = withDefaults(
  defineProps<{ maskClosable?: boolean; cardSize?: CardSize; bordered?: boolean }>(),
  { maskClosable: false, bordered: false },
)

const tier = useWindowTier()
const isMobileTier = computed(() => tier.value === 'mobile')

const cardStyle = computed(() => {
  if (isMobileTier.value) return MOBILE_CARD_STYLE
  return props.cardSize === undefined ? undefined : { width: `${CARD_WIDTH_PX[props.cardSize]}px` }
})

const attrs = useAttrs()
const overlay = createOverlayToken('modal')
const onUpdateShow = (value: boolean) => overlay.set(value)
watch(
  () => attrs.show,
  (value) => {
    if (value !== undefined) overlay.set(Boolean(value))
  },
  { immediate: true },
)
</script>

<template>
  <NModal
    :class="isMobileTier ? MOBILE_CARD_CLASS : undefined"
    :mask-closable="maskClosable"
    :bordered="bordered"
    :style="cardStyle"
    @update:show="onUpdateShow"
  >
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NModal>
</template>
