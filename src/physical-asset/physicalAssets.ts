import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '@ledger/api'
import { createPushFirstList } from '@/composables/push-first-list'
import type {
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
} from '@ledger/types'

/**
 * 实物资产（PhysicalAsset）领域 store（issue #466 / ADR-0064）。
 *
 * 实物资产是大件实物的估值档案（实物资产分域词汇表 `PhysicalAsset`），
 * **不进** `useReferenceStore`（不是可选值字典），拥有自己的单一来源 store。
 *
 * 清单生命周期（self-init / `ledger:changed` 失效重拉 / stale-while-revalidate
 * 整体替换 / 在途合并 / status / version）内化在 push-first 工厂单点
 * （`createPushFirstList`，ADR-0123）；本店只留快照落位（列表 + 在持合计同源
 * 快照）、筛选参数（statusFilter 闭包进 load，#468 T3）与领域动作。
 *
 * 列表只含未删除资产，默认口径 = 在持（「列表默认只看在持资产」，处置 /
 * 软删过滤由 T3 承接）；顶部合计消费后端同源在持估值合计（折本位币，
 * Amount 接缝当期汇率，缺汇率后端整体报错上抛——前端不做二次折算）。
 */
export const usePhysicalAssetsStore = defineStore('physicalAssets', () => {
  const assets = ref<PhysicalAsset[]>([])
  /** 在持估值合计（本位币，分）与折算基准币种（后端列表同源快照）。 */
  const holdingTotalNativeCents = ref(0)
  const nativeCurrency = ref('')
  /** 状态筛选（issue #468 T3）：默认只看在持；「已处置」筛选回看完整档案。
   *  在持合计口径与筛选无关（后端恒算在持，回看已处置时合计不变）。 */
  const statusFilter = ref<'holding' | 'disposed'>('holding')

  const { status, version, refresh } = createPushFirstList(
    () => api.listPhysicalAssets(statusFilter.value),
    (list) => {
      assets.value = list.assets
      holdingTotalNativeCents.value = list.holding_total_native_cents
      nativeCurrency.value = list.native_currency
    },
  )

  /** 切换状态筛选（issue #468 T3）：在持 / 已处置，切换后立即按新筛选重拉；
   *  后续 ledger:changed 信号重拉沿用当前筛选。 */
  async function setStatusFilter(filter: 'holding' | 'disposed'): Promise<void> {
    if (statusFilter.value === filter) return
    statusFilter.value = filter
    await refresh()
  }

  /** 建档（估值必填 = 首条估值历史行）：写入成功即返回 id，不因重拉失败
   *  反转为「保存失败」（数据已落库，重复提交才是真错）；列表刷新由后端
   *  同步发出的 ledger:changed 信号驱动重拉兜底，失败信号由 status 承载。 */
  async function create(input: PhysicalAssetInput): Promise<string> {
    const id = await api.createPhysicalAsset(input)
    await refresh().catch(() => {
      /* 重拉失败不阻断建档成功路径 */
    })
    return id
  }

  /** 编辑档案（issue #467 T2）：仅名称 / 购买信息；写入成功即返回（不因重拉
   *  失败反转为「保存失败」，重拉由 ledger:changed 信号兜底，同 create）。 */
  async function update(id: string, input: PhysicalAssetUpdateInput): Promise<void> {
    await api.updatePhysicalAsset(id, input)
    await refresh().catch(() => {
      /* 重拉失败不阻断编辑成功路径 */
    })
  }

  /** 更新估值（issue #467 T2）：追加一条估值历史行（旧值保留不覆盖），当前
   *  估值变为最新一条；写入成功即返回（重拉失败语义同上）。 */
  async function updateValuation(id: string, input: PhysicalAssetValuationInput): Promise<void> {
    await api.updatePhysicalAssetValuation(id, input)
    await refresh().catch(() => {
      /* 重拉失败不阻断更新成功路径 */
    })
  }

  /** 处置（issue #468 T3）：状态标记转已处置 + 处置信息纯记录（处置日期必填、
   *  处置价 + 币种可选成对，后端守卫）；写入成功即返回（重拉失败语义同上），
   *  资产退出默认列表与在持合计由后端读口径自然生效。 */
  async function dispose(id: string, input: PhysicalAssetDisposeInput): Promise<void> {
    await api.disposePhysicalAsset(id, input)
    await refresh().catch(() => {
      /* 重拉失败不阻断处置成功路径 */
    })
  }

  /** 软删除（issue #468 T3）：数据与估值历史保留，资产退出列表与合计；
   *  写入成功即返回（重拉失败语义同上）。 */
  async function remove(id: string): Promise<void> {
    await api.deletePhysicalAsset(id)
    await refresh().catch(() => {
      /* 重拉失败不阻断删除成功路径 */
    })
  }

  // push 生命周期（self-init 与 ledger:changed 订阅）由工厂内化（ADR-0123）。

  return {
    assets,
    holdingTotalNativeCents,
    nativeCurrency,
    status,
    version,
    statusFilter,
    refresh,
    setStatusFilter,
    create,
    update,
    updateValuation,
    dispose,
    remove,
  }
})
