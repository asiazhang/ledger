import { onMounted, ref } from 'vue'
import { h, type VNode } from 'vue'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { formatRate } from '@ledger/money'
import { pnlSemanticColor } from '@ledger/theme/semantic-colors'
import type { Theme } from '@ledger/theme'
import { useLoadable } from '@/composables/useLoadable'
import { usePricesChanged } from '@/composables/usePricesChanged'
import type { MoneyWeightedReturnSummary, MwrBasis } from '@ledger/types'

/**
 * 收益率单元格三态渲染单点（issue #1195 / ADR-0115）：行缺失（缺价跳过）→
 * 「-」（与缺价行金额列空值语义一致）；现金流无解 →「无法计算」（显式标注、
 * 不猜解）；可计算 → 盈亏涨跌色百分数。持仓页收益率列与盈亏页收益率卡共用
 * 同一形态，不各写一份三态分流。
 *
 * 口径标注（issue #1343 / ADR-0115 修订）：含期初存量（补记存量持仓、真实建仓
 * 时点未知）的标的走**未年化**口径，`basis` 非 `annualized` 时在百分数后补一句
 * 「未年化」——两种口径不可互算，不给标注会让用户把 3.16% 读成年化值。
 */
export function renderMwrRateCell(
  rate: number | null | undefined,
  theme: Theme,
  basis: MwrBasis = 'annualized',
): string | VNode {
  if (rate === undefined) return '-'
  if (rate === null) return t('investments.pnl.notComputable')
  const label = basis === 'annualized' ? '' : t('investments.pnl.cumulativeSuffix')
  return h('span', { style: { color: pnlSemanticColor(rate, theme) } }, formatRate(rate) + label)
}

/**
 * 资金加权收益率数据层（issue #1195 / ADR-0115）：`money_weighted_return_summary`
 * 之上的 Loadable 薄壳（ADR-0040 同款形态）——loading 置收、错误捕获与竞态裁决
 * 内化进 Loadable，本模块只持任务结果与首跑时序。
 *
 * 期末市值是收益率的输入之一，随行情变动：价格失效信号驱动重拉
 * （ADR-0031，调用方无需记得手动刷新；订阅在本接缝内化，持仓页签已有的
 * 同信号订阅互不影响——消费方自选订阅是信号机制的既有口径）。
 *
 * 查询语义（缺省无区间 = 自首笔流水起算、截至今日现值）；三个消费面共用
 * 同一次请求：持仓页签逐行取 `by_instrument`（按 账户 × 标的 定位），盈亏页
 * 取 `by_account` 与 `total`。行缺失 = 缺价跳过（展示「-」）；`rate` 为 null =
 * 现金流无解（展示「无法计算」，不猜解——ADR-0115 代价 1）。
 */
export function useMoneyWeightedReturn() {
  const summary = ref<MoneyWeightedReturnSummary | null>(null)

  const { loading, error, run } = useLoadable(async () => api.moneyWeightedReturnSummary())

  async function refresh() {
    const result = await run()
    // 失败回空（error 已置位）：summary 保持原值不清空成空态；迟到前发结果已被
    // Loadable 竞态裁决作废为空，不会覆写终态
    if (result !== null) summary.value = result
  }

  usePricesChanged(() => {
    void refresh()
  })

  onMounted(() => {
    void refresh()
  })

  /** 单标的行收益率（账户 × 标的 定位）：行缺失（缺价跳过）为 undefined、
   * 无解为 null、可计算为数值——三态由展示层分流（「-」/「无法计算」/百分比）。 */
  function instrumentRate(accountId: string, instrumentId: string): number | null | undefined {
    return summary.value?.by_instrument.find(
      (r) => r.account_id === accountId && r.instrument_id === instrumentId,
    )?.rate
  }

  /** 单标的行（账户 × 标的 定位，含口径）：行缺失为 undefined——展示层据 `basis`
   * 标注年化 / 未年化（issue #1343）。 */
  function instrumentMwr(
    accountId: string,
    instrumentId: string,
  ): { basis: MwrBasis; rate: number | null } | undefined {
    const row = summary.value?.by_instrument.find(
      (r) => r.account_id === accountId && r.instrument_id === instrumentId,
    )
    return row === undefined ? undefined : { basis: row.basis, rate: row.rate }
  }

  return { loading, error, summary, refresh, instrumentRate, instrumentMwr }
}
