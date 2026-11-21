<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, NText, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { yuanToCents } from "@ledger/money";
import { useFormShared } from "@/composables/useFormShared";
import { useAppStore } from "@/stores/app";
import { usePhysicalAssetsStore } from "@/physical-asset/physicalAssets";
import type { PhysicalAsset, PhysicalAssetValuationInput } from "@ledger/types";

const props = defineProps<{
  show: boolean;
  asset: PhysicalAsset | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const app = useAppStore();
const physicalAssetsStore = usePhysicalAssetsStore();
const { currencyOptions } = useFormShared();

const amountYuan = ref("");
const currency = ref<string | null>(null);
const valuationDate = ref<string | null>(null);

watch(
  () => [props.show, props.asset] as const,
  () => {
    if (!props.show) return;
    amountYuan.value = "";
    currency.value = props.asset?.current_valuation_currency_code ?? app.defaultCurrency;
    valuationDate.value = null;
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function save() {
  const asset = props.asset;
  if (!asset) return;
  const cents = yuanToCents(amountYuan.value);
  if (amountYuan.value.trim() === "" || cents === null) {
    message.warning(t("physicalAssets.valuation.msg.amountRequired"));
    return;
  }
  if (cents <= 0) {
    message.warning(t("physicalAssets.valuation.msg.amountInvalid"));
    return;
  }
  if (!currency.value) {
    message.warning(t("physicalAssets.valuation.msg.currencyRequired"));
    return;
  }
  const input: PhysicalAssetValuationInput = {
    amount_cents: cents,
    currency_code: currency.value,
    valuation_date: valuationDate.value || null,
  };
  try {
    await physicalAssetsStore.updateValuation(asset.id, input);
    message.success(t("physicalAssets.msg.valuationUpdated"));
    close();
  } catch (e) {
    message.error(t("physicalAssets.msg.saveFailed", { msg: errorMessage(e) }));
  }
}

defineExpose({ save });
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('physicalAssets.valuation.title')"
    card-size="sm"
    data-testid="physical-asset-valuation-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('physicalAssets.valuation.label.amount')">
          <NInput
            v-model:value="amountYuan"
            :placeholder="t('physicalAssets.valuation.placeholder.amount')"
            style="width: 160px"
            data-testid="physical-asset-valuation-amount"
          />
          <AppSelect
            v-model:value="currency"
            :options="currencyOptions"
            :placeholder="t('physicalAssets.form.placeholder.currency')"
            style="width: 120px"
            data-testid="physical-asset-valuation-currency-select"
          />
        </NFormItem>
        <NFormItem :label="t('physicalAssets.valuation.label.date')">
          <AppDatePicker
            v-model:formatted-value="valuationDate"
            type="date"
            value-format="yyyy-MM-dd"
            clearable
            :placeholder="t('physicalAssets.valuation.placeholder.date')"
            style="width: 160px"
            data-testid="physical-asset-valuation-date"
          />
        </NFormItem>
        <!-- 辅助说明统一段落式（spec #630 / #635）：不再内联挤在日期表单项旁 -->
        <NText depth="3" class="form-hint">
          {{ t("physicalAssets.valuation.dateHint") }}
        </NText>

        <NSpace justify="end">
          <NButton @click="close">{{ t("physicalAssets.form.cancel") }}</NButton>
          <NButton type="primary" data-testid="physical-asset-valuation-save" @click="save">
            {{ t("physicalAssets.form.save") }}
          </NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>

<style scoped>
.form-hint {
  display: block;
  margin: 8px 0 12px;
}
</style>
