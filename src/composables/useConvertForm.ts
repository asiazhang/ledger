import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
import { centsToYuan, yuanToCents, PRICE_UNITS_PER_FEN, PRICE_UNITS_PER_YUAN } from '@/types'
import { judgeQuantityText } from '@/utils/field-error'
import { useFieldErrors } from '@/composables/useFieldErrors'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import { buildConvertInput } from '@/domain/transaction-input'
import { errorMessage } from '@/utils/errors'
import type { Instrument, Transaction, TransactionConvert } from '@/types'

/**
 * 基金转换表单 composable（ADR-0099 / issue #979）：一笔转换 = 一条记录、两腿同记录，
 * 无现金腿、不跨账户。表单状态 → 提交的编排收口此处，wire 字段拼装走
 * [`buildConvertInput`]（TransactionInput 装配器，元转分与日期转换的单点口径）。
 *
 * 录入形态（ADR-0099 决策 8）：不做基金/非基金分流——转换天然只有确认单一种形态，
 * 转出/转入两侧的**份额 + 确认单金额**恒为权威输入；两侧单价由确认单金额 ÷ 份额
 * 反算、只读展示（金额权威、单价反算，与场外基金 buy/sell 同款，ADR-0038）。
 *
 * 复用既有接缝：标的远程搜索（与投资表单同款防抖 remote 搜索）、投资账户选择
 * （参考 store 单一派生）、金额元转分（yuanToCents / centsToYuan 单点）、
 * 数量格式判定与错误态装配（judgeQuantityText / useFieldErrors，ADR-0058 字段错误态）。
 *
 * 编辑模式（与 useInvestmentForm 同约定）：待编辑交易的转换读投影
 * （`get_transaction_convert`）回填「A → B」全量信息；仅在 composable 创建时读一次
 * 做回填、提交时重读一次定目标，换目标交易必须由父层强制重建组件实例（:key）。
 */
export function useConvertForm(options?: {
  onCreated?: () => void
  /** 编辑模式：更新成功回调（弹窗由父层关闭，表单不重置）。 */
  onUpdated?: () => void
  /** 编辑模式：待编辑交易 getter。 */
  editing?: () => Transaction | null
  /** 编辑模式：待编辑交易的转换两腿明细 getter（security_transactions 扩展表投影）。 */
  convert?: () => TransactionConvert | null
}) {
  const { reference, currencyOptions } = useFormShared()
  const message = useMessage()

  const accountId = ref<string | null>(null)
  const currencyCode = ref('CNY')
  const outInstrumentId = ref<string | null>(null)
  const inInstrumentId = ref<string | null>(null)
  /** 份额以原始文本承载（不拦截、不静默丢弃，ADR-0058）：判定口径走共享单点。 */
  const outQuantityText = ref('')
  const inQuantityText = ref('')
  /** 确认单金额（元）：两侧金额是列表展示与多腿分摊的口径输入（ADR-0099）。 */
  const outAmount = ref<number | null>(null)
  const inAmount = ref<number | null>(null)
  const fee = ref<number | null>(null)
  /** 结转成本（元，只读展示）：仅编辑回填时在场——行金额锚点，服务端按 FIFO 消耗算定。 */
  const carriedCost = ref<number | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  const outInstruments = ref<Instrument[]>([])
  const inInstruments = ref<Instrument[]>([])
  const searchingOut = ref(false)
  const searchingIn = ref(false)
  let outSearchTimer: ReturnType<typeof setTimeout> | undefined
  let inSearchTimer: ReturnType<typeof setTimeout> | undefined

  // 投资账户谓词单点收口在参考 store（与盈亏页/投资表单下拉同源）。
  const investmentAccountOptions = computed(() =>
    reference.investmentAccounts.map((a) => ({ label: a.name, value: a.id })),
  )

  /** 编辑回填（issue #979）：打开即回填两腿全部业务字段；两侧单价只读反算不回填
   *（展示值由 derivedOutPrice / derivedInPrice 按金额/份额同式反算）。
   * 数量以文本形态回填（存储值在录入路径下必在四位小数粒度内），日期以 UTC 午夜
   * 时间戳承载（提交端由装配器 toLocalDateISO 收口）。 */
  const editingTx = options?.editing?.() ?? null
  const editingConvert = options?.convert?.() ?? null
  const seededOutOption = editingConvert
    ? {
        label: displayLabel(editingConvert.out_symbol, editingConvert.out_instrument_name),
        value: editingConvert.out_instrument_id,
      }
    : null
  const seededInOption = editingConvert
    ? {
        label: displayLabel(editingConvert.in_symbol, editingConvert.in_instrument_name),
        value: editingConvert.in_instrument_id,
      }
    : null
  if (editingTx && editingConvert) {
    accountId.value = editingTx.account_id
    currencyCode.value = editingTx.currency_code
    outInstrumentId.value = editingConvert.out_instrument_id
    inInstrumentId.value = editingConvert.in_instrument_id
    outQuantityText.value = String(editingConvert.out_quantity)
    inQuantityText.value = String(editingConvert.in_quantity)
    outAmount.value = centsToYuan(
      editingConvert.out_amount_cents,
      reference.getCurrency(editingTx.currency_code),
    )
    inAmount.value = centsToYuan(
      editingConvert.in_amount_cents,
      reference.getCurrency(editingTx.currency_code),
    )
    fee.value =
      editingConvert.fee_cents != null
        ? centsToYuan(editingConvert.fee_cents, reference.getCurrency(editingTx.currency_code))
        : null
    carriedCost.value = centsToYuan(
      editingConvert.carried_cost_cents,
      reference.getCurrency(editingTx.currency_code),
    )
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  /** 标的候选：远程搜索结果 + 编辑回填项（保证打开编辑即显示标的而非裸 id）。 */
  function optionsWithSeed(
    items: Instrument[],
    seed: { label: string; value: string } | null,
  ): { label: string; value: string }[] {
    const opts = items.map((i) => ({ label: displayLabel(i.symbol, i.name), value: i.id }))
    if (seed && !opts.some((o) => o.value === seed.value)) return [seed, ...opts]
    return opts
  }
  const outInstrumentOptions = computed(() =>
    optionsWithSeed(outInstruments.value, seededOutOption),
  )
  const inInstrumentOptions = computed(() => optionsWithSeed(inInstruments.value, seededInOption))

  // 两侧份额错误态装配（ADR-0058 / issue #979 → #1007 收口）：判定走纯函数单点
  // judgeQuantityText、装配走表单级工厂 useFieldErrors——两腿各一行字段声明。
  const errors = useFieldErrors({
    out: { text: outQuantityText, judge: judgeQuantityText },
    in: { text: inQuantityText, judge: judgeQuantityText },
  })
  const {
    error: outQuantityError,
    value: outQuantityValue,
    markBlurred: markOutBlurred,
  } = errors.fields.out
  const {
    error: inQuantityError,
    value: inQuantityValue,
    markBlurred: markInBlurred,
  } = errors.fields.in
  /** 任一字段处于错误态，保存按钮随之禁用（红框＋提交禁用两件同发）。 */
  const hasFieldError = errors.hasError

  /** 两侧反算单价（元，只读展示）：与后端 prepare_convert 同一公式——确认单金额 ÷ 份额，
   * 万分之一元单次舍入。输入不完整时为 null（占位显示）。 */
  function derivedPrice(amount: number | null, quantity: number | null): number | null {
    if (amount == null || quantity == null || quantity <= 0) return null
    const cents = yuanToCents(amount)
    if (cents == null || cents <= 0) return null
    return Math.round((cents * PRICE_UNITS_PER_FEN) / quantity) / PRICE_UNITS_PER_YUAN
  }
  const derivedOutPrice = computed(() => derivedPrice(outAmount.value, outQuantityValue.value))
  const derivedInPrice = computed(() => derivedPrice(inAmount.value, inQuantityValue.value))

  /** 远程搜索标的（防抖），两个选择框各自独立候选：拼音过滤由后端统一模糊语义完成。 */
  async function runSearch(
    query: string,
    target: typeof outInstruments,
    searching: typeof searchingOut,
  ) {
    if (!query.trim()) {
      target.value = []
      return
    }
    searching.value = true
    try {
      const res = await api.listInstruments({ search: query.trim(), page_size: 50 })
      target.value = res.items
    } catch {
      target.value = []
    } finally {
      searching.value = false
    }
  }
  function searchOutInstruments(query: string) {
    clearTimeout(outSearchTimer)
    outSearchTimer = setTimeout(() => void runSearch(query, outInstruments, searchingOut), 300)
  }
  function searchInInstruments(query: string) {
    clearTimeout(inSearchTimer)
    inSearchTimer = setTimeout(() => void runSearch(query, inInstruments, searchingIn), 300)
  }

  async function submit() {
    errors.markSaveAttempted()
    if (hasFieldError.value) return
    if (!accountId.value) {
      message.warning(t('investments.form.selectAccount'))
      return
    }
    if (!outInstrumentId.value) {
      message.warning(t('investments.form.selectConvertOutInstrument'))
      return
    }
    if (!inInstrumentId.value) {
      message.warning(t('investments.form.selectConvertInInstrument'))
      return
    }
    if (outInstrumentId.value === inInstrumentId.value) {
      message.warning(t('investments.form.sameConvertInstrument'))
      return
    }
    if (outAmount.value == null || outAmount.value <= 0) {
      message.warning(t('investments.form.inputConvertOutAmount'))
      return
    }
    if (inAmount.value == null || inAmount.value <= 0) {
      message.warning(t('investments.form.inputConvertInAmount'))
      return
    }
    const outQuantity = outQuantityValue.value
    const inQuantity = inQuantityValue.value
    if (outQuantity == null || inQuantity == null) return // 不可达：错误态已被上方守卫拦截
    if (outQuantity <= 0) {
      message.warning(t('investments.form.inputConvertOutShares'))
      return
    }
    if (inQuantity <= 0) {
      message.warning(t('investments.form.inputConvertInShares'))
      return
    }

    const editing = options?.editing?.() ?? null
    try {
      const input = buildConvertInput({
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        outInstrumentId: outInstrumentId.value,
        outQuantity,
        outAmount: outAmount.value,
        inInstrumentId: inInstrumentId.value,
        inQuantity,
        inAmount: inAmount.value,
        fee: fee.value,
        note: note.value,
        date: date.value,
      })
      if (editing) {
        await api.updateTransaction(editing.id, input)
        message.success(t('investments.form.saved'))
        options?.onUpdated?.()
      } else {
        await api.createTransaction(input)
        message.success(t('investments.form.recordedConvert'))
        resetForm()
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
    outInstrumentId.value = null
    inInstrumentId.value = null
    outQuantityText.value = ''
    inQuantityText.value = ''
    errors.reset()
    outAmount.value = null
    inAmount.value = null
    fee.value = null
    note.value = ''
    date.value = Date.now()
    currencyCode.value = 'CNY'
  }

  /** 标的展示标签：代码 · 名称（无名称退化为裸代码，与来源列同口径）。 */
  function displayLabel(symbol: string, name: string | null): string {
    return name ? `${symbol} · ${name}` : symbol
  }

  return {
    accountId,
    currencyCode,
    outInstrumentId,
    inInstrumentId,
    outQuantityText,
    inQuantityText,
    outAmount,
    inAmount,
    fee,
    carriedCost,
    note,
    date,
    outQuantityError,
    inQuantityError,
    hasFieldError,
    markOutBlurred,
    markInBlurred,
    derivedOutPrice,
    derivedInPrice,
    investmentAccountOptions,
    outInstrumentOptions,
    inInstrumentOptions,
    currencyOptions,
    searchingOut,
    searchingIn,
    submit,
    searchOutInstruments,
    searchInInstruments,
    resetForm,
  }
}
