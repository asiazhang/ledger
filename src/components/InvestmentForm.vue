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
import { t } from '@/i18n'
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
      <NFormItem :label="t('investments.form.amount')">
        <!-- 基金申赎（issue #302 / ADR-0038 金额权威）：确认单整分金额为权威输入；
             其余类型金额由后端按数量 × 单价 ± 手续费重算，此处只展示 -->
        <NInputNumber
          v-if="ctx.isFundInstrument.value"
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          :placeholder="t('investments.form.amountPlaceholder')"
          style="width: 160px"
        />
        <NInputNumber
          v-else
          :value="ctx.investmentAmount.value"
          :disabled="true"
          :precision="2"
          :placeholder="t('investments.form.amountAuto')"
          style="width: 160px"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          :disabled="true"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem :label="t('investments.form.account')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.investmentAccountOptions.value"
          :placeholder="t('investments.form.accountPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('investments.form.instrument')">
        <!-- 远程搜索标的（字典同步全市场后不可前端全量驻留）：拼音过滤由后端
             list_instruments 以统一模糊语义（ADR-0027）完成，本组件的本地
             filter 在 remote 下不生效，仅收口 filterable 保持实体下拉载体一致。 -->
        <PinyinSelect
          v-model:value="ctx.instrumentId.value"
          :options="ctx.instrumentOptions.value"
          :placeholder="t('investments.form.instrumentPlaceholder')"
          remote
          clearable
          :loading="ctx.searchingInstruments.value"
          virtual-scroll
          style="width: 240px"
          @search="ctx.searchInstruments"
        >
          <template #empty>{{ t('investments.form.instrumentEmpty') }}</template>
        </PinyinSelect>
      </NFormItem>

      <NFormItem :label="ctx.isFundInstrument.value ? t('investments.form.shares') : t('investments.form.quantity')">
        <NInputNumber
          v-model:value="ctx.quantity.value"
          :min="0"
          :precision="4"
          :placeholder="ctx.isFundInstrument.value ? t('investments.form.sharesPlaceholder') : t('investments.form.quantityPlaceholder')"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem :label="ctx.isFundInstrument.value ? t('investments.form.unitPriceNav') : t('investments.form.unitPrice')">
        <!-- 基金：单价由（金额 ∓ 手续费）÷ 份额反算，只读展示 4 位小数净值（净值
             以万分之一元刻度无损保真，ADR-0038）；其余类型单价为权威输入 -->
        <NInputNumber
          v-if="ctx.isFundInstrument.value"
          :value="ctx.derivedPrice.value"
          :disabled="true"
          :precision="4"
          :placeholder="t('investments.form.derivedPricePlaceholder')"
          style="width: 160px"
        />
        <NInputNumber
          v-else
          v-model:value="ctx.price.value"
          :min="0"
          :precision="2"
          :placeholder="t('investments.form.unitPricePlaceholder')"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem :label="t('investments.form.fee')">
        <NInputNumber
          v-model:value="ctx.fee.value"
          :min="0"
          :precision="2"
          :placeholder="t('investments.form.feePlaceholder')"
          style="width: 160px"
        />
      </NFormItem>

      <NFormItem :label="t('investments.form.date')">
        <AppDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem :label="t('investments.form.note')">
        <NInput v-model:value="ctx.note.value" :placeholder="t('investments.form.notePlaceholder')" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        {{ editing ? t('investments.form.saveEdit') : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
