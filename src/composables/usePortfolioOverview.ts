import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { useLoadable } from '@/composables/useLoadable'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { Currency, Holding } from '@/types'

/** 当前持仓概览的一行：Holding 行叠加标的字典与账户的展示信息。 */
export interface PortfolioRow {
  holdingId: string
  accountId: string
  /** 账户名称（参考数据缺失时为 null，展示降级为「-」） */
  accountName: string | null
  instrumentId: string
  symbol: string | null
  instrumentName: string | null
  quantity: number
  costBasisCents: number
  costCurrencyCode: string
  latestPriceCents: number | null
  latestPriceCurrencyCode: string | null
  /** 净值日期：基金现价（= 最新公布单位净值）携带，持仓可见现价对应哪天的净值；股票恒 null */
  latestNavDate: string | null
  /** 账户本位币市值（v_holdings 实时计算；无行情时为 null） */
  marketValueCents: number | null
  /** 账户本位币未实现盈亏（v_holdings 实时计算；无行情/汇率缺失时为 null） */
  unrealizedPnlCents: number | null
  /** 市值/未实现盈亏的折算币种 = 账户币（账户缺失时回退成本币种） */
  valueCurrencyCode: string
}

/** 按币种分组的金额小计 */
export interface CurrencyAmountGroup {
  currencyCode: string
  cents: number
}

/**
 * 把一组 (币种, 可空金额) 按币种累加。
 * 市值/未实现盈亏经 v_holdings 折算到**各账户本位币**，多币种账户无法安全合并成单一数字，
 * 因此按币种分组返回，由展示层逐组格式化。
 */
export function sumByCurrency(
  values: { currencyCode: string; cents: number | null }[],
): CurrencyAmountGroup[] {
  const acc = new Map<string, number>()
  for (const { currencyCode, cents } of values) {
    if (cents === null) continue
    acc.set(currencyCode, (acc.get(currencyCode) ?? 0) + cents)
  }
  return [...acc.entries()]
    .map(([code, cents]) => ({ currencyCode: code, cents }))
    .sort((a, b) => a.currencyCode.localeCompare(b.currencyCode))
}

/**
 * 按币种分组的合计 → 展示文本：逐组格式化后以「 / 」连接；空分组（全部无行情）降级为「-」。
 * 首页投资概览卡与盈亏页持仓概览共用此实现，避免第二份分组展示逻辑。
 */
export function formatCurrencyGroups(
  groups: CurrencyAmountGroup[],
  currencyMap: Map<string, Currency>,
): string {
  if (groups.length === 0) return '-'
  return groups
    .map((g) => formatAmount(g.cents, currencyMap.get(g.currencyCode)))
    .join(' / ')
}

/** 一次拉全「持仓标的」字典的每页条数上限（list_instruments 单页上限） */
const INVESTED_INSTRUMENT_FETCH_LIMIT = 500

/**
 * 盈亏页持仓概览数据层（issue #110 / T6；issue #324 起为 Loadable 之上的薄壳，ADR-0040）：
 * 从 `list_holdings`（v_holdings 视图）取当前持仓行，并与「持仓标的」字典
 * （only_invested=true，与增量同步同口径）及账户信息拼装出可展示的明细与汇总。
 *
 * loading 置收、错误捕获与文案归一、错误展示（默认 toast + error 双通道）、
 * 竞态裁决全部内化进 Loadable；本薄壳只持任务结果（rows）与首跑时序，
 * Promise.all 双请求与行映射逻辑留在发起闭包内。失败不向上抛（首刷/刷新不再
 * 产生未处理 rejection，spec 治愈清单①）：error 置位 + 默认 toast，rows 保持
 * 原值不清空成空态。
 */
export function usePortfolioOverview() {
  const reference = useReferenceStore()

  const rows = ref<PortfolioRow[]>([])

  const { loading, error, run } = useLoadable(async () => {
    const [holdings, invested] = await Promise.all([
      api.listHoldings(),
      // 「持仓标的」字典：有当前持仓批次（remaining_quantity > 0），与增量同步同口径
      api.listInstruments({
        only_invested: true,
        // 持仓标的一般远少于标的总数；list_instruments 单页上限即此值
        page_size: INVESTED_INSTRUMENT_FETCH_LIMIT,
      }),
    ])
    // 行数通常远小于标的数，直接按 id 建 map（O(n+m)）
    const instrumentMap = new Map(invested.items.map((i) => [i.id, i]))
    return holdings.map((h: Holding) => toRow(h, instrumentMap, reference.accountMap))
  })

  async function refresh() {
    const result = await run()
    // 失败回空（error 已置位）：rows 保持原值不清空；迟到前发结果已被 Loadable
    // 竞态裁决作废为空，不会覆写终态
    if (result !== null) rows.value = result
  }

  const totalMarketValueGroups = computed(() =>
    sumByCurrency(rows.value.map((r) => ({ currencyCode: r.valueCurrencyCode, cents: r.marketValueCents }))),
  )
  const totalUnrealizedPnlGroups = computed(() =>
    sumByCurrency(rows.value.map((r) => ({ currencyCode: r.valueCurrencyCode, cents: r.unrealizedPnlCents }))),
  )

  onMounted(() => {
    void refresh()
  })

  return { rows, loading, error, totalMarketValueGroups, totalUnrealizedPnlGroups, refresh }
}

interface InstrumentLike {
  symbol: string
  name: string | null
}

interface AccountLike {
  name: string
  currency_code: string
}

/** Holding 行 + 标的字典 + 账户参考数据 → 概览明细行 */
function toRow(
  h: Holding,
  instrumentMap: Map<string, InstrumentLike>,
  accountMap: Map<string, AccountLike>,
): PortfolioRow {
  const inst = instrumentMap.get(h.instrument_id)
  const acct = accountMap.get(h.account_id)
  return {
    holdingId: h.id,
    accountId: h.account_id,
    accountName: acct?.name ?? null,
    instrumentId: h.instrument_id,
    symbol: inst?.symbol ?? null,
    instrumentName: inst?.name ?? null,
    quantity: h.quantity,
    costBasisCents: h.cost_basis_cents,
    costCurrencyCode: h.cost_currency_code,
    latestPriceCents: h.latest_price_cents,
    latestPriceCurrencyCode: h.latest_price_currency_code,
    latestNavDate: h.latest_nav_date,
    marketValueCents: h.market_value_cents,
    unrealizedPnlCents: h.unrealized_pnl_cents,
    // 市值/未实现盈亏由 v_holdings 折算到账户本位币；账户缺失时回退成本币种保证可展示
    valueCurrencyCode: acct?.currency_code ?? h.cost_currency_code,
  }
}
