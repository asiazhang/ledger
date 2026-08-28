import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { ItemInput, ItemWithDailyCost } from '@/types'

/** 物品加载状态：`idle` 为初始瞬态（self-init 同步置为 `loading`，外部基本观察不到）。 */
export type ItemsStatus = 'idle' | 'loading' | 'ready' | 'error'

/**
 * 物品（Item）领域 store（issue #116）。
 *
 * 物品是独立领域实体（CONTEXT.md `Item` / ADR-0014），**不进** `useReferenceStore`
 * （那不是"可选值字典"），拥有自己的单一来源 store。
 *
 * 生命周期（push-first，镜像 `useReferenceStore`）：
 * - 首次访问 self-init：store 首次被创建时自动触发一次加载；
 * - 订阅后端 `ledger:changed`：物品写入（本 store 或后续其他写路径）成功后自动重拉
 *   （stale-while-revalidate：拉取期间保留旧数据，成功后整体替换，不闪空）。
 *
 * 每天成本随日历天数实时变化（无事件），列表展示时刻快照由后端计算；
 * 事件驱动的重拉保证写入后即刻可见，无需新鲜度窗口。
 *
 * 失效信号：`status`（idle/loading/ready/error）与 `version`（每次成功重拉自增）。
 */
export const useItemsStore = defineStore('items', () => {
  const items = ref<ItemWithDailyCost[]>([])
  const status = ref<ItemsStatus>('idle')
  const version = ref(0)

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null

  /** 一次完整重拉：拉取期间保留旧数据，成功后整体替换（避免闪空与部分更新）。 */
  async function reload(): Promise<void> {
    status.value = 'loading'
    try {
      const list = await api.listItems()
      items.value = list
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

  /** 创建物品：成功后立即重拉（后端同时发 ledger:changed，事件侧重拉被在途合并）。 */
  async function create(input: ItemInput): Promise<string> {
    const id = await api.createItem(input)
    await refresh()
    return id
  }

  /** 按 id 修改物品（名称/购买日期/总成本/备注）：成功后立即重拉（同 create）。 */
  async function update(id: string, input: ItemInput): Promise<void> {
    await api.updateItem(id, input)
    await refresh()
  }

  /** 软删除物品（后端打 is_deleted=1，不物理移除）：成功后立即重拉（同上）。 */
  async function remove(id: string): Promise<void> {
    await api.deleteItem(id)
    await refresh()
  }

  // —— push 生命周期 ——
  // 首次访问 self-init：触发一次加载（失败静默，失败信号已由 status 承载）。
  void refresh().catch(() => {
    /* noop */
  })

  // 订阅后端 ledger:changed：物品写入即失效 → 静默重拉（stale-while-revalidate）。
  // 注册为异步；注册完成前到达的事件会丢失（窗口极窄，与 useReferenceStore 同款取舍）。
  listen('ledger:changed', () => {
    void refresh().catch(() => {
      /* noop：失败信号已由 status 承载 */
    })
  }).catch(() => {
    /* 监听注册失败不阻塞 store（本地事件，极少发生） */
  })

  return {
    items,
    status,
    version,
    refresh,
    create,
    update,
    remove,
  }
})
