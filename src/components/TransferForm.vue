<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NDatePicker,
  NButton,
  NSpace,
} from 'naive-ui'
import { useTransferForm } from '@/composables/useTransferForm'

const emit = defineEmits<{ created: [] }>()

const ctx = useTransferForm({ onCreated: () => emit('created') })
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem label="金额">
        <NInputNumber
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          placeholder="金额"
          style="width: 160px"
        />
        <NSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="转出账户">
        <NSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          placeholder="选择转出账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem label="转入账户">
        <NSelect
          v-model:value="ctx.toAccountId.value"
          :options="ctx.accountOptions.value"
          placeholder="目标账户"
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
        记转账
      </NButton>
    </NSpace>
  </NForm>
</template>
