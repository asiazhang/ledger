<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NSwitch, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PolicyAgreementFields from '@/components/PolicyAgreementFields.vue'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { todayStr } from '@/utils/date'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { useFormShared } from '@/composables/useFormShared'
import { resolveMerchantRef } from '@/composables/resolve-merchant'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import { api } from '@/api'
import { formatAmount } from '@/types'
import type { Policy, PolicyInput } from '@/types'

/**
 * 保单新建/编辑弹窗（issue #360 / ADR-0051）：静态合同要素录入。
 *
 * 保司复用商户字典（ADR-0051 决策 7）：PinyinSelect tag 模式「选择或输入即建」,
 * 保存时单点解析——选中 id 原样携带、输入文本精确命中按名复用、未命中
 * `create_merchant` 即建（重名竞态先强制重拉按名复用，同计划表单收口）。
 * 保障期间止日可空 = 长期/终身；保额可选纯展示，与币种成对（清金额即清币种）。
 * 编辑模式全量替换；保存成功后关弹窗，列表经 store 重拉刷新。
 *
 * 缴费协议区（issue #362 / ADR-0051 决策 2，新建模式）：可折叠可选——开关
 * 默认关（趸交/缴清纯档案）；开启后填频率/金额/扣款账户/起始日，保存时先建档
 * 再创建订阅形态协议（携带保单引用，保司/险种名随协议组装），期次自动生成
 * 带引用的保费流水。编辑模式的协议历史与添加/改价见 PolicyAgreementSection。
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

// —— 缴费协议区（新建模式，issue #362）：可折叠可选，默认关 = 趸交/缴清纯档案 ——
const withAgreement = ref(false)
const agreementFields = ref<InstanceType<typeof PolicyAgreementFields> | null>(null)

const merchantOptions = computed(() =>
  reference.merchants.map((m) => ({ label: m.name, value: m.id })),
)

/** 保额录入中：币种选择仅在填了保额时有意义（与后端「成对」校验同形）。 */
const coverageFilled = computed(() => coverageYuan.value.trim() !== '')

// —— 保单视角统计（issue #363，编辑模式 = 详情）：消费 store 同源快照，
// 实时推导不落库；列表与弹窗共用同一份数据，不做本地二次聚合 ——
const statsSummary = computed(() => {
  if (!props.editing) return null
  return policiesStore.statsById.get(props.editing.id) ?? null
})

const paidText = computed(() => {
  const s = statsSummary.value
  if (!s) return '—'
  return formatAmount(s.total_paid_native_cents, reference.getCurrency(s.native_currency))
})

const inflowText = computed(() => {
  const s = statsSummary.value
  if (!s) return '—'
  return formatAmount(s.total_inflow_native_cents, reference.getCurrency(s.native_currency))
})

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
    // 协议区随弹窗复位：开关默认关；字段组复位（起始日预填保障期间起日），
    // nextTick 等字段组随弹窗内容挂载/更新后可取 ref。
    withAgreement.value = false
    void nextTick(() => agreementFields.value?.reset({ startDate: startDate.value }))
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

/**
 * 商户解析（保存时单点收口）：「输入即建 + 重名兜底」交互收口在共享接缝
 * `resolveMerchantRef`（计划表单同款消费）——选中 id 原样携带（编辑维持历史
 * 引用）、输入名精确命中在用商户复用、未命中即建。
 */
async function resolveMerchant(): Promise<string> {
  return (await resolveMerchantRef(merchantRef.value)) ?? ''
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

  // 协议区字段校验前置（先于建档，避免半建档状态）：校验失败不提交任何请求。
  if (withAgreement.value) {
    const agreementErr = agreementFields.value?.validate()
    if (agreementErr) {
      message.warning(agreementErr)
      return
    }
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
      const policyId = await policiesStore.create(input)
      // 同时创建缴费协议（issue #362 / ADR-0051 决策 2）：订阅形态 + 保单引用；
      // 保司与险种名随字段组组装（保险公司即保费流水的付款对象，备注带险种可读）。
      if (withAgreement.value && agreementFields.value) {
        await api.createScheduledTransaction(
          agreementFields.value.build(policyId, merchantId, input.product_name),
        )
      }
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

      <!-- 新建模式：缴费协议折叠开关（默认关 = 趸交/缴清纯档案）+ 字段组 -->
      <template v-if="!editing">
        <NFormItem :label="t('policies.agreement.sectionTitle')">
          <NSpace :size="8" align="center">
            <NSwitch v-model:value="withAgreement" data-testid="policy-agreement-toggle" />
            <span style="opacity: 0.6; font-size: 12px">{{ t('policies.agreement.toggleHint') }}</span>
          </NSpace>
        </NFormItem>
        <div v-show="withAgreement" data-testid="policy-agreement-fields">
          <PolicyAgreementFields ref="agreementFields" />
        </div>
      </template>

      <!-- 编辑模式：协议历史 + 添加/改价（1 张保单 → 多段协议可展示） -->
      <!-- 编辑模式：保单视角统计（实时推导，issue #363）+ 协议历史与添加/改价 -->
      <div
        v-if="editing"
        data-testid="policy-stats-summary"
        style="display: flex; gap: 16px; margin-bottom: 12px; font-size: 13px"
      >
        <span>{{ t('policies.stats.paid') }}：<strong>{{ paidText }}</strong></span>
        <span>{{ t('policies.stats.inflow') }}：<strong>{{ inflowText }}</strong></span>
        <span>
          {{ t('policies.stats.nextCharge') }}：<strong>{{
            statsSummary?.next_charge_date ?? '—'
          }}</strong>
        </span>
      </div>
      <NFormItem
        v-if="editing"
        :label="t('policies.agreement.sectionTitle')"
        :show-feedback="false"
      >
        <PolicyAgreementSection
          :policy="editing"
          style="width: 100%"
          data-testid="policy-agreement-section"
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
