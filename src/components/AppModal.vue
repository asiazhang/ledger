<script setup lang="ts">
import { NModal } from 'naive-ui'

// AppModal（issue #251）：薄封装 NModal，收口弹层关闭语义——
// 默认 maskClosable=false，全部弹窗点遮罩不再关闭；点 ✕ / ESC 照常关闭。
// 其余 props/attrs/slots 原样透传（透传依赖单根节点 attrs fallthrough：
// attrs 合并到根 NModal，:show / v-model:show / @update:show 用法均不变；
// 若改多根模板需显式 v-bind="$attrs"）。maskClosable 收口值仍可被调用方
// 显式传 mask-closable 覆盖，属预期逃逸门（先例：PinyinSelect）。
withDefaults(defineProps<{ maskClosable?: boolean }>(), { maskClosable: false })
</script>

<template>
  <NModal :mask-closable="maskClosable">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NModal>
</template>
