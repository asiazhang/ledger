import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { PhysicalAsset, PhysicalAssetInput, PhysicalAssetList } from '@/types'

/** 实物资产加载状态：`idle` 为初始瞬态（self-init 同步置为 `loading`，外部基本观察不到）。 */
export type PhysicalAssetsStatus = 'idle' | 'loading' | 'ready' | 'error'

/**
 * 实物资产（PhysicalAsset）领域 store（issue #466 / ADR-0063）。
 *
 * 实物资产是大件实物的估值档案（实物资产分域词汇表 `PhysicalAsset`），
 * **不进** `useReferenceStore`（不是可选值字典），拥有自己的单一来源 store
 * （镜像 `usePoliciesStore` 的 push-first 生命周期）：
 * - 首次访问 self-init：store 首次被创建时自动触发一次加载；
 * - 订阅后端 `ledger:changed`：实物资产写入（本 store 或后续写路径）成功后
 *   自动静默重拉（stale-while-revalidate：拉取期间保留旧数据，成功后整体替换）。
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
  const status = ref<PhysicalAssetsStatus>('idle')
  const version = ref(0)

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null

  /** 一次完整重拉：拉取期间保留旧数据，成功后整体替换（避免闪空与部分更新；
   *  任一失败则整体失败，失败信号由 status 承载）。 */
  async function reload(): Promise<void> {
    status.value = 'loading'
    try {
      const list: PhysicalAssetList = await api.listPhysicalAssets()
      assets.value = list.assets
      holdingTotalNativeCents.value = list.holding_total_native_cents
      nativeCurrency.value = list.native_currency
      version.value += 1
      status.value = 'ready'
    } catch (e) {
      status.value = 'error'
      throw e
    }
  }

  /** 在途去重：并发调用（self-init / refresh / create / 事件）合并为同一次加载。 */
  function reloadMerged(): Promise<void> {
    if (inFlight) return inFlight
    inFlight = reload().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  /** 强制刷新（在途时合并，避免 IPC 风暴）。 */
  function refresh(): Promise<void> {
    return reloadMerged()
  }

  /** 建档（估值必填 = 首条估值历史行）：成功后立即重拉
   *  （后端同时发 ledger:changed，事件侧重拉被在途合并）。 */
  async function create(input: PhysicalAssetInput): Promise<string> {
    const id = await api.createPhysicalAsset(input)
    await refresh()
    return id
  }

  /** 在持合计展示文本的折算基准（合计卡与币种符号同源）。 */
  const holdingTotalCurrency = computed(() => nativeCurrency.value)

  // —— push 生命周期 ——
  // 首次访问 self-init：触发一次加载（失败静默，失败信号已由 status 承载）。
  void refresh().catch(() => {
    /* noop */
  })

  // 订阅后端 ledger:changed：实物资产写入即失效 → 静默重拉（stale-while-revalidate）。
  listen('ledger:changed', () => {
    void refresh().catch(() => {
      /* noop：失败信号已由 status 承载 */
    })
  }).catch(() => {
    /* 监听注册失败不阻塞 store（本地事件，极少发生） */
  })

  return {
    assets,
    holdingTotalNativeCents,
    holdingTotalCurrency,
    status,
    version,
    refresh,
    create,
  }
})
