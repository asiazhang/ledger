import { ref } from 'vue'
import { api } from '@/api'
import type { SyncHoldingPricesResult } from '@/types'

export type HoldingPriceSyncStatus = 'idle' | 'success' | 'error'
/** sync() 返回的终态：HoldingPriceSyncStatus 去掉 idle，与 status ref 同形。 */
export type HoldingPriceSyncOutcome = Exclude<HoldingPriceSyncStatus, 'idle'>

/**
 * 持仓价格增量同步（issue #108 / #103，接缝承诺 #237 / ADR-0031）：
 * 封装同步进行中状态与结果消息。供标的页（T4）与盈亏页（T6）共用，
 * 保证两处按钮 loading 与消息反馈行为一致——调用方仅需渲染 `syncing`
 * （按钮 loading）与 `resultMessage` / `status`（轻量消息反馈）。
 *
 * 接缝承诺：`sync()` 返回终态 `'success' | 'error'`（无持仓/部分跳过是
 * success 子形态，`lastResult.synced / skipped` 供调用方区分；不造「取消」
 * 态——增量同步无中断机制）。失效通知不归本接缝：价格写入完成后由后端
 * 发 `ledger:prices-changed` 信号（ADR-0031），调用方经 `usePricesChanged`
 * 订阅重拉自身数据，无需在 sync 返回后手工刷新。
 */
export function useHoldingPriceSync() {
  const syncing = ref(false)
  /** 反馈文案：同步结果（含「无持仓标的可同步」）或失败信息 */
  const resultMessage = ref<string | null>(null)
  /** 反馈状态：成功/失败，供消息着色 */
  const status = ref<HoldingPriceSyncStatus>('idle')
  /** 最近一次同步结果（含同步/跳过统计），便于调用方按需展示 */
  const lastResult = ref<SyncHoldingPricesResult | null>(null)
  /** 在途同步承诺：进行中的重复调用短路复用，终态与首次调用一致 */
  let inFlight: Promise<HoldingPriceSyncOutcome> | null = null

  function sync(): Promise<HoldingPriceSyncOutcome> {
    if (inFlight) return inFlight
    syncing.value = true
    resultMessage.value = null
    status.value = 'idle'
    lastResult.value = null
    inFlight = (async () => {
      try {
        const res = await api.syncHoldingPrices()
        lastResult.value = res
        resultMessage.value = res.message
        status.value = 'success'
        return 'success'
      } catch (e: any) {
        const detail = e instanceof Error ? e.message : String(e)
        resultMessage.value = `同步失败：${detail}`
        status.value = 'error'
        return 'error'
      } finally {
        syncing.value = false
        inFlight = null
      }
    })()
    return inFlight
  }

  return { syncing, resultMessage, status, lastResult, sync }
}
