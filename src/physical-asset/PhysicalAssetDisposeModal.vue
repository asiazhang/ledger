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
import type { PhysicalAsset, PhysicalAssetDisposeInput } from "@ledger/types";

const props = defineProps<{
  show: boolean;
  asset: PhysicalAsset | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const app = useAppStore();
const physicalAssetsStore = usePhysicalAssetsStore();
const { currencyOptions } = useFormShared();

const disposalDate = ref<string | null>(null);
const priceYuan = ref("");
const currency = ref<string | null>(null);

watch(
  () => [props.show, props.asset] as const,
  () => {
    if (!props.show) return;
    disposalDate.value = null;
    priceYuan.value = "";
    currency.value = props.asset?.current_valuation_currency_code ?? app.defaultCurrency;
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function save() {
  const asset = props.asset;
  if (!asset) return;
  if (!disposalDate.value) {
    message.warning(t("physicalAssets.dispose.msg.dateRequired"));
    return;
  }
  let priceCents: number | null = null;
  if (priceYuan.value.trim() !== "") {
    const cents = yuanToCents(priceYuan.value);
    if (cents === null || cents <= 0) {
      message.warning(t("physicalAssets.dispose.msg.priceInvalid"));
      return;
    }
    priceCents = cents;
    if (!currency.value) {
      message.warning(t("physicalAssets.dispose.msg.currencyRequired"));
      return;
    }
  }
  const input: PhysicalAssetDisposeInput = {
    disposal_date: disposalDate.value,
    disposal_price_cents: priceCents,
    disposal_currency_code: priceCents !== null ? currency.value : null,
  };
  try {
    await physicalAssetsStore.dispose(asset.id, input);
    message.success(t("physicalAssets.msg.disposed"));
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
    :title="t('physicalAssets.dispose.title')"
    card-size="sm"
    data-testid="physical-asset-dispose-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('physicalAssets.dispose.label.date')">
          <AppDatePicker
            v-model:formatted-value="disposalDate"
            type="date"
            value-format="yyyy-MM-dd"
            :placeholder="t('physicalAssets.dispose.placeholder.date')"
            style="width: 160px"
            data-testid="physical-asset-dispose-date"
          />
        </NFormItem>
        <NFormItem :label="t('physicalAssets.dispose.label.price')">
          <NInput
            v-model:value="priceYuan"
            :placeholder="t('physicalAssets.dispose.placeholder.price')"
            style="width: 160px"
            data-testid="physical-asset-dispose-price"
          />
          <AppSelect
            v-model:value="currency"
            :options="currencyOptions"
            :placeholder="t('physicalAssets.form.placeholder.currency')"
            style="width: 120px"
            data-testid="physical-asset-dispose-currency-select"
          />
        </NFormItem>
        <!-- 辅助说明统一段落式（spec #630 / #635）：废除空 label 表单项 hack -->
        <NText depth="3" class="form-hint">
          {{ t("physicalAssets.dispose.dateHint") }}
        </NText>

        <NSpace justify="end">
          <NButton @click="close">{{ t("physicalAssets.form.cancel") }}</NButton>
          <NButton type="primary" data-testid="physical-asset-dispose-save" @click="save">
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
