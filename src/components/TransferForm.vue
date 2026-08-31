<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NButton,
  NSpace,
} from 'naive-ui'
import { t } from '@/i18n'
import AppSelect from '@/components/AppSelect.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useTransferForm } from '@/composables/useTransferForm'
import type { Transaction } from '@/types'

// 编辑模式（issue #178）：传入 editing 时回填既有交易并走更新命令。
const props = defineProps<{ editing?: Transaction | null }>()

const emit = defineEmits<{ created: []; saved: [] }>()

const ctx = useTransferForm({
  onCreated: () => emit('created'),
  onUpdated: () => emit('saved'),
  editing: () => props.editing ?? null,
})
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem :label="t('transactions.form.amount')">
        <NInputNumber
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          :placeholder="t('transactions.form.amount')"
          style="width: 160px"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.fromAccount')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          :placeholder="t('transactions.form.fromAccountPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.toAccount')">
        <PinyinSelect
          v-model:value="ctx.toAccountId.value"
          :options="ctx.accountOptions.value"
          :placeholder="t('transactions.form.toAccountPlaceholder')"
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

      <NButton type="primary" @click="ctx.submit">
        {{ editing ? t('transactions.form.saveChanges') : t('transactions.form.submitTransfer') }}
      </NButton>
    </NSpace>
  </NForm>
</template>
