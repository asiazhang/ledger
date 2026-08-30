<script setup lang="ts">
import { useAttrs, watch } from 'vue'
import { NModal } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

// AppModal（issue #251）：薄封装 NModal，收口弹层关闭语义——
// 默认 maskClosable=false，全部弹窗点遮罩不再关闭；点 ✕ / ESC 照常关闭。
// 其余 props/attrs/slots 原样透传（透传依赖单根节点 attrs fallthrough：
// attrs 合并到根 NModal，:show / v-model:show / @update:show 用法均不变；
// 若改多根模板需显式 v-bind="$attrs"）。maskClosable 收口值仍可被调用方
// 显式传 mask-closable 覆盖，属预期逃逸门（先例：PinyinSelect）。
//
// 另接入弹层注册表（ADR-0035）：开/关状态实时上报，驱动快捷键抑制。刻意
// 不声明 show prop（原因见 AppSelect 注释）：非受控/受控内部触发的开合经根上
// 的 update:show 监听上报，受控调用方直接改 :show prop 的开合由 attrs watch
// 兜底。
withDefaults(defineProps<{ maskClosable?: boolean }>(), { maskClosable: false })

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
  <NModal :mask-closable="maskClosable" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NModal>
</template>
