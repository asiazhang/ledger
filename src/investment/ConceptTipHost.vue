<script setup lang="ts">
import { computed } from "vue";
import { NTooltip } from "naive-ui";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import { useInputMode } from "@/composables/useInputMode";

defineProps<{
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
