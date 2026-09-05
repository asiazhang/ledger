import { defineStore } from 'pinia'
import { ref } from 'vue'
import { presetRange, type DateRange } from '@/utils/time-period'

/** 商户排行 TopN 档位闭集（issue #588）：仅 Top 5 / Top 10 两档，不设「全部」。 */
export const MERCHANT_TOP_N_OPTIONS = [5, 10] as const

/** 商户排行 TopN 默认档：头部不被长尾稀释的最小档位；冷启动回此默认。 */
export const MERCHANT_TOP_N_DEFAULT: number = 5

/**
 * 报表页会话状态 store（issue #427）：会话级、不持久化的界面状态——
 * 持有报表期间快照与图内下钻一级分类 id。
 *
 * 「会话内保留」语义（spec #426）：同一应用会话内，离开报表页再回来
 * （含 Cmd+左回退、侧栏切换）= 回到离开时的样子。store 存活期长于组件挂载期
 * （pinia 实例存活期 = 应用会话），状态提升为会话级后「保留」自然成立；
 * 冷启动（新 pinia）回默认「当年」；不写 localStorage、不写回路由 URL。
 *
 * 「意图进、状态出」的小深模块，规则内化其中（视图不再各持一份）：
 * - 同值守卫：重复设置同段期间不动作（下钻不复位、视图 watch 不触发重拉）；
 * - 期间切换复位图内下钻：下钻是悬在旧期间上的瞬时视图状态，切期间回基础态
 *   （分类下钻词条既有裁决随状态迁入）；
 * - 商户排行 TopN 档位（issue #588）：会话内保留、冷启动回默认，与期间/下钻
 *   互不牵连；切换由视图 watch 以新 top_n 重拉商户卡。
 * 报表视图退为接线：QuickTimeRange 受控 v-model 进出，三卡数据拉取与 loading
 * 仍归视图，进入与期间变化照常重拉。
 */
export const useReportsSessionStore = defineStore('reports-session', () => {
  /** 报表期间快照（YYYY-MM-DD 含边界，ADR-0057 快照语义）：默认 = 会话内首次
   * 使用时按「当年」自然周期派生的快照；跨月/季/年后区间不漂移。 */
  const period = ref<DateRange>(presetRange('year', new Date()))

  /** 图内下钻的一级分类 id：null = 基础态（分类下钻第一段，瞬时视图状态不持久化）。 */
  const drilledRootId = ref<string | null>(null)

  /** 商户排行 TopN 档位（issue #588）：会话内保留、冷启动回默认（ADR-0061 同粒度，
   * 不写盘）；档位闭集二（5/10），默认 5。 */
  const merchantTopN = ref<number>(MERCHANT_TOP_N_DEFAULT)

  /** 期间意图入口：同值守卫与期间切换复位下钻的唯一实现点。
   * 双端有界的精确自然周期快照进；同段期间不动作，不同期间写入快照并复位下钻。 */
  function setPeriod(range: DateRange) {
    if (range.from === period.value.from && range.to === period.value.to) return
    period.value = { from: range.from, to: range.to }
    drilledRootId.value = null
  }

  /** 图内下钻意图入口：一级分类 id 进（null = 点面包屑根回基础态），期间不受牵连。 */
  function setDrilldown(categoryId: string | null) {
    drilledRootId.value = categoryId
  }

  /** 商户排行 TopN 档位意图入口（issue #588）：同档重复写入无副作用
   * （ref 同值不触发订阅，视图 watch 不会重拉）。 */
  function setMerchantTopN(n: number) {
    merchantTopN.value = n
  }

  return {
    period,
    drilledRootId,
    merchantTopN,
    setPeriod,
    setDrilldown,
    setMerchantTopN,
  }
})
