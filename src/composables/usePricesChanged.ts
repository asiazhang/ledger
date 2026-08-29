import { onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/**
 * 价格失效信号（issue #237 / ADR-0031）：后端同步命令（增量
 * `sync_holding_prices` / 全量 `sync_instruments`）实际写入价格后发出
 * `ledger:prices-changed` 无 payload 信号（与 `ledger:changed` /
 * `ledger:backups-changed` 平行的第三域信号，同一 `ledger:*` 命名空间）。
 *
 * 价格消费方订阅后重拉自身数据，替代「同步后记得手动刷新」的调用方自觉；
 * 事件名常量在此单点定义，消费方一律经本 composable 订阅。
 */
export const PRICES_CHANGED_EVENT = 'ledger:prices-changed'

/**
 * 订阅价格失效信号：组件卸载时自动注销（卸载早于注册落定时，落定后立即
 * 注销，不留悬挂监听）。注册失败静默（本地事件，极少发生），不影响组件。
 *
 * 注：注册为异步，注册完成前到达的信号会丢失（窗口极窄：组件挂载瞬间，
 * 与参考 store 的 ledger:changed 订阅同形）。
 */
export function usePricesChanged(callback: () => void): void {
  let unlisten: UnlistenFn | null = null
  let disposed = false
  void listen(PRICES_CHANGED_EVENT, callback)
    .then((fn) => {
      if (disposed) {
        fn()
        return
      }
      unlisten = fn
    })
    .catch((e) => {
      console.warn(`订阅 ${PRICES_CHANGED_EVENT} 失败`, e)
    })
  onUnmounted(() => {
    disposed = true
    unlisten?.()
    unlisten = null
  })
}
