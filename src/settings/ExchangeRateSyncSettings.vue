<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import { t } from "@ledger/i18n";
import { useFxSync } from "@/settings/useFxSync";

const message = useMessage();

const { syncing, syncError, report, stageText, sync } = useFxSync();

const successText = computed(() =>
  report.value
    ? t("settings.fxSync.ok", {
        points: report.value.persist.points,
        earliest: report.value.persist.earliest ?? "-",
        latest: report.value.persist.latest ?? "-",
      })
    : "",
);

async function syncNow(): Promise<void> {
  const result = await sync();
  if (result) {
    message.success(successText.value);
  }
}
</script>

<template>
  <NCard :title="t('settings.fxSync.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">{{ t("settings.fxSync.hint") }}</NText>

      <!-- 失败原因优先于上次成功报告（错误位就地重显，错误文案按码本地化可分辨） -->
      <NAlert v-if="syncError" type="error" :show-icon="true">
        {{ syncError }}
      </NAlert>
      <NAlert v-else-if="report" type="success" :show-icon="true">
        {{ successText }}
      </NAlert>

      <NButton size="small" type="primary" :loading="syncing" :disabled="syncing" @click="syncNow">
        {{ t("settings.fxSync.action") }}
      </NButton>

      <!-- 阶段文字（issue #1762）：按钮下方一行真实文本，随真实阶段流转 -->
      <NText v-if="stageText" depth="3">{{ stageText }}</NText>
    </NSpace>
  </NCard>
</template>
