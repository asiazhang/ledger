<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NButton,
  NSpace,
} from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useInvestmentForm } from '@/composables/useInvestmentForm'
import type { Transaction, TransactionTrade } from '@/types'

const props = defineProps<{
  kind: 'buy' | 'sell'
  submitLabel: string
  /** 编辑模式（issue #180）：待编辑交易与买卖明细，创建路径不传 */
  editing?: Transaction | null
  trade?: TransactionTrade | null
}>()
const emit = defineEmits<{ created: []; saved: [] }>()

const ctx = useInvestmentForm(props.kind, {
  onCreated: () => emit('created'),
  onUpdated: () => emit('saved'),
  editing: () => props.editing ?? null,
  trade: () => props.trade ?? null,
})
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
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          :disabled="true"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="投资账户">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.investmentAccountOptions.value"
          placeholder="选择投资账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem label="标的">
        <!-- 远程搜索标的（字典同步全市场后不可前端全量驻留）：拼音过滤由后端
             list_instruments 以统一模糊语义（ADR-0027）完成，本组件的本地
             filter 在 remote 下不生效，仅收口 filterable 保持实体下拉载体一致。 -->
        <PinyinSelect
          v-model:value="ctx.instrumentId.value"
          :options="ctx.instrumentOptions.value"
          placeholder="选择标的（输入代码/名称/拼音搜索）"
          remote
          clearable
          :loading="ctx.searchingInstruments.value"
          virtual-scroll
          style="width: 240px"
          @search="ctx.searchInstruments"
        >
          <template #empty>未找到标的，可通过同步或 AI 导入新增</template>
        </PinyinSelect>
      </NFormItem>

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
        <AppDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="ctx.note.value" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        {{ editing ? '保存修改' : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
