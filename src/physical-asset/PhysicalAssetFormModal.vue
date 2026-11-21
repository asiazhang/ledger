<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { yuanToCents, centsToYuan } from "@ledger/money";
import { useFormShared } from "@/composables/useFormShared";
import { useAppStore } from "@/stores/app";
import { usePhysicalAssetsStore } from "@/physical-asset/physicalAssets";
import type { PhysicalAsset, PhysicalAssetInput, PhysicalAssetUpdateInput } from "@ledger/types";

const props = defineProps<{
  show: boolean;
  editing: PhysicalAsset | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const app = useAppStore();
const physicalAssetsStore = usePhysicalAssetsStore();
const { currencyOptions } = useFormShared();

const name = ref("");
const valuationYuan = ref("");
const valuationCurrency = ref<string | null>(null);
const purchaseDate = ref<string | null>(null);
const purchaseYuan = ref("");
const purchaseCurrency = ref<string | null>(null);

const valuationFilled = computed(() => valuationYuan.value.trim() !== "");
const purchaseFilled = computed(() => purchaseYuan.value.trim() !== "");

watch(
  () => [props.show, props.editing] as const,
  () => {
    if (!props.show) return;
    const p = props.editing;
    name.value = p?.name ?? "";
    valuationYuan.value = "";
    valuationCurrency.value = app.defaultCurrency;
    purchaseDate.value = p?.purchase_date ?? null;
    purchaseYuan.value =
      p?.purchase_price_cents != null ? String(centsToYuan(p.purchase_price_cents)) : "";
    purchaseCurrency.value = p?.purchase_currency_code ?? app.defaultCurrency;
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function save() {
  if (!name.value.trim()) {
    message.warning(t("physicalAssets.form.msg.nameRequired"));
    return;
  }
  let purchaseCents: number | null = null;
  if (purchaseFilled.value) {
    const cents = yuanToCents(purchaseYuan.value);
    if (cents === null || cents <= 0) {
      message.warning(t("physicalAssets.form.msg.purchaseInvalid"));
      return;
    }
    if (!purchaseCurrency.value) {
      message.warning(t("physicalAssets.form.msg.purchaseCurrencyRequired"));
      return;
    }
    purchaseCents = cents;
  }

  try {
    if (props.editing) {
      const input: PhysicalAssetUpdateInput = {
        name: name.value.trim(),
        purchase_date: purchaseDate.value || null,
        purchase_price_cents: purchaseCents,
        purchase_currency_code: purchaseCents !== null ? purchaseCurrency.value : null,
      };
      await physicalAssetsStore.update(props.editing.id, input);
      message.success(t("physicalAssets.msg.updated"));
    } else {
      if (!valuationFilled.value) {
        message.warning(t("physicalAssets.form.msg.valuationRequired"));
        return;
      }
      const valuationCents = yuanToCents(valuationYuan.value);
      if (valuationCents === null || valuationCents <= 0) {
        message.warning(t("physicalAssets.form.msg.valuationInvalid"));
        return;
      }
      if (!valuationCurrency.value) {
        message.warning(t("physicalAssets.form.msg.valuationCurrencyRequired"));
        return;
      }
      const input: PhysicalAssetInput = {
        name: name.value.trim(),
        purchase_date: purchaseDate.value || null,
        purchase_price_cents: purchaseCents,
        purchase_currency_code: purchaseCents !== null ? purchaseCurrency.value : null,
        initial_valuation_cents: valuationCents,
        initial_valuation_currency_code: valuationCurrency.value,
        initial_valuation_date: null,
      };
      await physicalAssetsStore.create(input);
      message.success(t("physicalAssets.msg.created"));
    }
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
    :title="editing ? t('physicalAssets.form.titleEdit') : t('physicalAssets.form.title')"
    card-size="md"
    data-testid="physical-asset-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，issue #699）：NFormItem 默认零行距 -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('physicalAssets.form.label.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('physicalAssets.form.placeholder.name')"
            data-testid="physical-asset-name"
          />
        </NFormItem>
        <!-- 编辑模式无估值字段：估值只能经「更新估值」变更（历史只追加，T2） -->
        <NFormItem v-if="!editing" :label="t('physicalAssets.form.label.valuation')">
          <NInput
            v-model:value="valuationYuan"
            :placeholder="t('physicalAssets.form.placeholder.valuation')"
            style="width: 160px"
            data-testid="physical-asset-valuation"
          />
          <AppSelect
            v-model:value="valuationCurrency"
            :options="currencyOptions"
            :placeholder="t('physicalAssets.form.placeholder.currency')"
            style="width: 120px"
            data-testid="physical-asset-valuation-currency"
          />
        </NFormItem>
        <NFormItem :label="t('physicalAssets.form.label.purchaseDate')">
          <AppDatePicker
            v-model:formatted-value="purchaseDate"
            type="date"
            value-format="yyyy-MM-dd"
            clearable
            :placeholder="t('physicalAssets.form.placeholder.purchaseDate')"
            style="width: 160px"
            data-testid="physical-asset-purchase-date"
          />
        </NFormItem>
        <NFormItem :label="t('physicalAssets.form.label.purchasePrice')">
          <NInput
            v-model:value="purchaseYuan"
            :placeholder="t('physicalAssets.form.placeholder.purchasePrice')"
            style="width: 160px"
            data-testid="physical-asset-purchase-price"
          />
          <AppSelect
            v-model:value="purchaseCurrency"
            :options="currencyOptions"
            :disabled="!purchaseFilled"
            :placeholder="t('physicalAssets.form.placeholder.currency')"
            style="width: 120px"
            data-testid="physical-asset-purchase-currency"
          />
        </NFormItem>

        <NSpace justify="end">
          <NButton @click="close">{{ t("physicalAssets.form.cancel") }}</NButton>
          <NButton type="primary" data-testid="physical-asset-save" @click="save">
            {{ t("physicalAssets.form.save") }}
          </NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>
