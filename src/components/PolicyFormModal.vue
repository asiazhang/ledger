<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NSwitch, NText, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import PolicyAgreementFields from '@/components/PolicyAgreementFields.vue'
// 编辑模式的协议历史区（v-if=editing）：模板曾未 import、渲染为未知元素，
// 编辑弹窗协议历史实际不显示——本票顺手修复（issue #713 改动本文件时发现）。
import PolicyAgreementSection from '@/components/PolicyAgreementSection.vue'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { todayStr } from '@/utils/date'
import { policyStatAmountText } from '@/utils/policy-stats'
import { yuanToCents, centsToYuan } from '@/utils/money'
import { useFormShared } from '@/composables/useFormShared'
import { resolveInsurerRef } from '@/composables/resolve-insurer'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { usePoliciesStore } from '@/stores/policies'
import { api } from '@/api'
import type { Policy, PolicyInput } from '@/types'

/**
 * 保单新建/编辑弹窗（issue #360 / ADR-0051）：静态合同要素录入。
 *
 * 保司为保险域自有独立字典（Insurer，ADR-0082）：PinyinSelect tag 模式「选择或
 * 输入即建」——保存时单点解析：选中 id 原样携带、输入文本精确命中按名复用、
 * 未命中 `create_insurer` 即建（重名竞态先强制重拉按名复用，同商户接缝先例）。
 * 保障期间止日可空 = 长期/终身；保额可选纯展示，与币种成对（清金额即清币种）。
 * 编辑模式全量替换；保存成功后关弹窗，列表经 store 重拉刷新。
 *
 * 缴费协议区（issue #362 / ADR-0051 决策 2，新建模式）：可折叠可选——开关
 * 默认关（趸交/缴清纯档案）；开启后填频率/金额/扣款账户/起始日，保存时先建档
 * 再创建订阅形态协议（携带保单引用，不挂商户，ADR-0082 决策 2），期次自动生成
 * 带保单引用、不带商户的保费流水。编辑模式的协议历史与添加/改价见
 * PolicyAgreementSection。
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
const insurerRef = ref<string | null>(null)
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

const insurerOptions = computed(() =>
  reference.insurers.map((i) => ({ label: i.name, value: i.id })),
)

/** 保额录入中：币种选择仅在填了保额时有意义（与后端「成对」校验同形）。 */
const coverageFilled = computed(() => coverageYuan.value.trim() !== '')

// —— 保单视角统计（issue #363，编辑模式 = 详情）：消费 store 同源快照，
// 实时推导不落库；列表与弹窗共用同一份数据，合计展示经共享辅助同口径 ——
const statsSummary = computed(() => {
  if (!props.editing) return null
  return policiesStore.statsById.get(props.editing.id) ?? null
})

const paidText = computed(() =>
  policyStatAmountText(statsSummary.value, (s) => s.total_paid_native_cents),
)

const inflowText = computed(() =>
  policyStatAmountText(statsSummary.value, (s) => s.total_inflow_native_cents),
)

// 到期态摘要（与列表徽标同一推导口径）：止日空 = 长期/终身（永不判到期）；
// 止日非空时消费统计同源 is_expired，统计行未加载时回落本地推导。
const expiryText = computed(() => {
  const p = props.editing
  if (!p) return '—'
  if (p.end_date === null) return t('policies.expiry.lifetime')
  const expired = statsSummary.value?.is_expired ?? p.end_date < todayStr()
  return expired ? t('policies.expiry.expired') : t('policies.expiry.active')
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
    insurerRef.value = p?.insurer_id ?? null
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
 * 保司解析（保存时单点收口）：「输入即建 + 重名兜底」交互收口在共享接缝
 * `resolveInsurerRef`——选中 id 原样携带（编辑维持历史引用）、输入名精确命中
 * 在用保司复用、未命中即建保司（ADR-0082：即建目标不再是商户）。
 */
async function resolveInsurer(): Promise<string> {
  return (await resolveInsurerRef(insurerRef.value)) ?? ''
}

async function save() {
  // 客户端必填校验（消息与后端错误码文案同源，双保险防呆）
  if (!insurerRef.value?.trim()) {
    message.warning(t('policies.form.msg.insurerRequired'))
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

  let insurerId: string
  try {
    insurerId = await resolveInsurer()
  } catch (e) {
    message.error(t('policies.form.msg.insurerFailed', { msg: errorMessage(e) }))
    return
  }
  if (!insurerId) {
    message.warning(t('policies.form.msg.insurerRequired'))
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
    insurer_id: insurerId,
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
      // 不挂商户（issue #713 / ADR-0082 决策 2：保费归属走保单引用），
      // 备注带险种可读。
      if (withAgreement.value && agreementFields.value) {
        await api.createScheduledTransaction(
          agreementFields.value.build(policyId, input.product_name),
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
    card-size="md"
    :title="editing ? t('policies.form.titleEdit') : t('policies.form.titleCreate')"
    data-testid="policy-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，issue #699）：NFormItem 默认零行距 -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('policies.form.label.insurer')">
          <PinyinSelect
            v-model:value="insurerRef"
            :options="insurerOptions"
            tag
            :placeholder="t('policies.form.placeholder.insurer')"
            style="width: 280px"
            data-testid="policy-insurer"
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
              style="width: 165px"
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
              style="width: 165px"
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
        <NFormItem v-if="!editing" :label="t('policies.agreement.sectionTitle')">
          <NSwitch v-model:value="withAgreement" data-testid="policy-agreement-toggle" />
        </NFormItem>
        <!-- 辅助说明统一段落式（spec #630 / #636）：不再内联 opacity 挤在开关旁 -->
        <NText v-if="!editing" depth="3" class="form-hint">
          {{ t('policies.agreement.toggleHint') }}
        </NText>
        <div v-if="!editing" v-show="withAgreement" data-testid="policy-agreement-fields">
          <PolicyAgreementFields ref="agreementFields" />
        </div>

        <!-- 编辑模式：保单视角统计（实时推导，issue #363）+ 协议历史与添加/改价 -->
        <div
          v-if="editing"
          data-testid="policy-stats-summary"
          style="display: flex; gap: 16px; font-size: 13px"
        >
          <span>{{ t('policies.stats.paid') }}：<strong>{{ paidText }}</strong></span>
          <span>{{ t('policies.stats.inflow') }}：<strong>{{ inflowText }}</strong></span>
          <span>
            {{ t('policies.stats.nextCharge') }}：<strong>{{
              statsSummary?.next_charge_date ?? '—'
            }}</strong>
          </span>
          <span>{{ t('policies.stats.expiry') }}：<strong>{{ expiryText }}</strong></span>
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
      </NSpace>
    </NForm>
  </AppModal>
</template>

<style scoped>
/* 表单下方段落式辅助说明（spec #630）：块级呈现；上下留白归 NSpace 行距（issue #699） */
.form-hint {
  display: block;
}
</style>
