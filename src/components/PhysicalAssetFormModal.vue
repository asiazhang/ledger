<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { useFormShared } from '@/composables/useFormShared'
import { useAppStore } from '@/stores/app'
import { usePhysicalAssetsStore } from '@/stores/physicalAssets'
import type { PhysicalAsset, PhysicalAssetInput, PhysicalAssetUpdateInput } from '@/types'

/**
 * 实物资产新建/编辑弹窗（issue #466 建档 / issue #467 T2 编辑 / ADR-0064）：
 * 新建时名称与当前估值必填（三十秒建档的最简表单），购买日期与购买价（含
 * 币种）可选；估值与购买价币种均预选默认币种（核心交易域 DefaultCurrency
 * 设备偏好）。估值日期不出表单（缺省 = 今天）。
 *
 * 编辑模式（T2，PolicyFormModal 先例）：仅名称与购买信息可改（全量替换），
 * **估值字段结构性排除**（v-if 不渲染）——估值只能经「更新估值」变更（历史
 * 只追加不改写，ADR-0064），编辑表单无估值入口。保存成功后关弹窗，列表经
 * store 重拉刷新；后端校验错误原样展示，弹窗不关、内容不丢。
 */
const props = defineProps<{
  show: boolean
  /** 待编辑资产；null = 新建模式 */
  editing: PhysicalAsset | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()
const app = useAppStore()
const physicalAssetsStore = usePhysicalAssetsStore()
const { currencyOptions } = useFormShared()

// —— 表单状态 ——
const name = ref('')
const valuationYuan = ref('')
const valuationCurrency = ref<string | null>(null)
const purchaseDate = ref<string | null>(null)
const purchaseYuan = ref('')
const purchaseCurrency = ref<string | null>(null)

const valuationFilled = computed(() => valuationYuan.value.trim() !== '')
const purchaseFilled = computed(() => purchaseYuan.value.trim() !== '')

/** 打开时回填/复位（PolicyFormModal 先例；immediate 兼容初始 show）：
 *  编辑模式预填名称与购买信息；新建模式复位为空白建档单（币种预选默认币种）。 */
watch(
  () => [props.show, props.editing] as const,
  () => {
    if (!props.show) return
    const p = props.editing
    name.value = p?.name ?? ''
    // 估值字段仅新建模式使用（编辑模式结构性排除，不渲染不提交）
    valuationYuan.value = ''
    valuationCurrency.value = app.defaultCurrency
    purchaseDate.value = p?.purchase_date ?? null
    purchaseYuan.value =
      p?.purchase_price_cents != null ? String(centsToYuan(p.purchase_price_cents)) : ''
    purchaseCurrency.value = p?.purchase_currency_code ?? app.defaultCurrency
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

async function save() {
  // 客户端必填校验（消息与后端错误码文案同源，双保险防呆）
  if (!name.value.trim()) {
    message.warning(t('physicalAssets.form.msg.nameRequired'))
    return
  }
  let purchaseCents: number | null = null
  if (purchaseFilled.value) {
    const cents = yuanToCents(purchaseYuan.value)
    if (cents === null || cents <= 0) {
      message.warning(t('physicalAssets.form.msg.purchaseInvalid'))
      return
    }
    if (!purchaseCurrency.value) {
      message.warning(t('physicalAssets.form.msg.purchaseCurrencyRequired'))
      return
    }
    purchaseCents = cents
  }

  // 购买价与币种成对（清金额即清币种，不产生只有币种的半挂状态）
  try {
    if (props.editing) {
      // 编辑模式（T2）：仅名称 / 购买信息全量替换，无估值字段（结构性排除）
      const input: PhysicalAssetUpdateInput = {
        name: name.value.trim(),
        purchase_date: purchaseDate.value || null,
        purchase_price_cents: purchaseCents,
        purchase_currency_code: purchaseCents !== null ? purchaseCurrency.value : null,
      }
      await physicalAssetsStore.update(props.editing.id, input)
      message.success(t('physicalAssets.msg.updated'))
    } else {
      // 新建模式：估值必填（即首条估值历史行）
      if (!valuationFilled.value) {
        message.warning(t('physicalAssets.form.msg.valuationRequired'))
        return
      }
      const valuationCents = yuanToCents(valuationYuan.value)
      if (valuationCents === null || valuationCents <= 0) {
        message.warning(t('physicalAssets.form.msg.valuationInvalid'))
        return
      }
      if (!valuationCurrency.value) {
        message.warning(t('physicalAssets.form.msg.valuationCurrencyRequired'))
        return
      }
      const input: PhysicalAssetInput = {
        name: name.value.trim(),
        purchase_date: purchaseDate.value || null,
        purchase_price_cents: purchaseCents,
        purchase_currency_code: purchaseCents !== null ? purchaseCurrency.value : null,
        initial_valuation_cents: valuationCents,
        initial_valuation_currency_code: valuationCurrency.value,
        initial_valuation_date: null,
      }
      await physicalAssetsStore.create(input)
      message.success(t('physicalAssets.msg.created'))
    }
    close()
  } catch (e) {
    // 后端校验错误原样展示（如「资产名称不能为空」），弹窗不关、内容不丢
    message.error(t('physicalAssets.msg.saveFailed', { msg: errorMessage(e) }))
  }
}

defineExpose({ save })
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="editing ? t('physicalAssets.form.titleEdit') : t('physicalAssets.form.title')"
    style="width: 460px"
    data-testid="physical-asset-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('physicalAssets.form.label.name')">
        <NInput
          v-model:value="name"
          :placeholder="t('physicalAssets.form.placeholder.name')"
          data-testid="physical-asset-name"
        />
      </NFormItem>
      <!-- 编辑模式无估值字段：估值只能经「更新估值」变更（历史只追加，T2） -->
      <NFormItem v-if="!editing" :label="t('physicalAssets.form.label.valuation')">
        <NInput
          v-model:value="valuationYuan"
          :placeholder="t('physicalAssets.form.placeholder.valuation')"
          style="width: 160px"
          data-testid="physical-asset-valuation"
        />
        <AppSelect
          v-model:value="valuationCurrency"
          :options="currencyOptions"
          :placeholder="t('physicalAssets.form.placeholder.currency')"
          style="width: 120px"
          data-testid="physical-asset-valuation-currency"
        />
      </NFormItem>
      <NFormItem :label="t('physicalAssets.form.label.purchaseDate')">
        <AppDatePicker
          v-model:formatted-value="purchaseDate"
          type="date"
          value-format="yyyy-MM-dd"
          clearable
          :placeholder="t('physicalAssets.form.placeholder.purchaseDate')"
          style="width: 160px"
          data-testid="physical-asset-purchase-date"
        />
      </NFormItem>
      <NFormItem :label="t('physicalAssets.form.label.purchasePrice')">
        <NInput
          v-model:value="purchaseYuan"
          :placeholder="t('physicalAssets.form.placeholder.purchasePrice')"
          style="width: 160px"
          data-testid="physical-asset-purchase-price"
        />
        <AppSelect
          v-model:value="purchaseCurrency"
          :options="currencyOptions"
          :disabled="!purchaseFilled"
          :placeholder="t('physicalAssets.form.placeholder.currency')"
          style="width: 120px"
          data-testid="physical-asset-purchase-currency"
        />
      </NFormItem>

      <NSpace justify="end">
        <NButton @click="close">{{ t('physicalAssets.form.cancel') }}</NButton>
        <NButton type="primary" data-testid="physical-asset-save" @click="save">
          {{ t('physicalAssets.form.save') }}
        </NButton>
      </NSpace>
    </NForm>
  </AppModal>
</template>
