<script setup lang="ts">
import { computed } from "vue";
import { formatRate } from "@ledger/money";
import { t } from "@ledger/i18n";
import ConceptTipHost from "@/investment/ConceptTipHost.vue";
import { marker, trigger } from "./mwr-rate-cell.css.ts";

const props = defineProps<{
  rate: number;
  color: string;
  annualized?: boolean;
}>();

const text = computed(() => formatRate(props.rate));
const tip = computed(() => t("investments.concepts.mwrCumulativeTip"));
const marked = computed(() => props.annualized === false);

const ariaLabel = computed(() => (marked.value ? `${text.value}，${tip.value}` : text.value));
</script>

<template>
  <ConceptTipHost v-if="marked" :text="tip">
    <template #default="{ isTouch }">
      <span
        data-mwr-marker
        :class="isTouch ? [trigger, 'touch-hit-area'] : trigger"
        :role="isTouch ? 'button' : undefined"
        :aria-label="isTouch ? ariaLabel : undefined"
        :style="{ color }"
      >
        {{ text }}<sup :class="marker">*</sup>
      </span>
    </template>
  </ConceptTipHost>
  <span v-else :style="{ color }">{{ text }}</span>
</template>
