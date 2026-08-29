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
      <NFormItem v-if="!fixedTarget" label="退款关联">
        <PinyinSelect
          v-model:value="ctx.refundTargetId.value"
          :options="ctx.refundTargetOptions.value"
          placeholder="选择原支出交易"
          style="width: 340px"
        />
      </NFormItem>

      <NFormItem v-if="ctx.refundTarget.value" label="原交易">
        <NText depth="3" style="font-size: 12px">
          {{ ctx.refundTarget.value.date }} ·
          {{ formatAmount(ctx.refundTarget.value.amount_cents, reference.getCurrency(ctx.refundTarget.value.currency_code)) }}
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
        <PinyinSelect
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
