import { computed, onMounted, onUnmounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { CancelSyncResult, SyncProgress } from '@/types'

/// 全量同步终态：完成 / 中断 / 失败 三方明确区分（issue #109）。
/// 与「持仓价格增量同步」区分：本接缝针对全量同步命令 `sync_instruments` /
/// `cancel_sync_instruments` 与 `sync-instruments:progress` 进度事件。
export type FullSyncStatus = 'idle' | 'syncing' | 'done' | 'cancelled' | 'error'

/**
 * 股票标的全量同步（issue #109）：封装「二次确认 → 启动 → 进度 → 中断 → 终态」全流程。
 *
 * - 状态为组件实例内局部（per-instance），经 onMounted/onUnmounted 注册/注销进度监听。
 * - 同步在后台线程执行，关闭进度对话框不影响同步；重开对话框可继续查看进度/终态。
 * - 启动前须经二次确认（`openConfirm` → `confirmSync`），未确认不调用同步命令。
 * - 中断经 `requestCancel` 置位取消标志，终态（cancelled=true）由进度事件回流。
 */
export function useInstrumentFullSync() {
  // 同步状态与进度
  const syncStatus = ref<FullSyncStatus>('idle')
  const progress = ref(0) // 0-100 百分比
  const current = ref(0) // 已处理标的数（同步事件 current）
  const total = ref(0) // 总数（同步事件 total）
  const inserted = ref(0) // 累计新增
  const updated = ref(0) // 累计更新
  const errorMessage = ref<string | null>(null)

  // 对话框开关
  const confirmOpen = ref(false) // 二次确认
  const modalOpen = ref(false) // 进度模态框
  const cancelling = ref(false) // 中断请求进行中（按钮 loading）

  const syncing = computed(() => syncStatus.value === 'syncing')

  let unlisten: UnlistenFn | null = null

  function applyProgress(p: SyncProgress) {
    if (p.error) {
      // 失败终态（done=true + error）
      syncStatus.value = 'error'
      errorMessage.value = p.error
      return
    }
    if (p.done) {
      // 终态：完成（cancelled=false）或中断（cancelled=true）
      inserted.value = p.total_inserted
      updated.value = p.total_updated
      syncStatus.value = p.cancelled ? 'cancelled' : 'done'
      progress.value = 100
      return
    }
    // 进行中：进度条 + 已处理/总数 + 累计新增/更新
    syncStatus.value = 'syncing'
    current.value = p.current
    total.value = p.total
    inserted.value = p.total_inserted
    updated.value = p.total_updated
    progress.value = p.total > 0 ? Math.round((p.current / p.total) * 100) : 0
  }

  onMounted(async () => {
    unlisten = await listen<SyncProgress>('sync-instruments:progress', (event) =>
      applyProgress(event.payload),
    )
  })

  onUnmounted(() => {
    unlisten?.()
    unlisten = null
  })

  // 打开二次确认（同步进行中不再弹确认，改为重开进度框由组件层处理）
  function openConfirm() {
    if (syncing.value) return
    confirmOpen.value = true
  }

  function closeConfirm() {
    confirmOpen.value = false
  }

  /** 二次确认：关闭确认框并启动同步（接入新同步前置守卫，防重复触发） */
  function confirmSync() {
    confirmOpen.value = false
    return startSync()
  }

  /** 启动全量同步：置位 syncing 并调用同步命令，打开进度框展示进度 */
  async function startSync() {
    if (syncing.value) return
    syncStatus.value = 'syncing'
    progress.value = 0
    current.value = 0
    total.value = 0
    inserted.value = 0
    updated.value = 0
    errorMessage.value = null
    modalOpen.value = true
    try {
      await api.syncInstruments()
    } catch (e) {
      // sync_instruments 启动即失败（如已有同步进行中）：置错误态
      syncStatus.value = 'error'
      errorMessage.value = e instanceof Error ? e.message : String(e)
    }
  }

  /** 中断同步：置位取消标志，终态（cancelled）经进度事件回流 */
  async function requestCancel() {
    if (!syncing.value || cancelling.value) return
    cancelling.value = true
    try {
      const res: CancelSyncResult = await api.cancelSyncInstruments()
      // cancelled=false 表示取消时已无同步在跑（可能在边缘时刻已结束）：
      // 终态由 sync-instruments:progress 事件统一落定，这里不改动同步状态。
      // 仅当确实取消了才继续等待终态回流；命令失败则报中断失败。
      if (!res.cancelled) return
    } catch (e) {
      errorMessage.value = `中断失败：${e instanceof Error ? e.message : String(e)}`
    } finally {
      cancelling.value = false
    }
  }

  function openModal() {
    modalOpen.value = true
  }

  function closeModal() {
    modalOpen.value = false
  }

  return {
    syncStatus,
    syncing,
    progress,
    current,
    total,
    inserted,
    updated,
    errorMessage,
    confirmOpen,
    modalOpen,
    cancelling,
    openConfirm,
    closeConfirm,
    confirmSync,
    startSync,
    requestCancel,
    openModal,
    closeModal,
  }
}
