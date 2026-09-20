<script setup lang="ts">
import { computed, ref } from "vue";
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useLoadable } from "@ledger/loadable";
import type { ExchangeRateSyncReport } from "@ledger/types";

// 汇率同步卡片（issue #1545）：设置页「同步汇率」手动入口——一键触发一次汇率
// 同步，进行中不可重复触发（loading 禁用），结果就地呈现（覆盖区间 / 条数）。
// 同步语义全部在命令层（ECB 增量取数 + 幂等落库，返回报告），本组件只触发命令
// 与呈现报告；失败原因码化三态（数据源不可达 / 该来源无数据 / 报文异常）经
// errorMessage 按码本地化后可分辨——toast 走 Loadable 默认策略，错误位就地重显。
// 纯就地反馈、无弹层，不涉 Overlay Suppression 登记。

const message = useMessage();

const report = ref<ExchangeRateSyncReport | null>(null);

const syncLoad = useLoadable(async () => {
  const result = await api.syncExchangeRates();
  report.value = result;
  return result;
});
const syncing = syncLoad.loading;
const syncError = syncLoad.error;

// 成功文案单一拼装点（评审收口）：toast 与成功告警位同文案（同一键，避免双源
// 漂移，#513 先例），插值只在这里发生一次。
const successText = computed(() =>
  report.value
    ? t("settings.fxSync.ok", {
        points: report.value.points,
        earliest: report.value.earliest,
        latest: report.value.latest,
      })
    : "",
);

/** 触发一次同步：成功 toast 与成功告警位同文案。 */
async function syncNow(): Promise<void> {
  const result = await syncLoad.run();
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
    </NSpace>
  </NCard>
</template>
