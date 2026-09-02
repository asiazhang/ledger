<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NButton,
  NText,
  NSpace,
} from 'naive-ui'
import { t } from '@/i18n'
import AppSelect from '@/components/AppSelect.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { useRefundForm } from '@/composables/useRefundForm'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { formatAmount } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import type { Transaction } from '@/types'

const emit = defineEmits<{ created: [] }>()

/** 行内退款（issue #151）：传入时原交易由所在行固定，隐藏搜索下拉、
 * 展示原交易只读信息；不传则保留搜索选择模式（记一笔弹窗）。 */
const props = defineProps<{ fixedTarget?: Transaction | null }>()

const ctx = useRefundForm({
  onCreated: () => emit('created'),
  fixedTarget: () => props.fixedTarget ?? null,
})
const reference = useReferenceStore()
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem v-if="!fixedTarget" :label="t('transactions.refund.link')">
        <PinyinSelect
          v-model:value="ctx.refundTargetId.value"
          :options="ctx.refundTargetOptions.value"
          :placeholder="t('transactions.refund.targetPlaceholder')"
          style="width: 340px"
        />
      </NFormItem>

      <NFormItem v-if="ctx.refundTarget.value" :label="t('transactions.refund.original')">
        <NText depth="3" style="font-size: 12px">
          {{ ctx.refundTarget.value.date }} ·
          {{ formatAmount(ctx.refundTarget.value.amount_cents, reference.getCurrency(ctx.refundTarget.value.currency_code)) }}
          · {{ reference.categoryPath(ctx.refundTarget.value.category_id) || '-' }}
          · {{ reference.accountMap.get(ctx.refundTarget.value.account_id)?.name ?? '-' }}
        </NText>
      </NFormItem>

      <NFormItem :label="t('transactions.refund.amount')">
        <!-- 字段错误态（ADR-0058 / #415）：自由文本承载输入，不拦截不静默丢弃（取
             代 NInputNumber 失焦清空非法文本的旧行为）；格式错误即时红显（内置
             status 错误色），红态持续到修正 -->
        <NInput
          v-model:value="ctx.amountText.value"
          :status="ctx.amountError.value ? 'error' : undefined"
          :placeholder="t('transactions.refund.amount')"
          style="width: 160px"
          @blur="ctx.markAmountBlurred"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          disabled
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.account')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          disabled
          :placeholder="t('transactions.refund.accountLockedPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.date')">
        <AppDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem :label="t('transactions.form.note')">
        <NInput
          v-model:value="ctx.note.value"
          :placeholder="t('transactions.form.notePlaceholder')"
          style="width: 280px"
        />
      </NFormItem>

      <!-- 任一字段错误态下禁用（红框＋提交禁用两件同发，ADR-0058 决策 1） -->
      <NButton type="primary" :disabled="ctx.hasFieldError.value" @click="ctx.submit">
        {{ t('transactions.refund.submit') }}
      </NButton>
    </NSpace>
  </NForm>
</template>
