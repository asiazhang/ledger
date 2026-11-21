<script setup lang="ts">
import { computed } from "vue";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import { useInputMode } from "@/composables/useInputMode";

const props = defineProps<{
  text: string;
  color: string;
}>();

const inputMode = useInputMode();
const isTouch = computed(() => inputMode.value === "touch");
const cellStyle = computed(() => ({ color: props.color }));
</script>

<template>
  <AppPopover v-if="isTouch" trigger="click" placement="top">
    <template #trigger>
      <span
        class="amount-cell touch-hit-area"
        :style="cellStyle"
        role="button"
        :aria-label="text"
        >{{ text }}</span
      >
    </template>
    <span class="amount-cell-full">{{ text }}</span>
  </AppPopover>
  <span v-else class="amount-cell" :style="cellStyle">{{ text }}</span>
</template>

<style scoped>
.amount-cell-full {
  font-variant-numeric: tabular-nums;
}
</style>
