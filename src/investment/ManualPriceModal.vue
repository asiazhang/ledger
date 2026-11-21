<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInputNumber, NSpace, NText } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { errorMessage as extractErrorMessage } from "@ledger/utils/errors";
import { formatPrice, yuanToPrice } from "@ledger/money";
import { todayStr } from "@ledger/utils/date";

const props = defineProps<{ show: boolean; instrument: { id: string; symbol: string } | null }>();
const emit = defineEmits<{
  "update:show": [value: boolean];
  quoted: [message: string];
}>();

const date = ref(todayStr());
const price = ref<number | null>(null);
const submitting = ref(false);
const error = ref<string | null>(null);

const canSubmit = computed(
  () => price.value !== null && price.value > 0 && !!date.value && !submitting.value,
);

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    date.value = todayStr();
    price.value = null;
    error.value = null;
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function submit() {
  if (!canSubmit.value || !props.instrument) return;
  const priceCents = yuanToPrice(price.value!);
  if (priceCents === null || priceCents <= 0) {
    error.value = t("investments.manualPrice.priceMustBePositive");
    return;
  }
  submitting.value = true;
  error.value = null;
  try {
    const result = await api.recordManualPrice({
      instrument_id: props.instrument.id,
      date: date.value,
      price_cents: priceCents,
    });
    const message = result.current_price_written
      ? t("investments.manualPrice.success", {
          symbol: props.instrument.symbol,
          price: formatPrice(priceCents),
        })
      : t("investments.manualPrice.historyOnly", { symbol: props.instrument.symbol });
    emit("quoted", message);
    close();
  } catch (e) {
    error.value = extractErrorMessage(e);
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('investments.manualPrice.title', { symbol: instrument?.symbol ?? '' })"
    card-size="md"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t("investments.manualPrice.intro") }}
      </NText>
      <NForm label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，不得裸排（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('investments.manualPrice.dateLabel')" required>
            <AppDatePicker
              v-model:formatted-value="date"
              type="date"
              value-format="yyyy-MM-dd"
              :disabled="submitting"
              style="width: 200px"
              data-testid="manual-quote-date"
            />
          </NFormItem>
          <NFormItem :label="t('investments.manualPrice.priceLabel')" required>
            <NInputNumber
              v-model:value="price"
              :min="0"
              :precision="4"
              :placeholder="t('investments.manualPrice.pricePlaceholder')"
              :disabled="submitting"
              style="width: 200px"
              data-testid="manual-quote-price"
            />
          </NFormItem>
        </NSpace>
      </NForm>
      <NText v-if="error" type="error" data-testid="manual-quote-error">
        {{ error }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton data-testid="cancel-manual-quote" :disabled="submitting" @click="close">
          {{ t("investments.manualPrice.cancel") }}
        </NButton>
        <NButton
          type="primary"
          data-testid="submit-manual-quote"
          :loading="submitting"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ t("investments.manualPrice.submit") }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
