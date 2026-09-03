import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
import { centsToYuan, priceToYuan, yuanToCents } from '@/types'
import { judgeQuantityText, judgePriceText, fieldErrorKind } from '@/utils/field-error'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import { buildTradeInput } from '@/domain/transaction-input'
import { errorMessage } from '@/utils/errors'
import type {
  Instrument,
  Transaction,
  TransactionTrade,
} from '@/types'

/** 价格刻度换算因子（ADR-0038，与后端 investment::prices::PRICE_UNITS_PER_FEN 同一倍率）：
 * 金额（分）→ 价格（万分之一元）乘 100；价格（万分之一元）→ 元展示值除 10000。 */
const PRICE_UNITS_PER_FEN = 100
const PRICE_UNITS_PER_YUAN = 10000

export function useInvestmentForm(
  kind: 'buy' | 'sell',
  options?: {
    onCreated?: () => void
    /** 编辑模式（issue #180）：更新成功回调。编辑路径与创建路径共用 submit，
     * 按是否存在 editing 目标分派命令；成功后不重置表单（弹窗由父层关闭）。 */
    onUpdated?: () => void
    /** 编辑模式：待编辑交易 getter。与 useCategoryForm editing 同约定：仅在
     * composable 创建时读一次做回填、提交时重读一次定目标，换目标交易必须由
     * 父层强制重建组件实例（:key 序号重建），否则回填/提交仍指向旧交易。 */
    editing?: () => Transaction | null
    /** 编辑模式：待编辑交易的买卖明细 getter（security_transactions 扩展表投影）。
     * 创建时读一次做回填；标的展示字段随明细带出，回填后选择框直接显示
     * symbol · name，不依赖远程搜索候选。 */
    trade?: () => TransactionTrade | null
  },
) {
  const { reference, currencyOptions } = useFormShared()
  const message = useMessage()

  const accountId = ref<string | null>(null)
  const instrumentId = ref<string | null>(null)
  /** 确认单金额（元）：基金申赎的权威输入（issue #302 / ADR-0038 金额权威）；
   * 非基金形态恒 null（金额由后端按数量 × 单价重算，表单只展示）。 */
  const amount = ref<number | null>(null)
  // 数量/价格字段错误态（ADR-0058 / issue #416）：同金额形态（#414/#415 先例）——
  // 以原始文本承载输入（不拦截、不静默丢弃，非法文本原样保留），判定口径走共享单点
  // judgeQuantityText / judgePriceText（数量与价格的精度口径与金额整数分不同：均至多
  // 四位小数——数量同既有 NInputNumber precision=4 约束、价格同万分之一元刻度）；
  // 错误态装配（输入中即时红 / 空值红在失焦或保存尝试后）由本薄层声明时机。
  const quantityText = ref('')
  const priceText = ref('')
  const quantityBlurred = ref(false)
  const priceBlurred = ref(false)
  const saveAttempted = ref(false)
  const fee = ref<number | null>(null)
  const note = ref('')
  const date = ref(Date.now())
  const currencyCode = ref('CNY')

  const instruments = ref<Instrument[]>([])
  const searchingInstruments = ref(false)
  let searchTimer: ReturnType<typeof setTimeout> | undefined

  const investmentAccountOptions = computed(() =>
    reference.accounts
      .filter((a) => a.type === 'investment')
      .map((a) => ({ label: a.name, value: a.id })),
  )

  const instrumentOptions = computed(() => {
    const opts = instruments.value.map((i) => ({
      label: i.name ? `${i.symbol} · ${i.name}` : i.symbol,
      value: i.id,
    }))
    // 编辑回填（issue #180）：待编辑标的合入候选（已含于搜索结果则不重复），
    // 保证打开编辑即显示该标的而非裸 id。
    if (seededInstrumentOption && !opts.some((o) => o.value === seededInstrumentOption.value)) {
      return [seededInstrumentOption, ...opts]
    }
    return opts
  })

  // 编辑回填（issue #180）：打开即回填该笔 buy/sell 全部业务字段。单价经 priceToYuan
  // 按万分之一元刻度换算（ADR-0038，不手写 /10000）；费用为金额，经 centsToYuan
  // 按币种小数位换算；日期以 UTC 午夜时间戳承载回填，提交端日期转换收口装配器
  // toLocalDateISO（issue #216）。明细缺失时（父层未取到 trade）不回填。
  // 基金形态（issue #302）：回填确认单金额为权威输入，单价不回填——展示值由
  // derivedPrice 按金额/份额/费用反算，与存储单价同一公式。
  const editingTx = options?.editing?.() ?? null
  const editingTrade = options?.trade?.() ?? null
  const seededInstrumentIsFund = editingTrade?.instrument_type === 'fund'
  const seededInstrumentOption = editingTrade
    ? {
        label: editingTrade.instrument_name
          ? `${editingTrade.symbol} · ${editingTrade.instrument_name}`
          : editingTrade.symbol,
        value: editingTrade.instrument_id,
      }
    : null
  if (editingTx && editingTrade) {
    accountId.value = editingTx.account_id
    currencyCode.value = editingTx.currency_code
    instrumentId.value = editingTrade.instrument_id
    // 数量/价格以文本形态回填：存储值在本仓录入路径下必在各自精度口径内（数量
    // 经 precision=4 录入、单价万分之一元刻度 ≤四位小数），合法回填不显红态；
    // 极端历史数据（如导入超粒度数量）如实红显，属字段错误态的诚实反馈
    quantityText.value = String(editingTrade.quantity)
    if (seededInstrumentIsFund) {
      amount.value = centsToYuan(
        editingTx.amount_cents,
        reference.getCurrency(editingTx.currency_code),
      )
    } else {
      priceText.value = String(priceToYuan(editingTrade.price_cents))
    }
    fee.value =
      editingTrade.fee_cents != null
        ? centsToYuan(editingTrade.fee_cents, reference.getCurrency(editingTx.currency_code))
        : null
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  /** 选中标的是否场外基金：录入权威形态的开关（基金 = 金额 + 份额必填、单价反算）。
   * 搜索候选按标的字典类型判定；编辑回填候选（尚未被搜索结果覆盖）按明细带出类型。 */
  const isFundInstrument = computed(() => {
    const id = instrumentId.value
    if (id == null) return false
    const found = instruments.value.find((i) => i.id === id)
    if (found) return found.type === 'fund'
    return id === seededInstrumentOption?.value && seededInstrumentIsFund
  })

  // 数量/价格错误态装配：判定 + 时机 → 当前错误类别。价格错误态仅在非基金形态
  // 装配——基金无单价输入面（单价反算只读展示），空文本不构成红态；数量（股数/份额）
  // 两形态共用同一输入面，同规装配。
  const quantityJudgment = computed(() => judgeQuantityText(quantityText.value))
  const priceJudgment = computed(() => judgePriceText(priceText.value))
  const quantityError = computed(() =>
    fieldErrorKind(quantityJudgment.value, {
      touched: quantityBlurred.value,
      saveAttempted: saveAttempted.value,
    }),
  )
  const priceError = computed(() =>
    isFundInstrument.value
      ? null
      : fieldErrorKind(priceJudgment.value, {
          touched: priceBlurred.value,
          saveAttempted: saveAttempted.value,
        }),
  )
  /** 任一字段处于错误态，保存按钮随之禁用（红框＋提交禁用两件同发） */
  const hasFieldError = computed(
    () => quantityError.value != null || priceError.value != null,
  )

  /** 数量失焦：空值红时机输入（touched） */
  function markQuantityBlurred() {
    quantityBlurred.value = true
  }

  /** 单价失焦：空值红时机输入（touched） */
  function markPriceBlurred() {
    priceBlurred.value = true
  }

  /** 判定 ok 时的已解析数量（null = 文本非 ok，供计算/提交消费） */
  const quantityValue = computed(() =>
    quantityJudgment.value.kind === 'ok' ? quantityJudgment.value.value : null,
  )

  /** 判定 ok 时的已解析单价（元；null = 文本非 ok） */
  const priceValue = computed(() =>
    priceJudgment.value.kind === 'ok' ? priceJudgment.value.yuan : null,
  )

  /** 基金反算单价（元）：与后端 prepare 同一公式——(金额 ∓ 手续费) × 100 ÷ 份额，
   * 万分之一元单次舍入。买入减费（净投入）、卖出加费（费在收入外另收）。
   * 非基金形态或输入不完整时为 null（表单此时展示可编辑单价输入框而非反算值）。 */
  const derivedPrice = computed<number | null>(() => {
    if (!isFundInstrument.value) return null
    const qty = quantityValue.value
    if (amount.value == null || qty == null || qty <= 0) return null
    const amountCents = yuanToCents(amount.value)
    if (amountCents == null) return null
    const feeCents = fee.value == null ? 0 : (yuanToCents(fee.value) ?? 0)
    const baseCents = kind === 'buy' ? amountCents - feeCents : amountCents + feeCents
    if (baseCents <= 0) return null
    // 换算倍率命名收口（ADR-0038 价格刻度）：金额（分）→ 价格（万分之一元）乘
    // PRICE_UNITS_PER_FEN；价格（万分之一元）→ 元（展示值）除 PRICE_UNITS_PER_YUAN。
    return Math.round((baseCents * PRICE_UNITS_PER_FEN) / qty) / PRICE_UNITS_PER_YUAN
  })

  const investmentAmount = computed(() => {
    if (quantityValue.value == null || priceValue.value == null) return 0
    const feeValue = fee.value ?? 0
    const raw = kind === 'buy'
      ? quantityValue.value * priceValue.value + feeValue
      : quantityValue.value * priceValue.value - feeValue
    return Math.round(raw * 100) / 100
  })

  /** 远程搜索标的（防抖），不前端全量驻留 */
  function searchInstruments(query: string) {
    clearTimeout(searchTimer)
    searchTimer = setTimeout(async () => {
      if (!query.trim()) {
        instruments.value = []
        return
      }
      searchingInstruments.value = true
      try {
        const res = await api.listInstruments({ search: query.trim(), page_size: 50 })
        instruments.value = res.items
      } catch {
        instruments.value = []
      } finally {
        searchingInstruments.value = false
      }
    }, 300)
  }

  async function submit() {
    // 保存尝试即触发空值兜底红态（fieldErrorKind 的 saveAttempted 输入）
    saveAttempted.value = true
    // 格式类错误（解析失败 / 超精度 / 必填为空）由「红框＋提交禁用」取代旧格式
    // toast（ADR-0058 决策 1/3，#416 数量/价格接入）：错误态下静默中止提交（先于
    // 账户/标的 toast：红框已在字段上呈现，账户提示延后到格式修正后的下次尝试）
    if (quantityError.value != null || priceError.value != null) return
    if (!accountId.value) {
      message.warning(t('investments.form.selectAccount'))
      return
    }
    if (!instrumentId.value) {
      message.warning(t('investments.form.selectInstrument'))
      return
    }
    // 录入权威按标的类型分流（issue #302 / ADR-0038）：基金 = 确认单金额 + 份额必填、
    // 单价反算；其余 = 数量 + 单价必填。
    const fund = isFundInstrument.value
    if (fund && (amount.value == null || amount.value <= 0)) {
      message.warning(
        t(kind === 'buy' ? 'investments.form.inputBuyAmount' : 'investments.form.inputSellAmount'),
      )
      return
    }
    const quantity = quantityValue.value
    if (quantity == null) return // 不可达（错误态已被上方守卫拦截），仅为类型收窄
    // 业务类校验（纯零/负数）保留既有提交 toast 通道，不动（ADR-0058：业务不成立不属字段错误态）
    if (quantity <= 0) {
      message.warning(
        fund
          ? t('investments.form.inputShares')
          : t(kind === 'buy' ? 'investments.form.inputBuyQuantity' : 'investments.form.inputSellQuantity'),
      )
      return
    }
    const price = priceValue.value
    if (!fund) {
      if (price == null) return // 不可达（错误态已被上方守卫拦截），仅为类型收窄
      if (price <= 0) {
        message.warning(
          t(kind === 'buy' ? 'investments.form.inputBuyPrice' : 'investments.form.inputSellPrice'),
        )
        return
      }
    }

    // 编辑目标提交时重读（getter 约定见 options.editing 注释）
    const editing = options?.editing?.() ?? null
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）：创建/编辑共用同一
      // 装配结果（幂等键不可编辑）。基金申赎落权威金额（amount_cents）不落单价；
      // 其余类型金额占位（amount_cents 恒 0）与关联字段 null 占位收口装配器 per-kind
      // 矩阵；非基金成交金额由后端行为层按数量×单价±手续费重算
      const input = buildTradeInput({
        kind,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        instrumentId: instrumentId.value,
        amount: fund ? amount.value : null,
        quantity,
        price: fund ? null : price,
        fee: fee.value,
        note: note.value,
        date: date.value,
      })
      if (editing) {
        await api.updateTransaction(editing.id, input)
        message.success(t('investments.form.saved'))
        // 编辑路径不重置表单：成功即关窗（onUpdated），实例整体销毁
        options?.onUpdated?.()
      } else {
        await api.createTransaction(input)
        message.success(t(kind === 'buy' ? 'investments.form.recordedBuy' : 'investments.form.recordedSell'))
        instrumentId.value = null
        amount.value = null
        quantityText.value = ''
        priceText.value = ''
        // 时机标志同清：弹窗关窗销毁实例前不留潜伏红态（初始为空不红，ADR-0058 决策 2）
        quantityBlurred.value = false
        priceBlurred.value = false
        saveAttempted.value = false
        fee.value = null
        note.value = ''
        options?.onCreated?.()
      }
    } catch (e) {
      message.error(
        t(editing ? 'investments.form.saveFailed' : 'investments.form.recordFailed', {
          message: errorMessage(e),
        }),
      )
    }
  }

  function resetForm() {
    accountId.value = null
    instrumentId.value = null
    amount.value = null
    quantityText.value = ''
    priceText.value = ''
    quantityBlurred.value = false
    priceBlurred.value = false
    saveAttempted.value = false
    fee.value = null
    note.value = ''
    date.value = Date.now()
    currencyCode.value = 'CNY'
  }

  return {
    accountId, instrumentId, amount, quantityText, priceText, fee, note, date, currencyCode,
    quantityError, priceError, hasFieldError, markQuantityBlurred, markPriceBlurred,
    isFundInstrument, derivedPrice, investmentAmount,
    investmentAccountOptions, instrumentOptions, currencyOptions,
    searchingInstruments,
    submit, searchInstruments, resetForm,
  }
}
