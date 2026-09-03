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
        <!-- 字段错误态（ADR-0058 / #416）：数量/份额自由文本承载输入，不拦截不静默丢弃
             （取代 NInputNumber precision 钳制的旧行为）；格式错误（含超四位小数）即时
             红显（内置 status 错误色），红态持续到修正 -->
        <NInput
          v-model:value="ctx.quantityText.value"
          :status="ctx.quantityError.value ? 'error' : undefined"
          :placeholder="ctx.isFundInstrument.value ? t('investments.form.sharesPlaceholder') : t('investments.form.quantityPlaceholder')"
          style="width: 160px"
          @blur="ctx.markQuantityBlurred"
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
        <!-- 字段错误态（ADR-0058 / #416）：单价同数量接线；精度口径为万分之一元刻度
             （至多四位小数，价格刻度 ADR-0038）；基金形态无此输入面（反算只读），
             错误态不装配 -->
        <NInput
          v-else
          v-model:value="ctx.priceText.value"
          :status="ctx.priceError.value ? 'error' : undefined"
          :placeholder="t('investments.form.unitPricePlaceholder')"
          style="width: 160px"
          @blur="ctx.markPriceBlurred"
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

      <!-- 任一字段错误态下禁用（红框＋提交禁用两件同发，ADR-0058 决策 1） -->
      <NButton type="primary" :disabled="ctx.hasFieldError.value" @click="ctx.submit">
        {{ editing ? t('investments.form.saveEdit') : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
