<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { useReferenceStore } from '@/stores/reference'
import { errorCodeOf, errorMessage as extractErrorMessage } from '@/utils/errors'
import { formatPrice } from '@/types'
import type {
  AddInstrumentChannel,
  AddStockInstrumentResult,
  AddFundResult,
  InstrumentType,
  MarketType,
} from '@/types'

// 「添加投资标的」对话框（issue #697 / spec #690 用户故事 11-16）：标的创建的
// 唯一入口——市场必选录入通道（沪/深/港/美股/场外基金）+ 按代码查询，命中即
// 后端自动识别类型（fund 接口命中 → fund；行情命中 → stock，类型特征 → etf）
// 并回填权威名称与最新价；未命中在对话框内手动建档兜底（名称必填、类型白名单
// 债券/ETF/其他、市场取所选值）。场外基金通道复用既有 add_fund_by_code 命令
// （fund 类型唯一创建入口仍为按代码即拉，语义不变）；股票通道走
// add_instrument_by_code（查询→识别→创建增强）。既有「新建标的」独立弹窗与
// 「添加基金」独立入口随本弹窗收编退役。
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{
  'update:show': [value: boolean]
  /** 添加成功回执文案（页面级展示），列表重拉由父组件负责 */
  added: [message: string]
}>()

const reference = useReferenceStore()

// 市场必选的录入通道闭集（通道标签，非存储市场）：沪/深/港复用市场标签，
// 美股/场外基金是通道语义（美股折叠三交易所、场外基金落 unknown 市场）。
const CHANNEL_OPTIONS = computed(() => [
  { label: t('investments.market.sh'), value: 'sh' as AddInstrumentChannel },
  { label: t('investments.market.sz'), value: 'sz' as AddInstrumentChannel },
  { label: t('investments.market.hk'), value: 'hk' as AddInstrumentChannel },
  { label: t('investments.addInstrument.channelUs'), value: 'us' as AddInstrumentChannel },
  { label: t('investments.addInstrument.channelFund'), value: 'fund' as AddInstrumentChannel },
])

// 兜底建档的类型白名单（与后端 IPC 入口守卫同源，ADR-0036）：股票类标的不
// 手动建（按代码查询承担），基金唯一创建入口归按代码即拉。
const TYPE_OPTIONS = computed(() => [
  { label: t('investments.type.bond'), value: 'bond' as InstrumentType },
  { label: t('investments.type.etf'), value: 'etf' as InstrumentType },
  { label: t('investments.type.other'), value: 'other' as InstrumentType },
])

const market = ref<AddInstrumentChannel | null>(null)
const code = ref('')
const querying = ref(false)
/** 兜底建档态：股票通道查询未命中（查无此码）后展开 */
const fallback = ref(false)
const fallbackName = ref('')
const fallbackType = ref<InstrumentType | null>(null)
const fallbackCurrency = ref('CNY')
const creating = ref(false)
/** 弹窗内错误提示（未命中/临时故障/建档校验）：保持弹窗打开供改码重试 */
const error = ref<string | null>(null)

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} · ${c.name}`, value: c.code })),
)

const codePlaceholder = computed(() =>
  market.value === 'fund'
    ? t('investments.addInstrument.codePlaceholderFund')
    : t('investments.addInstrument.codePlaceholderStock'),
)

// 基金通道 6 位纯数字才可提交（后端同样校验，前端仅提前拦截不发起无效请求）；
// 股票通道代码形态由后端按通道解析（矛盾/不支持显式报错），前端只拦空白。
const codeValid = computed(() => {
  const trimmed = code.value.trim()
  if (trimmed === '') return false
  if (market.value === 'fund') return /^\d{6}$/.test(trimmed)
  return true
})

const canQuery = computed(() => market.value !== null && codeValid.value && !querying.value)
const canCreate = computed(
  () =>
    fallback.value &&
    fallbackName.value.trim() !== '' &&
    fallbackType.value !== null &&
    !creating.value,
)

// 打开时重置表单（币种默认人民币），immediate 兼容 show 初始即为 true 的挂载
//（先例：MerchantEditModal）
watch(
  () => props.show,
  (show) => {
    if (!show) return
    market.value = null
    code.value = ''
    fallback.value = false
    fallbackName.value = ''
    fallbackType.value = null
    fallbackCurrency.value = 'CNY'
    querying.value = false
    creating.value = false
    error.value = null
  },
  { immediate: true },
)

function close() {
  emit('update:show', false)
}

/** 命中回执（识别回显）：股票通道带类型标签与最新价；停牌无价显式说明 */
function stockHitMessage(result: AddStockInstrumentResult): string {
  const price = result.price_cents !== null
    ? t('investments.addInstrument.price', { price: formatPrice(result.price_cents) })
    : t('investments.addInstrument.priceMissing')
  return t('investments.addInstrument.successStock', {
    name: result.name,
    symbol: result.symbol,
    typeLabel: t(`investments.type.${result.type}`),
    price,
  })
}

/** 命中回执（场外基金通道，语义不变）：权威名称 + 分类 + 最新净值 */
function fundHitMessage(result: AddFundResult): string {
  const nav = result.nav_cents !== null && result.nav_date !== null
    ? t('investments.addInstrument.nav', {
        price: formatPrice(result.nav_cents),
        date: result.nav_date,
      })
    : t('investments.addInstrument.navMissing')
  return t('investments.addInstrument.successFund', {
    name: result.name,
    symbol: result.symbol,
    fundClass: result.fund_class,
    nav,
  })
}

async function submitQuery() {
  if (!canQuery.value) return
  querying.value = true
  error.value = null
  try {
    const message =
      market.value === 'fund'
        ? fundHitMessage(await api.addFundByCode(code.value.trim()))
        : stockHitMessage(await api.addInstrumentByCode(market.value!, code.value.trim()))
    emit('added', message)
    close()
  } catch (e) {
    // 股票通道查无此码 → 展开兜底建档；其余错误（含基金通道未命中与临时故障）
    // 只提示。基金未命中不兜底：fund 类型唯一创建入口仍为按代码即拉。
    if (market.value !== 'fund' && errorCodeOf(e) === 'sync.stock-not-found') {
      fallback.value = true
    }
    error.value = extractErrorMessage(e)
  } finally {
    querying.value = false
  }
}

/** 兜底建档市场：显式市场通道透传（市场取所选值）；美股遍历未命中无法预知
 * 交易所归属、场外基金无市场概念，均传 null 走后端缺省 unknown。 */
const fallbackMarket = computed((): MarketType | null =>
  market.value === 'sh' || market.value === 'sz' || market.value === 'hk' ? market.value : null,
)

async function submitFallback() {
  if (!canCreate.value) return
  creating.value = true
  error.value = null
  try {
    const input = {
      symbol: code.value.trim(),
      type: fallbackType.value!,
      name: fallbackName.value.trim(),
      currency_code: fallbackCurrency.value,
      market: fallbackMarket.value,
    }
    await api.createInstrument(input)
    emit(
      'added',
      t('investments.addInstrument.fallbackSuccess', { name: input.name, symbol: input.symbol }),
    )
    close()
  } catch (e) {
    error.value = extractErrorMessage(e)
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="t('investments.addInstrument.title')"
    card-size="md"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('investments.addInstrument.intro') }}
      </NText>
      <NForm label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，不得裸排（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('investments.addInstrument.marketLabel')" required>
            <AppSelect
              v-model:value="market"
              :options="CHANNEL_OPTIONS"
              :placeholder="t('investments.addInstrument.marketPlaceholder')"
              :disabled="fallback || querying || creating"
              data-testid="add-instrument-market"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem :label="t('investments.addInstrument.codeLabel')" required>
            <NInput
              v-model:value="code"
              :placeholder="codePlaceholder"
              :maxlength="32"
              :disabled="querying || creating"
              data-testid="add-instrument-code"
              @keyup.enter="submitQuery"
            />
          </NFormItem>
          <template v-if="fallback">
            <NFormItem :label="t('investments.addInstrument.nameLabel')" required>
              <NInput
                v-model:value="fallbackName"
                :placeholder="t('investments.addInstrument.namePlaceholder')"
                :maxlength="64"
                :disabled="creating"
                data-testid="add-instrument-name"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.typeLabel')" required>
              <AppSelect
                v-model:value="fallbackType"
                :options="TYPE_OPTIONS"
                :placeholder="t('investments.addInstrument.typePlaceholder')"
                :disabled="creating"
                data-testid="add-instrument-type"
                style="width: 100%"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.currencyLabel')">
              <AppSelect
                v-model:value="fallbackCurrency"
                :options="currencyOptions"
                filterable
                :disabled="creating"
                data-testid="add-instrument-currency"
                style="width: 100%"
              />
            </NFormItem>
          </template>
        </NSpace>
      </NForm>
      <NText v-if="fallback" depth="3" data-testid="add-instrument-fallback-hint">
        {{ t('investments.addInstrument.fallbackHint') }}
      </NText>
      <NText v-if="error" type="error" data-testid="add-instrument-error">
        {{ error }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton data-testid="cancel-add-instrument" :disabled="querying || creating" @click="close">
          {{ t('investments.addInstrument.cancel') }}
        </NButton>
        <NButton
          v-if="!fallback"
          type="primary"
          data-testid="submit-add-instrument"
          :loading="querying"
          :disabled="!canQuery"
          @click="submitQuery"
        >
          {{ t('investments.addInstrument.query') }}
        </NButton>
        <NButton
          v-else
          type="primary"
          data-testid="submit-add-instrument-fallback"
          :loading="creating"
          :disabled="!canCreate"
          @click="submitFallback"
        >
          {{ t('investments.addInstrument.create') }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
