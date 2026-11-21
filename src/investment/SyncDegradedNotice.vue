<script setup lang="ts">
import { NText } from "naive-ui";
import { t } from "@ledger/i18n";

defineProps<{ degraded: boolean }>();
</script>

<template>
  <NText v-if="degraded" type="warning" data-testid="instrument-sync-degraded">
    {{ t("investments.sync.degraded") }}
  </NText>
</template>
