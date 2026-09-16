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

/** 工厂产出面：status / version 失效观测信号 + refresh 强制刷新 + invalidate 作废在途；数据面留店。 */
export interface PushFirstList {
  status: Ref<PushFirstListStatus>
  /** 每次成功落位自增（watch 源，先例：物品每日成本合计与商户管理视图）；
   *  被 invalidate 作废的旧纪元成功不落位、不自增。 */
  version: Ref<number>
  /** 强制刷新（在途时合并，避免 IPC 风暴）。 */
  refresh(): Promise<void>
  /**
   * 作废在途（ADR-0040 invalidate 先例，issue #1381）：推进竞态纪元并对 loading
   * 收尾（有成功快照 → ready，初载在途 → idle，error 不动），不发起新任务；此后
   * 迟到的旧纪元结果与失败一并作废（不落位、不置 error、对旧调用方静默 resolve）。
   * 参数化加载在参数变化时先 invalidate 再 refresh，新参数不再合并进旧参数的在途结果。
   */
  invalidate(): void
}

export function createPushFirstList<T>(
  load: () => Promise<T>,
  apply: (snapshot: T) => void,
): PushFirstList {
  const status = ref<PushFirstListStatus>('idle')
  const version = ref(0)

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null
  /** 竞态纪元：invalidate 推进，迟到旧纪元结果按过期作废（ADR-0040 竞态语义）。
   *  本纪元簿记是工厂机制本体（单点），非调用点手搓竞态守卫——同 useLoadable
   *  的 seq 豁免纪律（ADR-0123 决策 4 修订注）；check-async-guards 规则 1 的
   *  检测面（let *seq = 0）不含本形态，评审兜底以此注为凭。 */
  let epoch = 0

  /** 一次完整重拉：拉取期间保留旧数据，成功后整体替换；任一失败整体失败。
   *  加载期间被 invalidate 的旧纪元：结果与失败一并作废、对旧调用方静默 resolve。 */
  async function reload(myEpoch: number): Promise<void> {
    status.value = 'loading'
    try {
      const snapshot = await load()
      if (myEpoch !== epoch) return
      apply(snapshot)
      version.value += 1
      status.value = 'ready'
    } catch (e) {
      if (myEpoch !== epoch) return
      status.value = 'error'
      throw e
    }
  }

  /** 在途去重：并发调用（self-init / refresh / 写后重拉 / 事件）合并为同一次加载。 */
  function refresh(): Promise<void> {
    if (inFlight) return inFlight
    const myEpoch = epoch
    const pending = reload(myEpoch).finally(() => {
      // invalidate 已放行新加载时只清自己的槽位，不得动新纪元的在途槽
      if (inFlight === pending) inFlight = null
    })
    inFlight = pending
    return pending
  }

  /** 作废在途：推进纪元 + loading 收尾（不发起新任务，重拉由调用方随后 refresh）。 */
  function invalidate(): void {
    epoch += 1
    inFlight = null
    if (status.value === 'loading') {
      status.value = version.value > 0 ? 'ready' : 'idle'
    }
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

  return { status, version, refresh, invalidate }
}
