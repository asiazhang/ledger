<script setup lang="ts">
import { computed } from "vue";
import { NCard, NSpace, NSwitch, NText } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import LogSettings from "@/settings/LogSettings.vue";
import { SETTINGS_CARD_STACK_CLASS } from "@/settings/settings-layout.css.ts";
import { useAppStore } from "@/stores/app";
import { t, type LocaleSetting } from "@ledger/i18n";

const store = useAppStore();

// 界面语言选项（issue #342 / ADR-0049）：具体语言用原生名（中文/English），
// 不随界面语言翻译；「跟随系统」走文案资源。
const languageOptions = computed<{ label: string; value: LocaleSetting }[]>(() => [
  { label: t("common.language.followSystem"), value: "system" },
  { label: t("common.language.zh"), value: "zh-CN" },
  { label: t("common.language.en"), value: "en-US" },
]);

// 本 Tab 收纳不归属任何业务域页签的应用级偏好（ADR-0022 修订，issue #930）：以下卡片为
// 轻量设备偏好；「日志」卡片（LogSettings，后端消费、随备份迁移）是应用级设置的扩展
// 收纳。本位币基准、展示币种已按领域归属迁入「币种」页签（issue #1664，
// 展示币种抽为 DisplayCurrencySettings）。
</script>

<template>
  <div :class="SETTINGS_CARD_STACK_CLASS">
    <NCard :title="t('settings.appearance.card')" size="small">
      <NSpace align="center" :size="12">
        <NText>{{ t("settings.appearance.darkMode") }}</NText>
        <NSwitch
          :value="store.theme === 'dark'"
          @update:value="(val: boolean) => store.setTheme(val ? 'dark' : 'light')"
        />
      </NSpace>
    </NCard>

    <NCard :title="t('common.language.label')" size="small">
      <AppSelect
        :value="store.localeSetting"
        :options="languageOptions"
        @update:value="(val: LocaleSetting) => store.setLocale(val)"
        style="max-width: 280px"
      />
    </NCard>

    <LogSettings />
  </div>
</template>
