<script setup lang="ts">
import { computed } from "vue";
import { t } from "@ledger/i18n";
import type { InstrumentSyncProgress } from "@ledger/types";
import { bar, root, text, textStack, track } from "./sync-progress-bar.css.ts";

const props = defineProps<{ progress: InstrumentSyncProgress | null }>();

const percent = computed(() => {
  if (!props.progress || props.progress.total <= 0) return 0;
  return Math.min(100, Math.round((props.progress.done / props.progress.total) * 100));
});
</script>

<template>
  <div
    v-if="progress"
    :class="root"
    data-testid="instrument-sync-progress"
    role="progressbar"
    :aria-valuemin="0"
    :aria-valuemax="progress.total"
    :aria-valuenow="progress.done"
    :aria-label="t('investments.sync.progressAriaLabel')"
  >
    <div :class="track">
      <div
        :class="bar"
        data-testid="instrument-sync-progress-bar"
        :style="{ width: percent + '%' }"
      />
    </div>
    <div :class="textStack">
      <span :class="text">
        {{ t("investments.sync.progress", { done: progress.done, total: progress.total }) }}
      </span>
    </div>
  </div>
</template>
