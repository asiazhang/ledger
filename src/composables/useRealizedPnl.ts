import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { useLoadable } from '@/composables/useLoadable'
import type { CurrencyAmountGroup } from '@/composables/usePortfolioOverview'
import { useReferenceStore } from '@/stores/reference'
import type { RealizedPnlSummary } from '@/types'

/**
 * 已实现盈亏概览：账户/标的筛选 + 汇总数据加载（盈亏 tab 的数据层）。
 *
 * issue #325 起为 Loadable 之上的薄壳（ADR-0040）：loading 置收、错误捕获与文案归一、
 * 错误展示（默认 toast + error 双通道）、竞态裁决全部内化进 Loadable；refresh 为 0 元
 * 发起，闭包内自读当前账户/标的筛选。刷新失败不再静默/产生未处理 rejection（spec 治愈
 * 清单①）：error 置位 + 默认 toast，summary 保持原值不清空成空态。
 * 防抖远程标的搜索与其刻意吞错是「刻意静默不收编」的合法形态（词汇表 Loadable 边界），
 * 保持原样不迁入。
 */
export function useRealizedPnl() {
  const reference = useReferenceStore()

  const summary = ref<RealizedPnlSummary | null>(null)
  const selectedAccountId = ref<string | null>(null)
  const selectedInstrumentId = ref<string | null>(null)

  // 账户选项：投资账户谓词单点在参考 store（与投资录入表单同源，词汇表
  // RealizedPnl 词条），非投资账户不进选项面；隐藏投资账户保留（隐藏 ≠ 软删）。
  const accountOptions = computed(() =>
    reference.investmentAccounts.map((a) => ({ label: a.name, value: a.id })),
  )

  // 标的筛选下拉（远程搜索，不前端全量驻留）
  const searchInstrumentOptions = ref<{ label: string; value: string }[]>([])
  const selectedInstrumentOption = ref<{ label: string; value: string } | null>(null)
  const searchingInstruments = ref(false)
  let instrumentSearchTimer: ReturnType<typeof setTimeout> | undefined

  const pnlInstrumentOptions = computed(() => {
    const opts = [...searchInstrumentOptions.value]
    const sel = selectedInstrumentOption.value
    if (sel && !opts.some((o) => o.value === sel.value)) {
      opts.push(sel)
    }
    return opts
  })

  async function searchInstruments(query: string) {
    clearTimeout(instrumentSearchTimer)
    instrumentSearchTimer = setTimeout(async () => {
      if (!query.trim()) {
        searchInstrumentOptions.value = []
        return
      }
      searchingInstruments.value = true
      try {
        const res = await api.listInstruments({ search: query.trim(), page_size: 50 })
        searchInstrumentOptions.value = res.items.map((i) => ({
          label: `${i.symbol}${i.name ? ` · ${i.name}` : ''}`,
          value: i.id,
        }))
      } catch {
        searchInstrumentOptions.value = []
      } finally {
        searchingInstruments.value = false
      }
    }, 300)
  }

  const { loading, error, run } = useLoadable(async () => {
    // 0 元闭包自读当前筛选：发起时点即最新筛选，无需传参
    const filter: Record<string, string | null> = {}
    if (selectedAccountId.value) filter.account_id = selectedAccountId.value
    if (selectedInstrumentId.value) filter.instrument_id = selectedInstrumentId.value
    return api.realizedPnlSummary(Object.keys(filter).length > 0 ? filter : undefined)
  })

  async function refresh() {
    const result = await run()
    // 失败回空（error 已置位）：summary 保持原值不清空；迟到前发结果已被 Loadable
    // 竞态裁决作废为空，不会覆写终态
    if (result !== null) summary.value = result
  }

  function onSelectInstrument(value: string | null) {
    selectedInstrumentId.value = value
    selectedInstrumentOption.value =
      searchInstrumentOptions.value.find((o) => o.value === value) ?? null
    void refresh()
  }

  // 总盈亏按币种分组（ADR-0107 决策 6）：后端按匹配行币种分组返回，不做跨币种折算，
  // 展示层经 formatCurrencyGroups 逐组格式化（与持仓页签合计同款）。
  const totalGroups = computed<CurrencyAmountGroup[]>(() =>
    (summary.value?.total ?? []).map((g) => ({
      currencyCode: g.currency_code,
      cents: g.realized_pnl_cents,
    })),
  )

  onMounted(() => {
    // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
    void refresh()
  })

  return {
    loading,
    summary,
    error,
    selectedAccountId,
    selectedInstrumentId,
    accountOptions,
    pnlInstrumentOptions,
    searchingInstruments,
    totalGroups,
    refresh,
    searchInstruments,
    onSelectInstrument,
  }
}
