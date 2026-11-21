<script setup lang="ts">
import { computed, ref, watch, nextTick } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, NSwitch, NText, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import PolicyAgreementFields from "@/policy/PolicyAgreementFields.vue";
import PolicyAgreementSection from "@/policy/PolicyAgreementSection.vue";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { todayStr } from "@ledger/utils/date";
import { policyStatAmountText } from "@/policy/policy-stats";
import { yuanToCents, centsToYuan } from "@ledger/money";
import { useFormShared } from "@/composables/useFormShared";
import { resolveInsurerRef } from "@/policy/resolve-insurer";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { usePoliciesStore } from "@/policy/policies";
import { api } from "@ledger/api";
import type { Policy, PolicyInput } from "@ledger/types";

const props = defineProps<{
  show: boolean;
  editing: Policy | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const reference = useReferenceStore();
const app = useAppStore();
const policiesStore = usePoliciesStore();
const { currencyOptions } = useFormShared();

const insurerRef = ref<string | null>(null);
const policyNumber = ref("");
const productName = ref("");
const startDate = ref<string | null>(null);
const endDate = ref<string | null>(null);
const coverageYuan = ref("");
const coverageCurrency = ref<string | null>(null);
const note = ref("");

const withAgreement = ref(false);
const agreementFields = ref<InstanceType<typeof PolicyAgreementFields> | null>(null);

const insurerOptions = computed(() =>
  reference.insurers.map((i) => ({ label: i.name, value: i.id })),
);

const coverageFilled = computed(() => coverageYuan.value.trim() !== "");

const statsSummary = computed(() => {
  if (!props.editing) return null;
  return policiesStore.statsById.get(props.editing.id) ?? null;
});

const paidText = computed(() =>
  policyStatAmountText(statsSummary.value, (s) => s.total_paid_native_cents),
);

const inflowText = computed(() =>
  policyStatAmountText(statsSummary.value, (s) => s.total_inflow_native_cents),
);

const expiryText = computed(() => {
  const p = props.editing;
  if (!p) return "—";
  if (p.end_date === null) return t("policies.expiry.lifetime");
  const expired = statsSummary.value?.is_expired ?? p.end_date < todayStr();
  return expired ? t("policies.expiry.expired") : t("policies.expiry.active");
});

watch(coverageFilled, (filled) => {
  if (!filled) coverageCurrency.value = null;
});

watch(
  () => [props.show, props.editing] as const,
  () => {
    if (!props.show) return;
    const p = props.editing;
    insurerRef.value = p?.insurer_id ?? null;
    policyNumber.value = p?.policy_number ?? "";
    productName.value = p?.product_name ?? "";
    startDate.value = p?.start_date ?? todayStr();
    endDate.value = p?.end_date ?? null;
    coverageYuan.value =
      p?.coverage_amount_cents != null ? String(centsToYuan(p.coverage_amount_cents)) : "";
    coverageCurrency.value = p?.coverage_currency_code ?? app.defaultCurrency;
    note.value = p?.note ?? "";
    withAgreement.value = false;
    void nextTick(() => agreementFields.value?.reset({ startDate: startDate.value }));
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function resolveInsurer(): Promise<string> {
  return (await resolveInsurerRef(insurerRef.value)) ?? "";
}

async function save() {
  if (!insurerRef.value?.trim()) {
    message.warning(t("policies.form.msg.insurerRequired"));
    return;
  }
  if (!policyNumber.value.trim()) {
    message.warning(t("policies.form.msg.numberRequired"));
    return;
  }
  if (!productName.value.trim()) {
    message.warning(t("policies.form.msg.productRequired"));
    return;
  }
  if (!startDate.value) {
    message.warning(t("policies.form.msg.startRequired"));
    return;
  }
  if (endDate.value && endDate.value < startDate.value) {
    message.warning(t("policies.form.msg.endBeforeStart"));
    return;
  }
  let coverageCents: number | null = null;
  if (coverageFilled.value) {
    const cents = yuanToCents(coverageYuan.value);
    if (cents === null || cents <= 0) {
      message.warning(t("policies.form.msg.amountInvalid"));
      return;
    }
    if (!coverageCurrency.value) {
      message.warning(t("policies.form.msg.currencyRequired"));
      return;
    }
    coverageCents = cents;
  }

  let insurerId: string;
  try {
    insurerId = await resolveInsurer();
  } catch (e) {
    message.error(t("policies.form.msg.insurerFailed", { msg: errorMessage(e) }));
    return;
  }
  if (!insurerId) {
    message.warning(t("policies.form.msg.insurerRequired"));
    return;
  }

  if (withAgreement.value) {
    const agreementErr = agreementFields.value?.validate();
    if (agreementErr) {
      message.warning(agreementErr);
      return;
    }
  }

  const input: PolicyInput = {
    insurer_id: insurerId,
    policy_number: policyNumber.value.trim(),
    product_name: productName.value.trim(),
    start_date: startDate.value,
    end_date: endDate.value || null,
    coverage_amount_cents: coverageCents,
    coverage_currency_code: coverageCents !== null ? coverageCurrency.value : null,
    note: note.value.trim() || null,
  };
  try {
    if (props.editing) {
      await policiesStore.update(props.editing.id, input);
      message.success(t("policies.msg.saved"));
    } else {
      const policyId = await policiesStore.create(input);
      if (withAgreement.value && agreementFields.value) {
        await api.createScheduledTransaction(
          agreementFields.value.build(policyId, input.product_name),
        );
      }
      message.success(t("policies.msg.created"));
    }
    close();
  } catch (e) {
    message.error(t("policies.msg.saveFailed", { msg: errorMessage(e) }));
  }
}

defineExpose({ save });
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    card-size="md"
    :title="editing ? t('policies.form.titleEdit') : t('policies.form.titleCreate')"
    data-testid="policy-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，issue #699）：NFormItem 默认零行距 -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('policies.form.label.insurer')">
          <PinyinSelect
            v-model:value="insurerRef"
            :options="insurerOptions"
            tag
            :placeholder="t('policies.form.placeholder.insurer')"
            style="width: 280px"
            data-testid="policy-insurer"
          />
        </NFormItem>
        <NFormItem :label="t('policies.form.label.policyNumber')">
          <NInput
            v-model:value="policyNumber"
            :placeholder="t('policies.form.placeholder.policyNumber')"
            data-testid="policy-number"
          />
        </NFormItem>
        <NFormItem :label="t('policies.form.label.productName')">
          <NInput
            v-model:value="productName"
            :placeholder="t('policies.form.placeholder.productName')"
            data-testid="policy-product"
          />
        </NFormItem>
        <NFormItem :label="t('policies.form.label.period')">
          <NSpace :size="8" align="center" inline>
            <AppDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              :placeholder="t('policies.form.placeholder.startDate')"
              style="width: 165px"
              data-testid="policy-start-date"
              clearable
            />
            <span style="opacity: 0.6">~</span>
            <AppDatePicker
              v-model:formatted-value="endDate"
              type="date"
              value-format="yyyy-MM-dd"
              clearable
              :placeholder="t('policies.form.placeholder.endDate')"
              style="width: 165px"
              data-testid="policy-end-date"
            />
          </NSpace>
        </NFormItem>
        <NFormItem :label="t('policies.form.label.coverage')">
          <NInput
            v-model:value="coverageYuan"
            :placeholder="t('policies.form.placeholder.coverage')"
            style="width: 160px"
            data-testid="policy-coverage"
          />
          <AppSelect
            v-model:value="coverageCurrency"
            :options="currencyOptions"
            :disabled="!coverageFilled"
            :placeholder="t('policies.form.placeholder.coverageCurrency')"
            style="width: 120px"
            data-testid="policy-coverage-currency"
          />
        </NFormItem>
        <NFormItem :label="t('policies.form.label.note')">
          <NInput
            v-model:value="note"
            :placeholder="t('policies.form.placeholder.note')"
            data-testid="policy-note"
          />
        </NFormItem>

        <!-- 新建模式：缴费协议折叠开关（默认关 = 趸交/缴清纯档案）+ 字段组 -->
        <NFormItem v-if="!editing" :label="t('policies.agreement.sectionTitle')">
          <NSwitch v-model:value="withAgreement" data-testid="policy-agreement-toggle" />
        </NFormItem>
        <!-- 辅助说明统一段落式（spec #630 / #636）：不再内联 opacity 挤在开关旁 -->
        <NText v-if="!editing" depth="3" class="form-hint">
          {{ t("policies.agreement.toggleHint") }}
        </NText>
        <div v-if="!editing" v-show="withAgreement" data-testid="policy-agreement-fields">
          <PolicyAgreementFields ref="agreementFields" />
        </div>

        <!-- 编辑模式：保单视角统计（实时推导，issue #363）+ 协议历史与添加/改价 -->
        <div
          v-if="editing"
          data-testid="policy-stats-summary"
          style="display: flex; gap: 16px; font-size: 13px"
        >
          <span
            >{{ t("policies.stats.paid") }}：<strong>{{ paidText }}</strong></span
          >
          <span
            >{{ t("policies.stats.inflow") }}：<strong>{{ inflowText }}</strong></span
          >
          <span>
            {{ t("policies.stats.nextCharge") }}：<strong>{{
              statsSummary?.next_charge_date ?? "—"
            }}</strong>
          </span>
          <span
            >{{ t("policies.stats.expiry") }}：<strong>{{ expiryText }}</strong></span
          >
        </div>
        <NFormItem
          v-if="editing"
          :label="t('policies.agreement.sectionTitle')"
          :show-feedback="false"
        >
          <PolicyAgreementSection
            :policy="editing"
            style="width: 100%"
            data-testid="policy-agreement-section"
          />
        </NFormItem>

        <NSpace justify="end">
          <NButton @click="close">{{ t("policies.form.cancel") }}</NButton>
          <NButton type="primary" data-testid="policy-save" @click="save">
            {{ t("policies.form.save") }}
          </NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>

<style scoped>
.form-hint {
  display: block;
}
</style>
