import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import type { RealizedPnlSummary } from '@/types'

/// 已实现盈亏概览：账户/标的筛选 + 汇总数据加载（盈亏 tab 的数据层）。
export function useRealizedPnl() {
  const reference = useReferenceStore()

  const loading = ref(false)
  const summary = ref<RealizedPnlSummary | null>(null)
  const selectedAccountId = ref<string | null>(null)
  const selectedInstrumentId = ref<string | null>(null)

  // 账户选项读参考数据单一来源（ledger:changed 信号保持新鲜），不再单独拉取
  const accountOptions = computed(() =>
    reference.accounts.map((a) => ({ label: a.name, value: a.id })),
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

  function onSelectInstrument(value: string | null) {
    selectedInstrumentId.value = value
    selectedInstrumentOption.value =
      searchInstrumentOptions.value.find((o) => o.value === value) ?? null
    refresh()
  }

  async function refresh() {
    loading.value = true
    try {
      const filter: Record<string, string | null> = {}
      if (selectedAccountId.value) filter.account_id = selectedAccountId.value
      if (selectedInstrumentId.value) filter.instrument_id = selectedInstrumentId.value
      summary.value = await api.realizedPnlSummary(
        Object.keys(filter).length > 0 ? filter : undefined,
      )
    } finally {
      loading.value = false
    }
  }

  const totalPnl = computed(() => summary.value?.total_realized_pnl_cents ?? 0)

  onMounted(() => {
    // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
    void refresh()
  })

  return {
    loading,
    summary,
    selectedAccountId,
    selectedInstrumentId,
    accountOptions,
    pnlInstrumentOptions,
    searchingInstruments,
    totalPnl,
    refresh,
    searchInstruments,
    onSelectInstrument,
  }
}
