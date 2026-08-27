import { onMounted, ref } from 'vue'
import { api } from '@/api'
import type { DashboardOverview } from '@/types'

/**
 * 首页净资产总览数据层（issue #143）：
 * 消费后端 `dashboard_overview` 聚合命令（多币种折算与合计全部在后端完成，
 * 前端不出现第二份口径表达式），视图只做装配渲染。
 *
 * 缺汇率等命令报错不向上抛：转入 error 兜底状态（带后端中文错误信息），
 * 由视图显示提示文案而非空数字或崩溃。
 */
export function useDashboardOverview() {
  const loading = ref(false)
  const overview = ref<DashboardOverview | null>(null)
  const error = ref<string | null>(null)

  async function refresh() {
    loading.value = true
    try {
      overview.value = await api.dashboardOverview()
      error.value = null
    } catch (e) {
      overview.value = null
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    void refresh()
  })

  return { overview, loading, error, refresh }
}
