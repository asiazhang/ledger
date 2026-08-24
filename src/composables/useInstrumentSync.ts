import { onMounted, onUnmounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { SyncProgress, SyncResult } from '@/types'

/// 股票标的全量同步：状态与进度监听注册在视图挂载时，
/// 保证同步过程中切换 tab 不丢失事件与状态（tab 内容为懒挂载）。
export function useInstrumentSync() {
  const message = useMessage()

  const syncStatus = ref<'idle' | 'syncing' | 'done'>('idle')
  const syncProgress = ref(0)
  const syncResult = ref<SyncResult | null>(null)
  let unlisten: UnlistenFn | null = null

  onMounted(async () => {
    unlisten = await listen<SyncProgress>('sync-instruments:progress', (event) => {
      const p = event.payload
      if (p.error) {
        syncStatus.value = 'idle'
        syncResult.value = null
        message.error(`同步失败: ${p.error}`)
        return
      }
      if (p.done) {
        syncStatus.value = 'done'
        syncProgress.value = 100
        syncResult.value = { inserted: p.total_inserted, updated: p.total_updated }
        message.success(`同步完成: 新增 ${p.total_inserted} 只, 更新 ${p.total_updated} 只`)
        return
      }
      syncStatus.value = 'syncing'
      if (p.total > 0) {
        syncProgress.value = Math.round((p.current / p.total) * 100)
      }
    })
  })

  onUnmounted(() => {
    unlisten?.()
  })

  async function startSync() {
    if (syncStatus.value === 'syncing') return
    syncStatus.value = 'syncing'
    syncProgress.value = 0
    syncResult.value = null
    try {
      await api.syncInstruments()
    } catch (e: any) {
      syncStatus.value = 'idle'
      message.error(`同步启动失败: ${e}`)
    }
  }

  return { syncStatus, syncProgress, syncResult, startSync }
}
