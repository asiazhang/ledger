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
import { useInvestmentForm } from '@/composables/useInvestmentForm'
import { INSTRUMENT_TYPE_LABELS } from '@/types'

const props = defineProps<{
  kind: 'buy' | 'sell'
  submitLabel: string
}>()
const emit = defineEmits<{ created: [] }>()

const ctx = useInvestmentForm(props.kind, { onCreated: () => emit('created') })
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem label="金额">
        <NInputNumber
          :value="ctx.investmentAmount.value"
          :disabled="true"
          :precision="2"
          placeholder="自动计算"
          style="width: 160px"
        />
        <NSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          :disabled="true"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="投资账户">
        <NSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.investmentAccountOptions.value"
          placeholder="选择投资账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem label="标的">
        <NSpace align="center" :size="8">
          <NSelect
            v-model:value="ctx.instrumentId.value"
            :options="ctx.instrumentOptions.value"
            placeholder="选择标的（输入代码/名称搜索）"
            remote
            filterable
            clearable
            :loading="ctx.searchingInstruments.value"
            virtual-scroll
            style="width: 240px"
            @search="ctx.searchInstruments"
          />
          <NButton size="tiny" @click="ctx.showNewInstrument.value = !ctx.showNewInstrument.value">
            {{ ctx.showNewInstrument.value ? '取消' : '新增标的' }}
          </NButton>
        </NSpace>
      </NFormItem>

      <NSpace v-if="ctx.showNewInstrument.value" vertical :size="8">
        <NFormItem label="代码">
          <NInput
            v-model:value="ctx.newInstrumentSymbol.value"
            placeholder="如 NVDA"
            style="width: 120px"
          />
        </NFormItem>
        <NFormItem label="名称">
          <NInput
            v-model:value="ctx.newInstrumentName.value"
            placeholder="名称（可选）"
            style="width: 180px"
          />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect
            v-model:value="ctx.newInstrumentType.value"
            :options="Object.entries(INSTRUMENT_TYPE_LABELS).map(([value, label]) => ({ label, value }))"
            style="width: 120px"
          />
        </NFormItem>
        <NButton size="small" @click="ctx.createNewInstrument">
          保存标的
        </NButton>
      </NSpace>

      <NFormItem label="数量">
        <NInputNumber
          v-model:value="ctx.quantity.value"
          :min="0"
          :precision="4"
          placeholder="数量"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem label="单价">
        <NInputNumber
          v-model:value="ctx.price.value"
          :min="0"
          :precision="2"
          placeholder="单价"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem label="手续费">
        <NInputNumber
          v-model:value="ctx.fee.value"
          :min="0"
          :precision="2"
          placeholder="手续费"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem label="日期">
        <NDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="ctx.note.value" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        {{ submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
