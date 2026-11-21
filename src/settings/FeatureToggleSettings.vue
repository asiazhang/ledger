<script setup lang="ts">
import { NCard, NSpace, NSwitch, NText } from "naive-ui";
import { CLOSABLE_FEATURES, useFeatureToggleStore } from "@/settings/feature-toggles";
import { t } from "@ledger/i18n";
import { viewLabel } from "@ledger/i18n/view-label";

const store = useFeatureToggleStore();
</script>

<template>
  <NCard :title="t('settings.features.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3" style="font-size: 12px">
        {{ t("settings.features.intro") }}
      </NText>
      <NSpace
        v-for="id in CLOSABLE_FEATURES"
        :key="id"
        :data-testid="`feature-toggle-${id}`"
        justify="space-between"
        align="center"
        :size="16"
      >
        <NSpace vertical :size="2">
          <NText>{{ viewLabel(id) }}</NText>
          <NText depth="3" style="font-size: 12px">
            {{ t(`settings.features.descriptions.${id}`) }}
          </NText>
        </NSpace>
        <NSwitch
          :value="!store.isFeatureClosed(id)"
          @update:value="(val: boolean) => store.setFeatureClosed(id, !val)"
        />
      </NSpace>
    </NSpace>
  </NCard>
</template>
