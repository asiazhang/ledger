import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '@/api'
import { t } from '@ledger/i18n'
import type { InstrumentSyncProgress, SyncInstrumentInfoResult } from '@ledger/types'

export type InstrumentInfoSyncStatus = 'idle' | 'success' | 'error'
/** sync() 返回的终态：InstrumentInfoSyncStatus 去掉 idle，与 status ref 同形。 */
export type InstrumentInfoSyncOutcome = Exclude<InstrumentInfoSyncStatus, 'idle'>

/**
 * 标的信息同步（issue #108 / #103，接缝承诺 #237 / ADR-0031；#827 随覆盖面
 * 放开与命令改名由 useHoldingPriceSync 更名）：封装同步进行中状态与结果消息。
 * 供标的页（T4）与盈亏页（T6）共用，保证两处按钮 loading 与消息反馈行为
 * 一致——调用方仅需渲染 `syncing`（按钮 loading）、`resultMessage` / `status`
 *（轻量消息反馈）与 `progress`（确定进度条，issue #897）。
 *
 * 接缝承诺：`sync()` 返回终态 `'success' | 'error'`（空库/部分跳过是
 * success 子形态，`lastResult.synced / skipped` 供调用方区分；不造「取消」
 * 态——同步无中断机制）。失效通知不归本接缝：价格/名称写入完成后由后端
 * 发 `ledger:prices-changed` 信号（ADR-0031），调用方经 `usePricesChanged`
 * 订阅重拉自身数据，无需在 sync 返回后手工刷新。
 *
 * 同步全程可见（issue #897 / ADR-0095）：接缝订阅后端确定进度事件
 * [`INSTRUMENT_SYNC_PROGRESS_EVENT`]（payload `{ done, total }`，基金深回填
 * 期间另带页级明细 `fund`，issue #1061），经 `progress` 产出可观察进度；
 * 同步全程唯一——任一入口在途时，其余入口的 `sync()` 一律短路复用同一承诺、
 * 不并发第二次同步。
 *
 * **全部接缝状态为模块级共享单例**（唯一读写方是本模块的事件订阅与 sync
 * 生命周期，先例：globalBusy 的聚合计数）：同步只有一场，进行中/进度/结果
 * 消息就是同一份——两入口渲染同一状态、行为零分叉，短路入口与发起入口看到
 * 完全相同的终态反馈（按钮 loading、结果消息/错误提示），切页不丢状态。
 */

/** 标的信息同步进度事件名（issue #897 / ADR-0095；带 payload，与无 payload
 * 的失效信号族同处 `ledger:*` 命名空间；后端常量单点在 sync 域 progress 模块）。
 * 载荷形状见 `types/sync.ts` 的 InstrumentSyncProgress。 */
export const INSTRUMENT_SYNC_PROGRESS_EVENT = 'ledger:instrument-sync-progress'

/** 同步进行中：按钮 loading 的唯一来源（两入口共享，短路入口同样成立）。 */
const syncing = ref(false)
/** 反馈文案：同步结果（含「暂无标的可同步」）或失败信息 */
const resultMessage = ref<string | null>(null)
/** 反馈状态：成功/失败，供消息着色 */
const status = ref<InstrumentInfoSyncStatus>('idle')
/** 最近一次同步结果（含同步/跳过统计），便于调用方按需展示 */
const lastResult = ref<SyncInstrumentInfoResult | null>(null)
/** 确定进度（issue #897）：终态收起清空，两入口共享同一份 */
const progress = ref<InstrumentSyncProgress | null>(null)

/** 在途同步计数：进度事件只在该计数非零时被接受（终态后迟到事件不复活进度条）。 */
let inFlightSyncs = 0

/**
 * 在途同步承诺：模块级唯一——进度条展示的是唯一那次在途同步（issue #897），
 * 任一入口在途时其余入口的触发一律短路复用同一承诺，不并发第二次同步。
 */
let inFlight: Promise<InstrumentInfoSyncOutcome> | null = null

/** 订阅登记：模块生命周期内只订阅一次（测试经 reset 重置后随新 mock 重订）。 */
let subscribed = false

function ensureProgressSubscription(): void {
  if (subscribed) return
  subscribed = true
  // 模块级订阅与应用同生命周期：不随组件卸载注销（进度状态是模块单例，
  // 先例：globalBusy 聚合计数）；注册失败静默（本地事件，极少发生）。
  void listen<InstrumentSyncProgress>(INSTRUMENT_SYNC_PROGRESS_EVENT, (event) => {
    // 守卫：进度事件只在同步在途时有意义——终态后迟到的 (done, total)
    // 不得让进度条复活；载荷形状异常（脏数据/NaN）一并忽略。
    const payload = event.payload as Partial<InstrumentSyncProgress> | undefined
    if (
      inFlightSyncs > 0 &&
      payload &&
      typeof payload.done === 'number' && Number.isFinite(payload.done) &&
      typeof payload.total === 'number' && Number.isFinite(payload.total)
    ) {
      // 页级明细（issue #1061）为可选字段：形状不合法即丢弃明细、保留标的级
      // 进度；缺省即「标的级推进」，不残留上一只基金的页明细。
      const fund = payload.fund
      const validFund = (
        fund &&
        typeof fund.code === 'string' &&
        typeof fund.page === 'number' && Number.isFinite(fund.page) &&
        typeof fund.pages === 'number' && Number.isFinite(fund.pages) && fund.pages > 0
      )
        ? { code: fund.code, page: fund.page, pages: fund.pages }
        : null
      progress.value = validFund
        ? { done: payload.done, total: payload.total, fund: validFund }
        : { done: payload.done, total: payload.total }
    }
  }).catch((e) => {
    console.warn(`订阅 ${INSTRUMENT_SYNC_PROGRESS_EVENT} 失败`, e)
  })
}

/**
 * 测试隔离用：清空全部接缝状态与在途计数、撤销订阅登记
 *（先例：resetGlobalBusy）。生产代码不得调用。
 */
export function resetInstrumentInfoSyncForTest(): void {
  syncing.value = false
  resultMessage.value = null
  status.value = 'idle'
  lastResult.value = null
  progress.value = null
  inFlightSyncs = 0
  inFlight = null
  subscribed = false
}

/**
 * 标的信息同步接缝（标的页/盈亏页共用；状态为模块级单例，行为一致性由
 * 同一接缝同一份状态保证——短路入口与发起入口零分叉）。
 */
export function useInstrumentInfoSync() {
  ensureProgressSubscription()

  function sync(): Promise<InstrumentInfoSyncOutcome> {
    // 在途短路：直接复用唯一那次在途同步的承诺。本实例无需自行置 loading/
    // 清消息——全部状态是模块级单例，进行中的表现（按钮 loading、进度条）
    // 与终态反馈（结果消息/错误提示）随共享状态自然到达本入口。
    if (inFlight) return inFlight
    syncing.value = true
    resultMessage.value = null
    status.value = 'idle'
    lastResult.value = null
    progress.value = null
    inFlightSyncs += 1
    inFlight = (async () => {
      try {
        const res = await api.syncInstrumentInfo()
        lastResult.value = res
        resultMessage.value = res.message
        status.value = 'success'
        return 'success'
      } catch (e: any) {
        const detail = e instanceof Error ? e.message : String(e)
        resultMessage.value = t('investments.sync.failed', { message: detail })
        status.value = 'error'
        return 'error'
      } finally {
        syncing.value = false
        inFlightSyncs -= 1
        // 终态收起清空（成功/失败同路）：进度条收起、结果消息接棒（issue #897）。
        progress.value = null
        inFlight = null
      }
    })()
    return inFlight
  }

  return { syncing, resultMessage, status, lastResult, progress, sync }
}
