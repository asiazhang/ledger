import { onMounted, ref, watch } from 'vue'
import { api } from '@/api'
import type { ItemDailyTotal } from '@/types'
import { useItemsStore } from '@/stores/items'

/**
 * dashboard「物品使用成本」汇总卡数据层（issue #122）：
 * 消费后端 `item_daily_total` 聚合命令（多币种折算与合计全部在后端完成，
 * 前端不出现第二份口径表达式），视图只做装配渲染。
 *
 * 失效复用物品 store 的重拉节奏：物品写入后 store 经 `ledger:changed` 重拉并
 * 自增 `version`，本 composable 监听 `version` 跟随重拉合计——不重复订阅
 * `ledger:changed`（单一失效信号源）。
 *
 * 缺汇率等命令报错不向上抛：转入 error 兜底状态（带后端中文错误信息），
 * 由视图显示提示文案而非空数字或崩溃（与 `useDashboardOverview` 同款取舍）。
 */
export function useItemDailyTotal() {
  const itemsStore = useItemsStore()
  const total = ref<ItemDailyTotal | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function refresh() {
    loading.value = true
    try {
      total.value = await api.itemDailyTotal()
      error.value = null
    } catch (e) {
      total.value = null
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    void refresh()
  })
  watch(
    () => itemsStore.version,
    () => {
      void refresh()
    },
  )

  return { total, loading, error, refresh }
}
