<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { todayStr } from '@/utils/date'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { useFormShared } from '@/composables/useFormShared'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import type { Policy, PolicyInput } from '@/types'

/**
 * 保单新建/编辑弹窗（issue #360 / ADR-0051）：静态合同要素录入。
 *
 * 保司复用商户字典（ADR-0051 决策 7）：PinyinSelect tag 模式「选择或输入即建」，
 * 保存时单点解析——选中 id 原样携带、输入文本精确命中按名复用、未命中
 * `create_merchant` 即建（重名竞态先强制重拉按名复用，同计划表单收口）。
 * 保障期间止日可空 = 长期/终身；保额可选纯展示，与币种成对（清金额即清币种）。
 * 编辑模式全量替换；保存成功后关弹窗，列表经 store 重拉刷新。
 */
const props = defineProps<{
  show: boolean
  /** 待编辑保单；null = 新建模式 */
  editing: Policy | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()
const reference = useReferenceStore()
const app = useAppStore()
const policiesStore = usePoliciesStore()
const { currencyOptions } = useFormShared()

// —— 表单状态 ——
const merchantRef = ref<string | null>(null)
const policyNumber = ref('')
const productName = ref('')
const startDate = ref<string | null>(null)
const endDate = ref<string | null>(null)
const coverageYuan = ref('')
const coverageCurrency = ref<string | null>(null)
const note = ref('')

const merchantOptions = computed(() =>
  reference.merchants.map((m) => ({ label: m.name, value: m.id })),
)

/** 保额录入中：币种选择仅在填了保额时有意义（与后端「成对」校验同形）。 */
const coverageFilled = computed(() => coverageYuan.value.trim() !== '')

// 清空保额即清币种（成对原子，不产生只有币种的半挂状态）
watch(coverageFilled, (filled) => {
  if (!filled) coverageCurrency.value = null
})

/** 打开时回填/复位（弹窗内容关闭后仍在 DOM，打开瞬间同步；immediate 兼容初始 show）。 */
watch(
  () => [props.show, props.editing] as const,
  () => {
    if (!props.show) return
    const p = props.editing
    merchantRef.value = p?.merchant_id ?? null
    policyNumber.value = p?.policy_number ?? ''
    productName.value = p?.product_name ?? ''
    startDate.value = p?.start_date ?? todayStr()
    endDate.value = p?.end_date ?? null
    coverageYuan.value =
      p?.coverage_amount_cents != null ? String(centsToYuan(p.coverage_amount_cents)) : ''
    coverageCurrency.value = p?.coverage_currency_code ?? app.defaultCurrency
    note.value = p?.note ?? ''
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

/**
 * 商户解析（保存时单点收口，同计划表单的「输入即建」交互）：
 * 1. 选中既有商户（value 为 id，merchantMap 含软删显示映射）→ 原样携带；
 * 2. 输入文本精确命中在用商户名 → 按名复用；
 * 3. 未命中 → `create_merchant` 即建；重名错误（store 陈旧竞态）先强制重拉
 *    按名复用，仍失败才向上抛。
 */
async function resolveMerchant(): Promise<string> {
  const selected = merchantRef.value?.trim() ?? ''
  if (!selected) return ''
  if (reference.merchantMap.has(selected)) return selected
  const existing = reference.merchantByName.get(selected)
  if (existing) return existing.id
  try {
    return await api.createMerchant({ name: selected })
  } catch (e) {
    // 重名兜底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
    try {
      await reference.refresh()
    } catch {
      /* 保留原 create 错误 */
    }
    const retry = reference.merchantByName.get(selected)
    if (retry) return retry.id
    throw e
  }
}

async function save() {
  // 客户端必填校验（消息与后端错误码文案同源，双保险防呆）
  if (!merchantRef.value?.trim()) {
    message.warning(t('policies.form.msg.merchantRequired'))
    return
  }
  if (!policyNumber.value.trim()) {
    message.warning(t('policies.form.msg.numberRequired'))
    return
  }
  if (!productName.value.trim()) {
    message.warning(t('policies.form.msg.productRequired'))
    return
  }
  if (!startDate.value) {
    message.warning(t('policies.form.msg.startRequired'))
    return
  }
  if (endDate.value && endDate.value < startDate.value) {
    message.warning(t('policies.form.msg.endBeforeStart'))
    return
  }
  let coverageCents: number | null = null
  if (coverageFilled.value) {
    const cents = yuanToCents(coverageYuan.value)
    if (cents === null || cents <= 0) {
      message.warning(t('policies.form.msg.amountInvalid'))
      return
    }
    if (!coverageCurrency.value) {
      message.warning(t('policies.form.msg.currencyRequired'))
      return
    }
    coverageCents = cents
  }

  let merchantId: string
  try {
    merchantId = await resolveMerchant()
  } catch (e) {
    message.error(t('policies.form.msg.merchantFailed', { msg: errorMessage(e) }))
    return
  }
  if (!merchantId) {
    message.warning(t('policies.form.msg.merchantRequired'))
    return
  }

  const input: PolicyInput = {
    merchant_id: merchantId,
    policy_number: policyNumber.value.trim(),
    product_name: productName.value.trim(),
    start_date: startDate.value,
    end_date: endDate.value || null,
    // 保额与币种成对：清金额即清币种（不产生只有币种的半挂状态）
    coverage_amount_cents: coverageCents,
    coverage_currency_code: coverageCents !== null ? coverageCurrency.value : null,
    note: note.value.trim() || null,
  }
  try {
    if (props.editing) {
      await policiesStore.update(props.editing.id, input)
      message.success(t('policies.msg.saved'))
    } else {
      await policiesStore.create(input)
      message.success(t('policies.msg.created'))
    }
    close()
  } catch (e) {
    // 后端校验错误原样展示（如「保单号不能为空」），弹窗不关、内容不丢
    message.error(t('policies.msg.saveFailed', { msg: errorMessage(e) }))
  }
}

defineExpose({ save })
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="editing ? t('policies.form.titleEdit') : t('policies.form.titleCreate')"
    style="width: 460px"
    data-testid="policy-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('policies.form.label.merchant')">
        <PinyinSelect
          v-model:value="merchantRef"
          :options="merchantOptions"
          tag
          :placeholder="t('policies.form.placeholder.merchant')"
          style="width: 280px"
          data-testid="policy-merchant"
        />
      </NFormItem>
      <NFormItem :label="t('policies.form.label.policyNumber')">
        <NInput
          v-model:value="policyNumber"
          :placeholder="t('policies.form.placeholder.policyNumber')"
          data-testid="policy-number"
        />
      </NFormItem>
      <NFormItem :label="t('policies.form.label.productName')">
        <NInput
          v-model:value="productName"
          :placeholder="t('policies.form.placeholder.productName')"
          data-testid="policy-product"
        />
      </NFormItem>
      <NFormItem :label="t('policies.form.label.period')">
        <NSpace :size="8" align="center" inline>
          <AppDatePicker
            v-model:formatted-value="startDate"
            type="date"
            value-format="yyyy-MM-dd"
            :placeholder="t('policies.form.placeholder.startDate')"
            style="width: 140px"
            data-testid="policy-start-date"
            clearable
          />
          <span style="opacity: 0.6">~</span>
          <AppDatePicker
            v-model:formatted-value="endDate"
            type="date"
            value-format="yyyy-MM-dd"
            clearable
            :placeholder="t('policies.form.placeholder.endDate')"
            style="width: 140px"
            data-testid="policy-end-date"
          />
        </NSpace>
      </NFormItem>
      <NFormItem :label="t('policies.form.label.coverage')">
        <NInput
          v-model:value="coverageYuan"
          :placeholder="t('policies.form.placeholder.coverage')"
          style="width: 160px"
          data-testid="policy-coverage"
        />
        <AppSelect
          v-model:value="coverageCurrency"
          :options="currencyOptions"
          :disabled="!coverageFilled"
          :placeholder="t('policies.form.placeholder.coverageCurrency')"
          style="width: 120px"
          data-testid="policy-coverage-currency"
        />
      </NFormItem>
      <NFormItem :label="t('policies.form.label.note')">
        <NInput
          v-model:value="note"
          :placeholder="t('policies.form.placeholder.note')"
          data-testid="policy-note"
        />
      </NFormItem>
      <NSpace justify="end">
        <NButton @click="close">{{ t('policies.form.cancel') }}</NButton>
        <NButton type="primary" data-testid="policy-save" @click="save">
          {{ t('policies.form.save') }}
        </NButton>
      </NSpace>
    </NForm>
  </AppModal>
</template>
