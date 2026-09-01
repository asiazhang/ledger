import { onMounted, ref } from 'vue'
import { api } from '@/api'
import { useLoadable } from '@/composables/useLoadable'
import type { DashboardOverview } from '@/types'

/**
 * 首页净资产总览数据层（issue #143；issue #323 起为 Loadable 之上的薄壳，ADR-0040）：
 * 消费后端 `dashboard_overview` 聚合命令（多币种折算与合计全部在后端完成，
 * 前端不出现第二份口径表达式），视图只做装配渲染。
 *
 * loading 置收、错误捕获与文案归一、错误展示（默认 toast + error 双通道）、
 * 竞态裁决全部内化进 Loadable；本薄壳只持任务结果（overview）与首跑时序。
 * 缺汇率等命令报错不向上抛：转入 error 兜底状态（带后端中文错误信息），
 * 由视图显示提示文案而非空数字或崩溃。
 */
export function useDashboardOverview() {
  const overview = ref<DashboardOverview | null>(null)

  const { loading, error, run } = useLoadable(() => api.dashboardOverview())

  async function refresh() {
    overview.value = await run()
  }

  onMounted(() => {
    void refresh()
  })

  return { overview, loading, error, refresh }
}
