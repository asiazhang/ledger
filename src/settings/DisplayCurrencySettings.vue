<script setup lang="ts">
import { computed } from "vue";
import { NCard, NSpace, NText } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { t } from "@ledger/i18n";

// 展示币种卡片（issue #858 拆分为设备级偏好；issue #1664 自「通用」迁入「币种」
// 页签）：轻量设置项——localStorage 权威、不触后端、不随 Backup/Restore 迁移；
// 作用域是本机表单预选与金额符号提示，不参与账本折算（折算语义在账本本位币
// 基准，与本卡同页签相邻互为对照，各带作用域提示）。
const store = useAppStore();
const reference = useReferenceStore();

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
);
</script>

<template>
  <NCard :title="t('settings.appearance.displayCurrency')" size="small">
    <NSpace vertical :size="8">
      <AppSelect
        :value="store.defaultCurrency"
        :options="currencyOptions"
        @update:value="(val: string) => store.setDefaultCurrency(val)"
        style="max-width: 280px"
      />
      <NText depth="3" style="font-size: 12px">
        {{ t("settings.appearance.displayCurrencyHint") }}
      </NText>
    </NSpace>
  </NCard>
</template>
