<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import { errorMessage as extractErrorMessage } from '@/utils/errors'
import type { InstrumentInput, InstrumentType } from '@/types'

// 自建标的手动创建弹窗（issue #290 / ADR-0036）：类型白名单（债券/ETF/其他，
// 基金唯一创建入口归「添加基金」按代码即拉，不占通用表单选项）、名称必填
// （自建标的主身份是名称）、代码自由文本、市场固定未知、币种可选（默认人民币）。
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{
  'update:show': [value: boolean]
  /** 创建成功回执文案（页面级展示），列表重拉由父组件负责 */
  created: [message: string]
}>()

const reference = useReferenceStore()

// 类型三选（白名单在 UI 侧再收一道；后端 IPC 命令入口层同款守卫兜底）。
// 故意不选股票（同步管）与基金（按代码即拉管），与后端白名单同源。
const TYPE_OPTIONS = computed(() => [
  { label: t('investments.create.typeBond'), value: 'bond' as InstrumentType },
  { label: t('investments.create.typeEtf'), value: 'etf' as InstrumentType },
  { label: t('investments.create.typeOther'), value: 'other' as InstrumentType },
])

const form = ref({
  kind: null as InstrumentType | null,
  symbol: '',
  name: '',
  currencyCode: 'CNY',
})
const submitting = ref(false)
/** 弹窗内错误提示（后端白名单/名称校验、重复码等）：保持弹窗打开供修改重试 */
const error = ref<string | null>(null)

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} · ${c.name}`, value: c.code })),
)

// 名称必填（trim 后非空）、代码必填、类型已选才可提交
const canSubmit = computed(
  () =>
    form.value.kind !== null &&
    form.value.symbol.trim() !== '' &&
    form.value.name.trim() !== '' &&
    !submitting.value,
)

// 打开时重置表单（币种默认人民币；市场固定未知，不设字段），
// immediate 兼容 show 初始即为 true 的挂载（先例：MerchantEditModal）
watch(
  () => props.show,
  (show) => {
    if (!show) return
    form.value = { kind: null, symbol: '', name: '', currencyCode: 'CNY' }
    error.value = null
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

async function submit() {
  if (!canSubmit.value) return
  submitting.value = true
  error.value = null
  try {
    const input: InstrumentInput = {
      symbol: form.value.symbol.trim(),
      type: form.value.kind!,
      name: form.value.name.trim(),
      currency_code: form.value.currencyCode,
      // 市场固定未知（自建标的天然脱离行情同步体系，ADR-0036 决策 3）
      market: null,
    }
    const symbol = input.symbol
    await api.createInstrument(input)
    emit('created', t('investments.create.success', { name: input.name, symbol }))
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
    :title="t('investments.create.title')"
    style="width: 440px"
    :bordered="false"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('investments.create.intro') }}
      </NText>
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NFormItem :label="t('investments.create.typeLabel')" required>
          <AppSelect
            v-model:value="form.kind"
            :options="TYPE_OPTIONS"
            :placeholder="t('investments.create.typePlaceholder')"
            data-testid="create-instrument-type"
            style="width: 100%"
          />
        </NFormItem>
        <NFormItem :label="t('investments.create.symbolLabel')" required>
          <NInput
            v-model:value="form.symbol"
            :placeholder="t('investments.create.symbolPlaceholder')"
            :maxlength="32"
            :disabled="submitting"
            data-testid="create-instrument-symbol"
          />
        </NFormItem>
        <NFormItem :label="t('investments.create.nameLabel')" required>
          <NInput
            v-model:value="form.name"
            :placeholder="t('investments.create.namePlaceholder')"
            :maxlength="64"
            :disabled="submitting"
            data-testid="create-instrument-name"
            @keyup.enter="submit"
          />
        </NFormItem>
        <NFormItem :label="t('investments.create.currencyLabel')">
          <AppSelect
            v-model:value="form.currencyCode"
            :options="currencyOptions"
            filterable
            data-testid="create-instrument-currency"
            style="width: 100%"
          />
        </NFormItem>
      </NForm>
      <NText v-if="error" type="error" data-testid="create-instrument-error">
        {{ error }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton data-testid="cancel-create-instrument" :disabled="submitting" @click="close">
          {{ t('investments.create.cancel') }}
        </NButton>
        <NButton
          type="primary"
          data-testid="submit-create-instrument"
          :loading="submitting"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ t('investments.create.submit') }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
