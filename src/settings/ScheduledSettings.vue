<script setup lang="ts">
import { NCard, NSpace, NSwitch, NText } from "naive-ui";
import { useAppStore } from "@/stores/app";
import { t } from "@ledger/i18n";

const store = useAppStore();
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.scheduled.title')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center" :size="12">
          <NSwitch
            :value="store.autoExecutionEnabled"
            @update:value="store.setAutoExecutionEnabled"
          />
          <NText>{{ t("settings.scheduled.switchLabel") }}</NText>
        </NSpace>
        <NText depth="3">
          {{ t("settings.scheduled.body1") }}
        </NText>
        <NText depth="3">
          {{ t("settings.scheduled.body2") }}
        </NText>
      </NSpace>
    </NCard>
  </NSpace>
</template>
