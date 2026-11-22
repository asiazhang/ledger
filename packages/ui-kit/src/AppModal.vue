<script setup lang="ts">
import { computed } from "vue";
import { NModal } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";
import { useWindowTier } from "@ledger/window-tier";
import { MOBILE_CARD_CLASS } from "./app-modal.css.ts";

type CardSize = "sm" | "md" | "lg";

const CARD_WIDTH_PX: Record<CardSize, number> = { sm: 420, md: 480, lg: 560 };

const MOBILE_CARD_STYLE = {
  width: "calc(100vw - 32px)",
  height: "calc(100dvh - 32px - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px))",
};

const props = withDefaults(
  defineProps<{ maskClosable?: boolean; cardSize?: CardSize; bordered?: boolean }>(),
  { maskClosable: false, bordered: false },
);

const tier = useWindowTier();
const isMobileTier = computed(() => tier.value === "mobile");

const cardStyle = computed(() => {
  if (isMobileTier.value) return MOBILE_CARD_STYLE;
  return props.cardSize === undefined ? undefined : { width: `${CARD_WIDTH_PX[props.cardSize]}px` };
});

const { onUpdateShow, resolvedShow } = useOverlayReporting("modal");
</script>

<template>
  <NModal
    :class="isMobileTier ? MOBILE_CARD_CLASS : undefined"
    :mask-closable="maskClosable"
    :bordered="bordered"
    :style="cardStyle"
    :show="resolvedShow"
    @update:show="onUpdateShow"
  >
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NModal>
</template>
