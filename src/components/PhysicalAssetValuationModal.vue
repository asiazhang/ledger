<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NText, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { yuanToCents } from '@/utils/money'
import { useFormShared } from '@/composables/useFormShared'
import { useAppStore } from '@/stores/app'
import { usePhysicalAssetsStore } from '@/stores/physicalAssets'
import type { PhysicalAsset, PhysicalAssetValuationInput } from '@/types'

/**
 * 更新估值弹窗（issue #467 T2 / ADR-0064）：「更新估值」的唯一入口——
 * 填金额 + 日期（缺省今天、可补录过去日期、未来日期由后端拒绝）+ 币种
 * （预选该资产当前估值币种）。每次保存追加一条估值历史行（旧值保留不覆盖），
 * 当前估值变为最新一条并在列表生效；不在本弹窗展示或改写历史行。
 * 保存成功后关弹窗，列表经 store 重拉刷新；后端校验错误原样展示。
 */
const props = defineProps<{
  show: boolean
  /** 目标资产（当前估值为币种预选依据） */
  asset: PhysicalAsset | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()
const app = useAppStore()
const physicalAssetsStore = usePhysicalAssetsStore()
const { currencyOptions } = useFormShared()

// —— 表单状态 ——
const amountYuan = ref('')
const currency = ref<string | null>(null)
const valuationDate = ref<string | null>(null)

/** 打开时复位（immediate 兼容初始 show）：金额清空、币种预选当前估值币种、
 *  日期留空 = 今天（后端同语义缺省）。 */
watch(
  () => [props.show, props.asset] as const,
  () => {
    if (!props.show) return
    amountYuan.value = ''
    currency.value = props.asset?.current_valuation_currency_code ?? app.defaultCurrency
    valuationDate.value = null
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

async function save() {
  const asset = props.asset
  if (!asset) return
  // 客户端必填校验（消息与后端错误码文案同源，双保险防呆）
  const cents = yuanToCents(amountYuan.value)
  if (amountYuan.value.trim() === '' || cents === null) {
    message.warning(t('physicalAssets.valuation.msg.amountRequired'))
    return
  }
  if (cents <= 0) {
    message.warning(t('physicalAssets.valuation.msg.amountInvalid'))
    return
  }
  if (!currency.value) {
    message.warning(t('physicalAssets.valuation.msg.currencyRequired'))
    return
  }
  const input: PhysicalAssetValuationInput = {
    amount_cents: cents,
    currency_code: currency.value,
    // 日期留空 = 今天（后端缺省同语义）；可补录过去，未来由后端拒绝
    valuation_date: valuationDate.value || null,
  }
  try {
    await physicalAssetsStore.updateValuation(asset.id, input)
    message.success(t('physicalAssets.msg.valuationUpdated'))
    close()
  } catch (e) {
    // 后端校验错误原样展示（如「估值日期 … 不能是未来」），弹窗不关、内容不丢
    message.error(t('physicalAssets.msg.saveFailed', { msg: errorMessage(e) }))
  }
}

defineExpose({ save })
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('physicalAssets.valuation.title')"
    card-size="sm"
    data-testid="physical-asset-valuation-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('physicalAssets.valuation.label.amount')">
        <NInput
          v-model:value="amountYuan"
          :placeholder="t('physicalAssets.valuation.placeholder.amount')"
          style="width: 160px"
          data-testid="physical-asset-valuation-amount"
        />
        <AppSelect
          v-model:value="currency"
          :options="currencyOptions"
          :placeholder="t('physicalAssets.form.placeholder.currency')"
          style="width: 120px"
          data-testid="physical-asset-valuation-currency-select"
        />
      </NFormItem>
      <NFormItem :label="t('physicalAssets.valuation.label.date')">
        <AppDatePicker
          v-model:formatted-value="valuationDate"
          type="date"
          value-format="yyyy-MM-dd"
          clearable
          :placeholder="t('physicalAssets.valuation.placeholder.date')"
          style="width: 160px"
          data-testid="physical-asset-valuation-date"
        />
      </NFormItem>
      <!-- 辅助说明统一段落式（spec #630 / #635）：不再内联挤在日期表单项旁 -->
      <NText depth="3" class="form-hint">
        {{ t('physicalAssets.valuation.dateHint') }}
      </NText>

      <NSpace justify="end">
        <NButton @click="close">{{ t('physicalAssets.form.cancel') }}</NButton>
        <NButton type="primary" data-testid="physical-asset-valuation-save" @click="save">
          {{ t('physicalAssets.form.save') }}
        </NButton>
      </NSpace>
    </NForm>
  </AppModal>
</template>

<style scoped>
/* 表单下方段落式辅助说明（spec #630）：块级 + 上下留白，不挤占表单项 */
.form-hint {
  display: block;
  margin: 8px 0 12px;
}
</style>
