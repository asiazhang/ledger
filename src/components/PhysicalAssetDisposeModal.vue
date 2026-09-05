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
import type { PhysicalAsset, PhysicalAssetDisposeInput } from '@/types'

/**
 * 处置弹窗（issue #468 T3 / ADR-0064）：处置 = 状态标记的唯一入口——
 * 填处置日期（必填、留空由前端拦下，后端同守卫码化报错）+ 可选处置价与
 * 币种（成对，纯记录不进任何金额口径）。处置成功后资产退出默认列表与
 * 在持合计，档案保留可经「已处置」筛选回看；列表经 store 重拉刷新，
 * 后端校验错误原样展示。
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
const disposalDate = ref<string | null>(null)
const priceYuan = ref('')
const currency = ref<string | null>(null)

/** 打开时复位（immediate 兼容初始 show）：日期留空待填（必填）、金额清空、
 *  币种预选当前估值币种（纯记录，预选减少手选成本）。 */
watch(
  () => [props.show, props.asset] as const,
  () => {
    if (!props.show) return
    disposalDate.value = null
    priceYuan.value = ''
    currency.value = props.asset?.current_valuation_currency_code ?? app.defaultCurrency
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
  if (!disposalDate.value) {
    message.warning(t('physicalAssets.dispose.msg.dateRequired'))
    return
  }
  let priceCents: number | null = null
  if (priceYuan.value.trim() !== '') {
    const cents = yuanToCents(priceYuan.value)
    if (cents === null || cents <= 0) {
      message.warning(t('physicalAssets.dispose.msg.priceInvalid'))
      return
    }
    priceCents = cents
    if (!currency.value) {
      message.warning(t('physicalAssets.dispose.msg.currencyRequired'))
      return
    }
  }
  const input: PhysicalAssetDisposeInput = {
    disposal_date: disposalDate.value,
    disposal_price_cents: priceCents,
    disposal_currency_code: priceCents !== null ? currency.value : null,
  }
  try {
    await physicalAssetsStore.dispose(asset.id, input)
    message.success(t('physicalAssets.msg.disposed'))
    close()
  } catch (e) {
    // 后端校验错误原样展示（如「处置日期 … 不能是未来」），弹窗不关、内容不丢
    message.error(t('physicalAssets.msg.saveFailed', { msg: errorMessage(e) }))
  }
}

defineExpose({ save })
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('physicalAssets.dispose.title')"
    card-size="sm"
    data-testid="physical-asset-dispose-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('physicalAssets.dispose.label.date')">
        <AppDatePicker
          v-model:formatted-value="disposalDate"
          type="date"
          value-format="yyyy-MM-dd"
          :placeholder="t('physicalAssets.dispose.placeholder.date')"
          style="width: 160px"
          data-testid="physical-asset-dispose-date"
        />
      </NFormItem>
      <NFormItem :label="t('physicalAssets.dispose.label.price')">
        <NInput
          v-model:value="priceYuan"
          :placeholder="t('physicalAssets.dispose.placeholder.price')"
          style="width: 160px"
          data-testid="physical-asset-dispose-price"
        />
        <AppSelect
          v-model:value="currency"
          :options="currencyOptions"
          :placeholder="t('physicalAssets.form.placeholder.currency')"
          style="width: 120px"
          data-testid="physical-asset-dispose-currency-select"
        />
      </NFormItem>
      <!-- 辅助说明统一段落式（spec #630 / #635）：废除空 label 表单项 hack -->
      <NText depth="3" class="form-hint">
        {{ t('physicalAssets.dispose.dateHint') }}
      </NText>

      <NSpace justify="end">
        <NButton @click="close">{{ t('physicalAssets.form.cancel') }}</NButton>
        <NButton type="primary" data-testid="physical-asset-dispose-save" @click="save">
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
