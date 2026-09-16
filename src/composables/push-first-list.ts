import { ref } from 'vue'
import type { Ref } from 'vue'
import { listen } from '@tauri-apps/api/event'

/**
 * push-first 清单生命周期工厂（ADR-0123）：领域清单 store 共享的同一套机制单点——
 * status 四态 + version 成功计数 + inFlight 在途合并 + stale-while-revalidate
 * 整体替换 + self-init + 失效信号（`ledger:changed`）订阅重拉。
 *
 * 接缝形状：`load` 产出一次完整快照（单列表、双列表、多表 `Promise.all` 同形），
 * `apply` 由调用方把快照写入自己的 refs。时序由工厂钉死：
 * status=loading → 成功才 apply + version++ → ready；任一失败 → error、不 apply
 * （整体替换、不闪空、部分失败不落位）。软删拆分、派生映射、筛选参数等
 * 领域特化全部留店（「特化留适配器」，同 ScheduledPlanList 纪律门槛）。
 *
 * 机制术语不进词汇表（ADR-0118 口径）；参考数据域「Reference Data」词条的
 * 重拉语义（领域规则，ADR-0012）由本工厂作为实现载体承载，语义一字不改。
 */

/** 清单加载状态：`idle` 为初始瞬态（self-init 同步置为 `loading`，外部基本观察不到）。 */
export type PushFirstListStatus = 'idle' | 'loading' | 'ready' | 'error'

/** 工厂产出面：status / version 失效观测信号 + refresh 强制刷新；数据面留店。 */
export interface PushFirstList {
  status: Ref<PushFirstListStatus>
  /** 每次成功重拉自增（watch 源，先例：物品每日成本合计与商户管理视图）。 */
  version: Ref<number>
  /** 强制刷新（在途时合并，避免 IPC 风暴）。 */
  refresh(): Promise<void>
}

export function createPushFirstList<T>(
  load: () => Promise<T>,
  apply: (snapshot: T) => void,
): PushFirstList {
  const status = ref<PushFirstListStatus>('idle')
  const version = ref(0)

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null

  /** 一次完整重拉：拉取期间保留旧数据，成功后整体替换；任一失败整体失败。 */
  async function reload(): Promise<void> {
    status.value = 'loading'
    try {
      apply(await load())
      version.value += 1
      status.value = 'ready'
    } catch (e) {
      status.value = 'error'
      throw e
    }
  }

  /** 在途去重：并发调用（self-init / refresh / 写后重拉 / 事件）合并为同一次加载。 */
  function refresh(): Promise<void> {
    if (inFlight) return inFlight
    inFlight = reload().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  // —— push 生命周期 ——
  // 首次访问 self-init：触发一次加载（失败静默，失败信号已由 status 承载）。
  void refresh().catch(() => {
    /* noop */
  })

  // 订阅后端 ledger:changed：清单写入即失效 → 静默重拉（stale-while-revalidate）。
  // 注册为异步；注册完成前到达的事件会丢失（窗口极窄，既有取舍随迁单点化）。
  listen('ledger:changed', () => {
    void refresh().catch(() => {
      /* noop：失败信号已由 status 承载 */
    })
  }).catch(() => {
    /* 监听注册失败不阻塞调用方（本地事件，极少发生） */
  })

  return { status, version, refresh }
}
