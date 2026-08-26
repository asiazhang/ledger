<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NDatePicker,
  NButton,
  NText,
  NSpace,
} from 'naive-ui'
import { useRefundForm } from '@/composables/useRefundForm'
import { formatAmount } from '@/types'
import { useReferenceStore } from '@/stores/reference'

const emit = defineEmits<{ created: [] }>()

const ctx = useRefundForm({ onCreated: () => emit('created') })
const reference = useReferenceStore()
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem label="退款关联">
        <NSelect
          v-model:value="ctx.refundTargetId.value"
          :options="ctx.refundTargetOptions.value"
          filterable
          placeholder="选择原支出交易"
          style="width: 340px"
        />
      </NFormItem>

      <NFormItem v-if="ctx.refundTarget.value" label="原交易">
        <NText depth="3" style="font-size: 12px">
          {{ formatAmount(ctx.refundTarget.value.amount_native_cents, reference.getCurrency(ctx.refundTarget.value.currency_code)) }}
          · {{ reference.categoryPath(ctx.refundTarget.value.category_id) || '-' }}
          · {{ reference.accountMap.get(ctx.refundTarget.value.account_id)?.name ?? '-' }}
        </NText>
      </NFormItem>

      <NFormItem label="退款金额">
        <NInputNumber
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          placeholder="退款金额"
          style="width: 160px"
        />
        <NSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          disabled
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="账户">
        <NSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          disabled
          placeholder="由原交易决定"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem label="日期">
        <NDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="ctx.note.value" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        记退款
      </NButton>
    </NSpace>
  </NForm>
</template>
