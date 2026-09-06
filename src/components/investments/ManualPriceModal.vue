<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInputNumber, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { errorMessage as extractErrorMessage } from '@/utils/errors'
import { formatPrice, yuanToPrice } from '@/types'
import { todayStr } from '@/utils/date'
import type { Instrument } from '@/types'

// 手动报价弹窗（issue #291 / ADR-0036）：无行情数据源标的的「日期 + 价格」
// 单点录入。提交后一条通道两个落点（现价缓存 upsert + 价格历史周采样幂等覆盖，
// 后端同一命令完成）；回填早于最新价格点的旧价只沉淀历史、不动现价（由后端
// 最新点映像规则判定，回执按结果区分）。列表/持仓/走势刷新由后端广播的价格
// 失效信号驱动既有消费方完成，本组件与调用方零手动重拉。
const props = defineProps<{ show: boolean; instrument: Instrument | null }>()
const emit = defineEmits<{
  'update:show': [value: boolean]
  /** 录价成功回执文案（页面级展示）；列表刷新经价格失效信号，不由调用方重拉 */
  quoted: [message: string]
}>()

// 日期默认今天（录价当日即生效为主形态），价格以元输入（基金净值 4 位小数，
// precision 4），提交时经 yuanToPrice 换算万分之一元（不手写换算系数）。
const date = ref(todayStr())
const price = ref<number | null>(null)
const submitting = ref(false)
/** 弹窗内错误提示（后端校验等）：保持弹窗打开供修改重试 */
const error = ref<string | null>(null)

// 价格必须 > 0（后端同款校验，前端提前拦截）且日期已选才可提交
const canSubmit = computed(() => price.value !== null && price.value > 0 && !!date.value && !submitting.value)

// 打开时重置表单（日期回到今天、清空价格与错误），immediate 兼容初始即开
// （先例：MerchantEditModal / CreateInstrumentModal）
watch(
  () => props.show,
  (show) => {
    if (!show) return
    date.value = todayStr()
    price.value = null
    error.value = null
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

async function submit() {
  if (!canSubmit.value || !props.instrument) return
  const priceCents = yuanToPrice(price.value!)
  if (priceCents === null || priceCents <= 0) {
    error.value = t('investments.manualPrice.priceMustBePositive')
    return
  }
  submitting.value = true
  error.value = null
  try {
    const result = await api.recordManualPrice({
      instrument_id: props.instrument.id,
      date: date.value,
      price_cents: priceCents,
    })
    // 回执按落点结果区分：回填旧价只沉淀历史、不动现价（最新点映像规则）
    const message = result.current_price_written
      ? t('investments.manualPrice.success', {
          symbol: props.instrument.symbol,
          price: formatPrice(priceCents),
        })
      : t('investments.manualPrice.historyOnly', { symbol: props.instrument.symbol })
    emit('quoted', message)
    close()
  } catch (e) {
    error.value = extractErrorMessage(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('investments.manualPrice.title', { symbol: instrument?.symbol ?? '' })"
    card-size="md"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('investments.manualPrice.intro') }}
      </NText>
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NFormItem :label="t('investments.manualPrice.dateLabel')" required>
          <AppDatePicker
            v-model:formatted-value="date"
            type="date"
            value-format="yyyy-MM-dd"
            :disabled="submitting"
            style="width: 200px"
            data-testid="manual-quote-date"
          />
        </NFormItem>
        <NFormItem :label="t('investments.manualPrice.priceLabel')" required>
          <NInputNumber
            v-model:value="price"
            :min="0"
            :precision="4"
            :placeholder="t('investments.manualPrice.pricePlaceholder')"
            :disabled="submitting"
            style="width: 200px"
            data-testid="manual-quote-price"
          />
        </NFormItem>
      </NForm>
      <NText v-if="error" type="error" data-testid="manual-quote-error">
        {{ error }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton data-testid="cancel-manual-quote" :disabled="submitting" @click="close">
          {{ t('investments.manualPrice.cancel') }}
        </NButton>
        <NButton
          type="primary"
          data-testid="submit-manual-quote"
          :loading="submitting"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ t('investments.manualPrice.submit') }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
