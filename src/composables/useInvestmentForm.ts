import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan, priceToYuan } from '@/types'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import { buildTradeInput } from '@/domain/transaction-input'
import { errorMessage } from '@/utils/errors'
import type {
  Instrument,
  Transaction,
  TransactionTrade,
} from '@/types'

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
  const quantity = ref<number | null>(null)
  const price = ref<number | null>(null)
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
  const editingTx = options?.editing?.() ?? null
  const editingTrade = options?.trade?.() ?? null
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
    quantity.value = editingTrade.quantity
    price.value = priceToYuan(editingTrade.price_cents)
    fee.value =
      editingTrade.fee_cents != null
        ? centsToYuan(editingTrade.fee_cents, reference.getCurrency(editingTx.currency_code))
        : null
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  const investmentAmount = computed(() => {
    if (quantity.value == null || price.value == null) return 0
    const feeValue = fee.value ?? 0
    const raw = kind === 'buy'
      ? quantity.value * price.value + feeValue
      : quantity.value * price.value - feeValue
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
    if (!accountId.value) {
      message.warning('请选择投资账户')
      return
    }
    if (!instrumentId.value) {
      message.warning('请选择标的')
      return
    }
    if (quantity.value == null || quantity.value <= 0) {
      message.warning(kind === 'buy' ? '请输入买入数量' : '请输入卖出数量')
      return
    }
    if (price.value == null || price.value <= 0) {
      message.warning(kind === 'buy' ? '请输入买入单价' : '请输入卖出单价')
      return
    }

    // 编辑目标提交时重读（getter 约定见 options.editing 注释）
    const editing = options?.editing?.() ?? null
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）：创建/编辑共用同一
      // 装配结果（幂等键不可编辑）。buy/sell 金额占位（amount_cents 恒 0）与关联
      // 字段 null 占位收口装配器 per-kind 矩阵；成交金额由后端行为层按数量×单价±手续费重算
      const input = buildTradeInput({
        kind,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        instrumentId: instrumentId.value,
        quantity: quantity.value,
        price: price.value,
        fee: fee.value,
        note: note.value,
        date: date.value,
      })
      if (editing) {
        await api.updateTransaction(editing.id, input)
        message.success('已保存修改')
        // 编辑路径不重置表单：成功即关窗（onUpdated），实例整体销毁
        options?.onUpdated?.()
      } else {
        await api.createTransaction(input)
        message.success(kind === 'buy' ? '已记买入' : '已记卖出')
        instrumentId.value = null
        quantity.value = null
        price.value = null
        fee.value = null
        note.value = ''
        options?.onCreated?.()
      }
    } catch (e) {
      message.error(editing ? `保存失败: ${errorMessage(e)}` : `记账失败: ${errorMessage(e)}`)
    }
  }

  function resetForm() {
    accountId.value = null
    instrumentId.value = null
    quantity.value = null
    price.value = null
    fee.value = null
    note.value = ''
    date.value = Date.now()
    currencyCode.value = 'CNY'
  }

  return {
    accountId, instrumentId, quantity, price, fee, note, date, currencyCode,
    investmentAmount, investmentAccountOptions, instrumentOptions, currencyOptions,
    searchingInstruments,
    submit, searchInstruments, resetForm,
  }
}
