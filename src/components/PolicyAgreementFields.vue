<script setup lang="ts">
import { ref } from 'vue'
import { NFormItem, NInput, NInputNumber, NSpace } from 'naive-ui'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { t } from '@/i18n'
import { todayStr } from '@/utils/date'
import { yuanToCents } from '@/utils/money'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import { scheduledRecurrenceOptions } from '@/composables/useScheduledPlanList'
import type { CreateScheduledInput, RecurrenceType } from '@/types'

/**
 * 保单缴费协议字段组（issue #362 / ADR-0051 决策 2）：频率/每期金额（+币种）/
 * 扣款账户/起始日——新建（保单弹窗折叠区）与编辑（协议区添加/改价预填）共用的
 * 表单字段组件。草稿状态内聚在组件内，调用方经 ref 驱动：
 * `reset`（复位/预填）→ `validate`（校验，返回首个错误文案）→
 * `build`（组装 CreateScheduledInput，校验通过后调用）。
 *
 * 组装约定（ADR-0051 决策 7）：merchant_id 取保单保司（保险公司即保费流水的
 * 付款对象）；备注带险种名称（协议与保单分离，订阅清单靠备注可读）。
 */
const app = useAppStore()
const { accountOptions, currencyOptions } = useFormShared()

const amountYuan = ref('')
const currencyCode = ref<string | null>(app.defaultCurrency)
const recurrenceType = ref<RecurrenceType>('yearly')
const recurrenceInterval = ref(1)
const accountId = ref<string | null>(null)
const startDate = ref<string | null>(todayStr())

const recurrenceOptions = scheduledRecurrenceOptions()

/** 预填项（改价场景由调用方传旧段值；缺省字段回落默认态）。 */
interface AgreementPrefill {
  currencyCode?: string | null
  recurrenceType?: RecurrenceType
  recurrenceInterval?: number
  accountId?: string | null
  startDate?: string | null
}

/** 复位/预填草稿（金额恒清空：新建与改价都要求显式输入金额）。 */
function reset(prefill: AgreementPrefill = {}) {
  amountYuan.value = ''
  currencyCode.value = prefill.currencyCode ?? app.defaultCurrency
  recurrenceType.value = prefill.recurrenceType ?? 'yearly'
  recurrenceInterval.value = prefill.recurrenceInterval ?? 1
  accountId.value = prefill.accountId ?? null
  startDate.value = prefill.startDate ?? todayStr()
}

/** 校验草稿；返回首个错误的用户文案，通过返回 null。 */
function validate(): string | null {
  if (!accountId.value) return t('policies.agreement.msg.accountRequired')
  const cents = yuanToCents(amountYuan.value)
  if (cents === null || cents <= 0) return t('policies.agreement.msg.amountInvalid')
  if (!startDate.value) return t('policies.agreement.msg.startRequired')
  return null
}

/** 组装创建入参（订阅形态 + 保单引用；校验通过后调用）。 */
function build(policyId: string, merchantId: string, productName: string): CreateScheduledInput {
  return {
    kind: 'subscription',
    account_id: accountId.value!,
    category_id: null,
    amount_cents: yuanToCents(amountYuan.value)!,
    currency_code: currencyCode.value ?? app.defaultCurrency,
    recurrence_type: recurrenceType.value,
    recurrence_interval: recurrenceInterval.value,
    recurrence_day: null,
    start_date: startDate.value!,
    note: productName || null,
    merchant_id: merchantId || null,
    policy_id: policyId,
  }
}

defineExpose({ reset, validate, build })
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
        <span>{{ t('scheduled.form.every') }}</span>
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
