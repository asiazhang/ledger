<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, NText } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { errorCodeOf, errorMessage as extractErrorMessage } from "@ledger/utils/errors";
import { formatPrice } from "@ledger/money";
import type {
  AddInstrumentChannel,
  AddStockInstrumentResult,
  AddFundResult,
  InstrumentType,
} from "@ledger/types";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{
  "update:show": [value: boolean];
  added: [message: string];
}>();

const reference = useReferenceStore();

const CHANNEL_OPTIONS = computed(() => [
  { label: t("investments.market.sh"), value: "sh" as AddInstrumentChannel },
  { label: t("investments.market.sz"), value: "sz" as AddInstrumentChannel },
  { label: t("investments.market.hk"), value: "hk" as AddInstrumentChannel },
  { label: t("investments.addInstrument.channelUs"), value: "us" as AddInstrumentChannel },
  { label: t("investments.addInstrument.channelFund"), value: "fund" as AddInstrumentChannel },
  { label: t("investments.addInstrument.channelCustom"), value: "custom" as AddInstrumentChannel },
]);

const TYPE_OPTIONS = computed(() => [
  { label: t("investments.type.bond"), value: "bond" as InstrumentType },
  { label: t("investments.type.etf"), value: "etf" as InstrumentType },
  { label: t("investments.type.other"), value: "other" as InstrumentType },
]);

const market = ref<AddInstrumentChannel | null>(null);
const code = ref("");
const querying = ref(false);
const customName = ref("");
const customType = ref<InstrumentType | null>(null);
const customCurrency = ref("CNY");
const creating = ref(false);
const error = ref<string | null>(null);
const notFoundGuidance = ref(false);

const isCustom = computed(() => market.value === "custom");

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} · ${c.name}`, value: c.code })),
);

const codePlaceholder = computed(() => {
  if (isCustom.value) return t("investments.addInstrument.codePlaceholderCustom");
  return market.value === "fund"
    ? t("investments.addInstrument.codePlaceholderFund")
    : t("investments.addInstrument.codePlaceholderStock");
});

const codeValid = computed(() => {
  const trimmed = code.value.trim();
  if (trimmed === "") return false;
  if (market.value === "fund") return /^\d{6}$/.test(trimmed);
  return true;
});

const canQuery = computed(() => market.value !== null && codeValid.value && !querying.value);
const canCreate = computed(
  () =>
    isCustom.value &&
    codeValid.value &&
    customName.value.trim() !== "" &&
    customType.value !== null &&
    !creating.value,
);

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    market.value = null;
    code.value = "";
    customName.value = "";
    customType.value = null;
    customCurrency.value = "CNY";
    querying.value = false;
    creating.value = false;
    error.value = null;
    notFoundGuidance.value = false;
  },
  { immediate: true },
);

watch(market, () => {
  error.value = null;
  notFoundGuidance.value = false;
});

function close() {
  emit("update:show", false);
}

function stockHitMessage(result: AddStockInstrumentResult): string {
  const price =
    result.price_cents !== null
      ? t("investments.addInstrument.price", { price: formatPrice(result.price_cents) })
      : t("investments.addInstrument.priceMissing");
  return t("investments.addInstrument.successStock", {
    name: result.name,
    symbol: result.symbol,
    typeLabel: t(`investments.type.${result.type}`),
    price,
  });
}

function fundHitMessage(result: AddFundResult): string {
  const nav =
    result.nav_cents !== null && result.nav_date !== null
      ? t("investments.addInstrument.nav", {
          price: formatPrice(result.nav_cents),
          date: result.nav_date,
        })
      : t("investments.addInstrument.navMissing");
  return t("investments.addInstrument.successFund", {
    name: result.name,
    symbol: result.symbol,
    fundClass: result.fund_class ? ` · ${result.fund_class}` : "",
    nav,
  });
}

async function submitQuery() {
  if (!canQuery.value) return;
  querying.value = true;
  error.value = null;
  try {
    const message =
      market.value === "fund"
        ? fundHitMessage(await api.addFundByCode(code.value.trim()))
        : stockHitMessage(await api.addInstrumentByCode(market.value!, code.value.trim()));
    emit("added", message);
    close();
  } catch (e) {
    notFoundGuidance.value = market.value !== "fund" && errorCodeOf(e) === "sync.stock-not-found";
    error.value = extractErrorMessage(e);
  } finally {
    querying.value = false;
  }
}

async function submitCustom() {
  if (!canCreate.value) return;
  creating.value = true;
  error.value = null;
  try {
    const input = {
      symbol: code.value.trim(),
      type: customType.value!,
      name: customName.value.trim(),
      currency_code: customCurrency.value,
      market: null,
    };
    await api.createInstrument(input);
    emit(
      "added",
      t("investments.addInstrument.customSuccess", { name: input.name, symbol: input.symbol }),
    );
    close();
  } catch (e) {
    error.value = extractErrorMessage(e);
  } finally {
    creating.value = false;
  }
}

const primaryLabel = computed(() =>
  isCustom.value ? t("investments.addInstrument.create") : t("investments.addInstrument.query"),
);
const primaryLoading = computed(() => (isCustom.value ? creating.value : querying.value));
const primaryDisabled = computed(() => (isCustom.value ? !canCreate.value : !canQuery.value));

function submitPrimary() {
  if (isCustom.value) void submitCustom();
  else void submitQuery();
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('investments.addInstrument.title')"
    card-size="md"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t("investments.addInstrument.intro") }}
      </NText>
      <NForm label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，不得裸排（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('investments.addInstrument.marketLabel')" required>
            <AppSelect
              v-model:value="market"
              :options="CHANNEL_OPTIONS"
              :placeholder="t('investments.addInstrument.marketPlaceholder')"
              :disabled="querying || creating"
              data-testid="add-instrument-market"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem :label="t('investments.addInstrument.codeLabel')" required>
            <NInput
              v-model:value="code"
              :placeholder="codePlaceholder"
              :maxlength="32"
              :disabled="querying || creating"
              data-testid="add-instrument-code"
              @keyup.enter="submitPrimary"
            />
          </NFormItem>
          <template v-if="isCustom">
            <NFormItem :label="t('investments.addInstrument.nameLabel')" required>
              <NInput
                v-model:value="customName"
                :placeholder="t('investments.addInstrument.namePlaceholder')"
                :maxlength="64"
                :disabled="creating"
                data-testid="add-instrument-name"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.typeLabel')" required>
              <AppSelect
                v-model:value="customType"
                :options="TYPE_OPTIONS"
                :placeholder="t('investments.addInstrument.typePlaceholder')"
                :disabled="creating"
                data-testid="add-instrument-type"
                style="width: 100%"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.currencyLabel')">
              <AppSelect
                v-model:value="customCurrency"
                :options="currencyOptions"
                filterable
                :disabled="creating"
                data-testid="add-instrument-currency"
                style="width: 100%"
              />
            </NFormItem>
          </template>
        </NSpace>
      </NForm>
      <NText v-if="isCustom" depth="3" data-testid="add-instrument-custom-hint">
        {{ t("investments.addInstrument.customHint") }}
      </NText>
      <NText v-if="error" type="error" data-testid="add-instrument-error">
        {{ error }}
      </NText>
      <NText v-if="notFoundGuidance" depth="3" data-testid="add-instrument-not-found-hint">
        {{ t("investments.addInstrument.notFoundHint") }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton
          data-testid="cancel-add-instrument"
          :disabled="querying || creating"
          @click="close"
        >
          {{ t("investments.addInstrument.cancel") }}
        </NButton>
        <NButton
          type="primary"
          data-testid="submit-add-instrument"
          :loading="primaryLoading"
          :disabled="primaryDisabled"
          @click="submitPrimary"
        >
          {{ primaryLabel }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
