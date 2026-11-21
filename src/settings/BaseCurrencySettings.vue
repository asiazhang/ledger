<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NCard, NSpace, NText, useMessage } from "naive-ui";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import { useReferenceStore } from "@/stores/reference";
import { t } from "@ledger/i18n";

const reference = useReferenceStore();
const message = useMessage();

const baseCurrency = ref<string>("CNY");

onMounted(async () => {
  try {
    baseCurrency.value = (await api.getBaseCurrency()).code;
  } catch (e) {
    message.error(t("settings.appearance.baseCurrencyLoadFailed", { msg: errorMessage(e) }));
  }
});

async function setBaseCurrency(code: string) {
  try {
    baseCurrency.value = (await api.setBaseCurrency(code)).code;
  } catch (e) {
    message.error(t("settings.appearance.baseCurrencySaveFailed", { msg: errorMessage(e) }));
  }
}
</script>

<template>
  <NCard :title="t('settings.appearance.baseCurrency')" size="small">
    <NSpace vertical :size="8">
      <AppSelect
        :value="baseCurrency"
        :options="
          reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code }))
        "
        @update:value="(val: string) => setBaseCurrency(val)"
        style="max-width: 280px"
      />
      <NText depth="3" style="font-size: 12px">
        {{ t("settings.appearance.baseCurrencyHint") }}
      </NText>
    </NSpace>
  </NCard>
</template>
