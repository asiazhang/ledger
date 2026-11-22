<script setup lang="ts">
import { t } from "@ledger/i18n";
import { root, text } from "./history-backfill-indicator.css.ts";
import { useHistoryBackfill } from "./useHistoryBackfill";

const { progress } = useHistoryBackfill();
</script>

<template>
  <div v-if="progress" :class="root" data-testid="history-backfill-indicator">
    <span :class="text">
      {{ t("investments.backfill.progress", { done: progress.done, total: progress.total }) }}
    </span>
  </div>
</template>
