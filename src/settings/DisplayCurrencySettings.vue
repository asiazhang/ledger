<script setup lang="ts">
import { computed } from "vue";
import { NCard, NSpace, NText } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { t } from "@ledger/i18n";

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
