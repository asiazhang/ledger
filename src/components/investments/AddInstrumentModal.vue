<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import { api } from '@/api'
import { t } from '@ledger/i18n'
import { useReferenceStore } from '@/stores/reference'
import { errorCodeOf, errorMessage as extractErrorMessage } from '@/utils/errors'
import { formatPrice } from '@ledger/money'
import type {
  AddInstrumentChannel,
  AddStockInstrumentResult,
  AddFundResult,
  InstrumentType,
} from '@ledger/types'

// 「添加投资标的」对话框（issue #697 / spec #690；六通道修订 issue #826）：标的
// 创建的唯一入口——市场必选录入通道（沪/深/港/美股/场外基金/自定义标的）。
// 前五通道按代码查询，命中即后端自动识别类型（fund 接口命中 → fund；行情命中
// → stock，类型特征 → etf）并回填权威名称与最新价；「自定义标的」通道零网络
// 请求直接展开建档表单（代码必填自由文本幂等键、名称必填、类型白名单债券/
// ETF/其他、币种默认 CNY），落库 market=unknown（ADR-0081 口径）、同（代码，
// 类型）复用并更新名称。查询未命中只显式报错并引导切换自定义标的通道，不转
// 建档（全对话框仅一份建档表单）；场外基金通道复用既有 add_fund_by_code 命令
// （fund 类型唯一创建入口仍为按代码即拉，语义不变）。既有「新建标的」独立弹
// 窗与「添加基金」独立入口已收编退役。
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{
  'update:show': [value: boolean]
  /** 添加成功回执文案（页面级展示），列表重拉由父组件负责 */
  added: [message: string]
}>()

const reference = useReferenceStore()

// 市场必选的录入通道闭集（通道标签，非存储市场）：沪/深/港复用市场标签，
// 美股/场外基金/自定义标的是通道语义（美股折叠三交易所、场外基金落 unknown
// 市场、自定义标的零查询直接建档）。
const CHANNEL_OPTIONS = computed(() => [
  { label: t('investments.market.sh'), value: 'sh' as AddInstrumentChannel },
  { label: t('investments.market.sz'), value: 'sz' as AddInstrumentChannel },
  { label: t('investments.market.hk'), value: 'hk' as AddInstrumentChannel },
  { label: t('investments.addInstrument.channelUs'), value: 'us' as AddInstrumentChannel },
  { label: t('investments.addInstrument.channelFund'), value: 'fund' as AddInstrumentChannel },
  { label: t('investments.addInstrument.channelCustom'), value: 'custom' as AddInstrumentChannel },
])

// 自定义标的通道的类型白名单（与后端 IPC 入口守卫同源，ADR-0036）：股票类标
// 的不手动建（按代码查询承担），基金唯一创建入口归按代码即拉。
const TYPE_OPTIONS = computed(() => [
  { label: t('investments.type.bond'), value: 'bond' as InstrumentType },
  { label: t('investments.type.etf'), value: 'etf' as InstrumentType },
  { label: t('investments.type.other'), value: 'other' as InstrumentType },
])

const market = ref<AddInstrumentChannel | null>(null)
const code = ref('')
const querying = ref(false)
// 自定义标的通道建档表单（选中通道即展开，零网络请求）
const customName = ref('')
const customType = ref<InstrumentType | null>(null)
const customCurrency = ref('CNY')
const creating = ref(false)
/** 弹窗内错误提示（未命中/临时故障/建档校验失败）：保持弹窗打开供改码重试 */
const error = ref<string | null>(null)
/** 股票通道查无此码的引导标记：报错文案旁指向自定义标的通道 */
const notFoundGuidance = ref(false)

/** 自定义标的通道：建档表单直开、提交走创建（零网络请求直至提交） */
const isCustom = computed(() => market.value === 'custom')

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} · ${c.name}`, value: c.code })),
)

const codePlaceholder = computed(() => {
  if (isCustom.value) return t('investments.addInstrument.codePlaceholderCustom')
  return market.value === 'fund'
    ? t('investments.addInstrument.codePlaceholderFund')
    : t('investments.addInstrument.codePlaceholderStock')
})

// 基金通道 6 位纯数字才可提交（后端同样校验，前端仅提前拦截不发起无效请求）；
// 股票通道代码形态由后端按通道解析（矛盾/不支持显式报错），前端只拦空白；
// 自定义标的代码是自由文本幂等键，只拦空白（后端 instrument.symbol-required 同规）。
const codeValid = computed(() => {
  const trimmed = code.value.trim()
  if (trimmed === '') return false
  if (market.value === 'fund') return /^\d{6}$/.test(trimmed)
  return true
})

const canQuery = computed(() => market.value !== null && codeValid.value && !querying.value)
const canCreate = computed(
  () =>
    isCustom.value &&
    codeValid.value &&
    customName.value.trim() !== '' &&
    customType.value !== null &&
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
    customName.value = ''
    customType.value = null
    customCurrency.value = 'CNY'
    querying.value = false
    creating.value = false
    error.value = null
    notFoundGuidance.value = false
  },
  { immediate: true },
)

// 切换通道清报错与引导标记：报错归属查询通道，建档表单以干净态展开
watch(market, () => {
  error.value = null
  notFoundGuidance.value = false
})

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
    // 查询未命中/临时故障只显式报错（#826：查询未命中兜底建档分支删除）；
    // 股票通道查无此码追加引导文案指向自定义标的通道。基金未命中不引导：
    // fund 类型唯一创建入口仍为按代码即拉。
    notFoundGuidance.value =
      market.value !== 'fund' && errorCodeOf(e) === 'sync.stock-not-found'
    error.value = extractErrorMessage(e)
  } finally {
    querying.value = false
  }
}

async function submitCustom() {
  if (!canCreate.value) return
  creating.value = true
  error.value = null
  try {
    const input = {
      symbol: code.value.trim(),
      type: customType.value!,
      name: customName.value.trim(),
      currency_code: customCurrency.value,
      // 市场恒未知（ADR-0081 口径）：自定义标的无真实市场，不透传
      market: null,
    }
    await api.createInstrument(input)
    emit(
      'added',
      t('investments.addInstrument.customSuccess', { name: input.name, symbol: input.symbol }),
    )
    close()
  } catch (e) {
    error.value = extractErrorMessage(e)
  } finally {
    creating.value = false
  }
}

/** 主按钮形态随通道分派：自定义标的通道为建档创建，其余通道为按代码查询 */
const primaryLabel = computed(() =>
  isCustom.value ? t('investments.addInstrument.create') : t('investments.addInstrument.query'),
)
const primaryLoading = computed(() => (isCustom.value ? creating.value : querying.value))
const primaryDisabled = computed(() => (isCustom.value ? !canCreate.value : !canQuery.value))

/** 主按钮分派：自定义标的通道提交建档，其余通道提交查询 */
function submitPrimary() {
  if (isCustom.value) void submitCustom()
  else void submitQuery()
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
              :disabled="querying || creating"
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
              @keyup.enter="submitPrimary"
            />
          </NFormItem>
          <template v-if="isCustom">
            <NFormItem :label="t('investments.addInstrument.nameLabel')" required>
              <NInput
                v-model:value="customName"
                :placeholder="t('investments.addInstrument.namePlaceholder')"
                :maxlength="64"
                :disabled="creating"
                data-testid="add-instrument-name"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.typeLabel')" required>
              <AppSelect
                v-model:value="customType"
                :options="TYPE_OPTIONS"
                :placeholder="t('investments.addInstrument.typePlaceholder')"
                :disabled="creating"
                data-testid="add-instrument-type"
                style="width: 100%"
              />
            </NFormItem>
            <NFormItem :label="t('investments.addInstrument.currencyLabel')">
              <AppSelect
                v-model:value="customCurrency"
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
      <NText v-if="isCustom" depth="3" data-testid="add-instrument-custom-hint">
        {{ t('investments.addInstrument.customHint') }}
      </NText>
      <NText v-if="error" type="error" data-testid="add-instrument-error">
        {{ error }}
      </NText>
      <NText v-if="notFoundGuidance" depth="3" data-testid="add-instrument-not-found-hint">
        {{ t('investments.addInstrument.notFoundHint') }}
      </NText>
      <NSpace justify="end" :size="12">
        <NButton data-testid="cancel-add-instrument" :disabled="querying || creating" @click="close">
          {{ t('investments.addInstrument.cancel') }}
        </NButton>
        <NButton
          type="primary"
          data-testid="submit-add-instrument"
          :loading="primaryLoading"
          :disabled="primaryDisabled"
          @click="submitPrimary"
        >
          {{ primaryLabel }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
