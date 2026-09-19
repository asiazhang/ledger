<script setup lang="ts">
import { computed } from "vue";
import { NTooltip } from "naive-ui";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import { useInputMode } from "@/composables/useInputMode";

/**
 * 口径说明气泡宿主（issue #1369）：双轴形态的唯一实现——指针轴悬挂停即现的裸
 * NTooltip（ADR-0088 决策 6「悬停才可见的信息触控档必须一击可达」的指针半边；
 * 裸 NTooltip 不在 ADR-0035 弹层注册表枚举内），触控轴改挂注册过的 AppPopover
 * （点按弹出）。两个投资域消费方（ConceptLabel 的标签+ⓘ、MwrRateCell 的收益率
 * 角标「*」）形态不同但轴分流相同，本例只承载分流与气泡体。
 *
 * 触发器形态归调用方：作用域插槽给出 `isTouch`，调用方据此按轴挂触控热区
 * （.touch-hit-area + --touch-hit-inset）与读屏替代（role/aria 只在点按轴成立）。
 * 气泡正文由调用方计算传入（唯一文案源在各概念的 i18n 键），两轴同一份。
 */
defineProps<{
  /** 气泡正文：指针轴与触控轴同源 */
  text: string;
}>();

const inputMode = useInputMode();
const isTouch = computed(() => inputMode.value === "touch");
</script>

<template>
  <NTooltip v-if="!isTouch" placement="top" :style="{ maxWidth: '320px' }">
    <template #trigger>
      <slot :is-touch="false" />
    </template>
    {{ text }}
  </NTooltip>
  <AppPopover v-else trigger="click" placement="top" :style="{ maxWidth: '320px' }">
    <template #trigger>
      <slot :is-touch="true" />
    </template>
    {{ text }}
  </AppPopover>
</template>
