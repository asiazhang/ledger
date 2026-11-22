<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import { NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { t } from "@ledger/i18n";

const message = useMessage();

const currentLogLevel = ref<string>("info");
const savingLogLevel = ref(false);
const logLevelOptions = computed(() =>
  ["error", "warn", "info", "debug", "trace"].map((v) => ({
    label: t(`settings.logs.${v}`),
    value: v,
  })),
);

async function loadLogLevel() {
  try {
    const s = await api.getLogLevel();
    currentLogLevel.value = s.level;
  } catch (e) {
    message.error(t("settings.logs.loadFailed", { msg: errorMessage(e) }));
  }
}

async function handleLevelChange(level: string) {
  savingLogLevel.value = true;
  try {
    await api.setLogLevel(level);
    currentLogLevel.value = level;
  } catch (e) {
    message.error(t("settings.logs.saveFailed", { msg: errorMessage(e) }));
  } finally {
    savingLogLevel.value = false;
  }
}

onMounted(loadLogLevel);

async function openLogDir() {
  try {
    await api.openLogDir();
  } catch (e) {
    message.error(errorMessage(e));
  }
}
</script>

<template>
  <NCard :title="t('settings.logs.card')" size="small">
    <NSpace vertical :size="8">
      <NText>{{ t("settings.logs.label") }}</NText>
      <AppSelect
        :value="currentLogLevel"
        :options="logLevelOptions"
        :disabled="savingLogLevel"
        @update:value="handleLevelChange"
        :data-testid="'log-level-select'"
      />
      <NText depth="3">{{ t("settings.logs.hint") }}</NText>
      <NButton size="small" @click="openLogDir">{{ t("settings.logs.openLogDir") }}</NButton>
    </NSpace>
  </NCard>
</template>
