import { ref } from 'vue'
import { api } from '@/api'
import type { SyncHoldingPricesResult } from '@/types'

export type HoldingPriceSyncStatus = 'idle' | 'success' | 'error'

/**
 * 持仓价格增量同步（issue #108 / #103）：封装同步进行中状态与结果消息。
 * 供标的页（T4）与盈亏页（T6）共用，保证两处按钮 loading 与消息反馈行为一致——
 * 调用方仅需渲染 `syncing`（按钮 loading）与 `resultMessage` / `status`（轻量消息反馈）。
 */
export function useHoldingPriceSync() {
  const syncing = ref(false)
  /** 反馈文案：同步结果（含「无持仓标的可同步」）或失败信息 */
  const resultMessage = ref<string | null>(null)
  /** 反馈状态：成功/失败，供消息着色 */
  const status = ref<HoldingPriceSyncStatus>('idle')
  /** 最近一次同步结果（含同步/跳过统计），便于调用方按需展示 */
  const lastResult = ref<SyncHoldingPricesResult | null>(null)

  async function sync() {
    if (syncing.value) return
    syncing.value = true
    resultMessage.value = null
    status.value = 'idle'
    lastResult.value = null
    try {
      const res = await api.syncHoldingPrices()
      lastResult.value = res
      resultMessage.value = res.message
      status.value = 'success'
    } catch (e: any) {
      const detail = e instanceof Error ? e.message : String(e)
      resultMessage.value = `同步失败：${detail}`
      status.value = 'error'
    } finally {
      syncing.value = false
    }
  }

  return { syncing, resultMessage, status, lastResult, sync }
}
