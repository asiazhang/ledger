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
import { useConvertForm } from '@/composables/useConvertForm'
import type { Transaction, TransactionConvert } from '@/types'

/**
 * 基金转换表单（ADR-0099 / issue #979）：一笔转换 = 转出腿（A）+ 转入腿（B），
 * 无现金腿、不跨账户。两侧的**份额 + 确认单金额**为权威输入，两侧单价由金额 ÷ 份额
 * 反算、只读展示（金额权威、单价反算，与场外基金 buy/sell 同款，ADR-0038）。
 *
 * 不做基金/非基金形态分流：转换天然只有确认单一种录入形态。
 * 编辑模式另显示结转成本（行金额锚点，服务端按 FIFO 消耗算定的转入批次总成本）。
 */
const props = defineProps<{
  submitLabel: string
  /** 编辑模式：待编辑交易与转换两腿明细，创建路径不传 */
  editing?: Transaction | null
  convert?: TransactionConvert | null
}>()
const emit = defineEmits<{ created: []; saved: [] }>()

const ctx = useConvertForm({
  onCreated: () => emit('created'),
  onUpdated: () => emit('saved'),
  editing: () => props.editing ?? null,
  convert: () => props.convert ?? null,
})
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem :label="t('investments.form.account')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.investmentAccountOptions.value"
          :placeholder="t('investments.form.accountPlaceholder')"
          style="width: 220px"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          :disabled="true"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <!-- 转出腿：标的 / 份额 / 确认单金额 / 反算单价 -->
      <NFormItem :label="t('investments.form.convertOutInstrument')">
        <PinyinSelect
          v-model:value="ctx.outInstrumentId.value"
          :options="ctx.outInstrumentOptions.value"
          :placeholder="t('investments.form.convertOutInstrumentPlaceholder')"
          remote
          clearable
          :loading="ctx.searchingOut.value"
          virtual-scroll
          style="width: 240px"
          @search="ctx.searchOutInstruments"
        >
          <template #empty>{{ t('investments.form.instrumentEmpty') }}</template>
        </PinyinSelect>
      </NFormItem>
      <NFormItem :label="t('investments.form.convertOutShares')">
        <NInput
          v-model:value="ctx.outQuantityText.value"
          :status="ctx.outQuantityError.value ? 'error' : undefined"
          :placeholder="t('investments.form.convertOutSharesPlaceholder')"
          style="width: 160px"
          @blur="ctx.markOutBlurred"
        />
      </NFormItem>
      <NFormItem :label="t('investments.form.convertOutAmount')">
        <NInputNumber
          v-model:value="ctx.outAmount.value"
          :min="0"
          :precision="2"
          :placeholder="t('investments.form.convertOutAmountPlaceholder')"
          style="width: 160px"
        />
      </NFormItem>
      <NFormItem :label="t('investments.form.convertOutPrice')">
        <NInputNumber
          :value="ctx.derivedOutPrice.value"
          :disabled="true"
          :precision="4"
          :placeholder="t('investments.form.convertPriceAuto')"
          style="width: 160px"
        />
      </NFormItem>

      <!-- 转入腿：标的 / 份额 / 确认单金额 / 反算单价 -->
      <NFormItem :label="t('investments.form.convertInInstrument')">
        <PinyinSelect
          v-model:value="ctx.inInstrumentId.value"
          :options="ctx.inInstrumentOptions.value"
          :placeholder="t('investments.form.convertInInstrumentPlaceholder')"
          remote
          clearable
          :loading="ctx.searchingIn.value"
          virtual-scroll
          style="width: 240px"
          @search="ctx.searchInInstruments"
        >
          <template #empty>{{ t('investments.form.instrumentEmpty') }}</template>
        </PinyinSelect>
      </NFormItem>
      <NFormItem :label="t('investments.form.convertInShares')">
        <NInput
          v-model:value="ctx.inQuantityText.value"
          :status="ctx.inQuantityError.value ? 'error' : undefined"
          :placeholder="t('investments.form.convertInSharesPlaceholder')"
          style="width: 160px"
          @blur="ctx.markInBlurred"
        />
      </NFormItem>
      <NFormItem :label="t('investments.form.convertInAmount')">
        <NInputNumber
          v-model:value="ctx.inAmount.value"
          :min="0"
          :precision="2"
          :placeholder="t('investments.form.convertInAmountPlaceholder')"
          style="width: 160px"
        />
      </NFormItem>
      <NFormItem :label="t('investments.form.convertInPrice')">
        <NInputNumber
          :value="ctx.derivedInPrice.value"
          :disabled="true"
          :precision="4"
          :placeholder="t('investments.form.convertPriceAuto')"
          style="width: 160px"
        />
      </NFormItem>

      <!-- 结转成本（只读，仅编辑回填时在场）：行金额锚点，转入批次总成本 -->
      <NFormItem v-if="ctx.carriedCost.value != null" :label="t('investments.form.convertCarriedCost')">
        <NInputNumber
          :value="ctx.carriedCost.value"
          :disabled="true"
          :precision="2"
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

      <!-- 任一字段错误态下禁用（红框＋提交禁用两件同发，ADR-0058 决策 1） -->
      <NButton type="primary" :disabled="ctx.hasFieldError.value" @click="ctx.submit">
        {{ editing ? t('investments.form.saveEdit') : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
