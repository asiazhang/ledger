<script setup lang="ts">
import { computed } from "vue";
import { NCard, NSpace, NSwitch, NText } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import LogSettings from "@/settings/LogSettings.vue";
import UpdateSettings from "@/settings/UpdateSettings.vue";
import { SETTINGS_CARD_STACK_CLASS } from "@/settings/settings-layout.css.ts";
import { useAppStore } from "@/stores/app";
import { t, type LocaleSetting } from "@ledger/i18n";

const store = useAppStore();

const languageOptions = computed<{ label: string; value: LocaleSetting }[]>(() => [
  { label: t("common.language.followSystem"), value: "system" },
  { label: t("common.language.zh"), value: "zh-CN" },
  { label: t("common.language.en"), value: "en-US" },
]);
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
    <UpdateSettings />
  </div>
</template>
