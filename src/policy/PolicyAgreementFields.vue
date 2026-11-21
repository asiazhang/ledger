<script setup lang="ts">
import { ref } from "vue";
import { NFormItem, NInput, NInputNumber, NSpace } from "naive-ui";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { t } from "@ledger/i18n";
import { todayStr } from "@ledger/utils/date";
import { yuanToCents } from "@ledger/money";
import { useAppStore } from "@/stores/app";
import { useFormShared, usePinnedAccountOptions } from "@/composables/useFormShared";
import { scheduledRecurrenceOptions } from "@ledger/scheduled-plan-list";
import type { CreateScheduledInput, RecurrenceType } from "@ledger/types";

const app = useAppStore();
const { accountOptions: selectableAccountOptions, currencyOptions } = useFormShared();

const amountYuan = ref("");
const currencyCode = ref<string | null>(app.defaultCurrency);
const recurrenceType = ref<RecurrenceType>("yearly");
const recurrenceInterval = ref(1);
const accountId = ref<string | null>(null);
const accountOptions = usePinnedAccountOptions(accountId, selectableAccountOptions);
const startDate = ref<string | null>(todayStr());

const recurrenceOptions = scheduledRecurrenceOptions();

interface AgreementPrefill {
  currencyCode?: string | null;
  recurrenceType?: RecurrenceType;
  recurrenceInterval?: number;
  accountId?: string | null;
  startDate?: string | null;
}

function reset(prefill: AgreementPrefill = {}) {
  amountYuan.value = "";
  currencyCode.value = prefill.currencyCode ?? app.defaultCurrency;
  recurrenceType.value = prefill.recurrenceType ?? "yearly";
  recurrenceInterval.value = prefill.recurrenceInterval ?? 1;
  accountId.value = prefill.accountId ?? null;
  startDate.value = prefill.startDate ?? todayStr();
}

function validate(): string | null {
  if (!accountId.value) return t("policies.agreement.msg.accountRequired");
  const cents = yuanToCents(amountYuan.value);
  if (cents === null || cents <= 0) return t("policies.agreement.msg.amountInvalid");
  if (!startDate.value) return t("policies.agreement.msg.startRequired");
  return null;
}

function build(policyId: string, productName: string): CreateScheduledInput {
  return {
    kind: "subscription",
    account_id: accountId.value!,
    category_id: null,
    amount_cents: yuanToCents(amountYuan.value)!,
    currency_code: currencyCode.value ?? app.defaultCurrency,
    recurrence_type: recurrenceType.value,
    recurrence_interval: recurrenceInterval.value,
    recurrence_day: null,
    start_date: startDate.value!,
    note: productName || null,
    merchant_id: null,
    policy_id: policyId,
  };
}

defineExpose({ reset, validate, build });
</script>

<template>
  <!-- 布局对齐保单静态区：NSpace 提供行距 -->
  <NSpace vertical :size="12">
    <NFormItem :label="t('policies.agreement.amount')">
      <NInput
        v-model:value="amountYuan"
        :placeholder="t('policies.agreement.amountPlaceholder')"
        style="width: 160px"
        data-testid="policy-agreement-amount"
      />
      <AppSelect
        v-model:value="currencyCode"
        :options="currencyOptions"
        style="width: 130px; margin-left: 8px"
        data-testid="policy-agreement-currency"
      />
    </NFormItem>
    <NFormItem :label="t('policies.agreement.recurrence')">
      <NSpace :size="8" align="center" :wrap="false">
        <span>{{ t("scheduled.form.every") }}</span>
        <NInputNumber
          v-model:value="recurrenceInterval"
          :min="1"
          :precision="0"
          style="width: 90px"
          data-testid="policy-agreement-interval"
        />
        <AppSelect
          v-model:value="recurrenceType"
          :options="recurrenceOptions"
          style="width: 100px"
          data-testid="policy-agreement-recurrence"
        />
      </NSpace>
    </NFormItem>
    <NFormItem :label="t('policies.agreement.account')">
      <PinyinSelect
        v-model:value="accountId"
        :options="accountOptions"
        :placeholder="t('policies.agreement.accountPlaceholder')"
        style="width: 200px"
        data-testid="policy-agreement-account"
      />
    </NFormItem>
    <NFormItem :label="t('policies.agreement.startDate')">
      <AppDatePicker
        v-model:formatted-value="startDate"
        type="date"
        value-format="yyyy-MM-dd"
        style="width: 200px"
        data-testid="policy-agreement-start"
        clearable
      />
    </NFormItem>
  </NSpace>
</template>
