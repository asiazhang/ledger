import { onMounted, ref } from 'vue'
import { api } from '@/api'
import { useLoadable } from '@/composables/useLoadable'
import type { FinancialFreedomOverview } from '@/types'

/**
 * dashboard「财务自由度」卡数据层（issue #344；口径见 ADR-0048）：消费后端
 * `financial_freedom` 聚合命令——分子折算、分母年化与 3% 提取率全在后端完成，
 * 前端不出现第二份口径表达式，视图只做装配渲染与阶段标签派生。
 *
 * 取数时机与概览页既有卡片一致：挂载首跑、导航返回（视图重挂载）即最新；
 * 价格失效信号订阅维持既有边界不扩（仪表盘订阅是既定的后续迭代），
 * 买卖/录价/编辑预算的即时性由导航返回重挂载兜底。
 *
 * loading 置收、错误捕获与文案归一、错误展示（默认 toast + error 双通道）、
 * 竞态裁决全部内化进 Loadable（ADR-0040，物品日均成本同款薄壳）。缺汇率等
 * 命令报错不向上抛：转入 error 兜底状态（带后端中文错误信息），由视图显示
 * 卡内警告并可重试（重试即再次 refresh）。
 */
export function useFinancialFreedom() {
  const data = ref<FinancialFreedomOverview | null>(null)

  const { loading, error, run } = useLoadable(() => api.financialFreedom())

  async function refresh() {
    data.value = await run()
  }

  onMounted(() => {
    void refresh()
  })

  return { data, loading, error, refresh }
}
