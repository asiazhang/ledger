import { computed, onMounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { useAppStore } from '@/stores/app'
import { api } from '@/api'
import type { BackupFileInfo } from '@/types'

// 备份文件列表与滚动清理。命名规则与后端受管备份规则保持一致。
const MANAGED_BACKUP_PREFIX = 'ledger-backup-'
const MANAGED_BACKUP_SUFFIX = '.db.zip'

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatBackupTime(iso: string): string {
  return iso.slice(0, 16).replace('T', ' ')
}

function defaultBackupFileName(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `ledger-backup-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.db.zip`
}

/// 规整备份目录：去掉尾部斜杠，返回目录与分隔符。
function normalizeBackupDir(raw: string): { dir: string; sep: '/' | '\\' } {
  const dir = raw.replace(/[\\/]+$/, '')
  const sep = dir.includes('\\') ? '\\' : '/'
  return { dir, sep }
}

/// 目标路径是否为受管备份：位于配置的备份目录内且文件名匹配自动命名规则。
function isManagedBackupPath(target: string, backupDir: string): boolean {
  if (!backupDir) return false
  const { dir, sep } = normalizeBackupDir(backupDir)
  const base = target.split(/[\\/]/).pop() ?? ''
  return (
    target.startsWith(dir + sep) &&
    base.startsWith(MANAGED_BACKUP_PREFIX) &&
    base.endsWith(MANAGED_BACKUP_SUFFIX)
  )
}

export function useBackup() {
  const store = useAppStore()
  const message = useMessage()

  const backingUp = ref(false)
  const restoring = ref(false)
  const lastBackup = ref('')
  const backups = ref<BackupFileInfo[]>([])
  const pruning = ref(false)

  const backupRows = computed(() =>
    backups.value.map((b) => ({
      ...b,
      size_text: formatSize(b.size_bytes),
      created_at: formatBackupTime(b.created_at),
    })),
  )

  async function pickBackupDir() {
    const dir = await open({ directory: true, multiple: false, title: '选择备份目录' })
    if (typeof dir === 'string' && dir) {
      store.setBackupDir(dir)
      message.success('备份目录已设置')
      await refreshBackups()
    }
  }

  function clearBackupDir() {
    store.setBackupDir('')
    backups.value = []
  }

  async function refreshBackups() {
    if (!store.backupDir) {
      backups.value = []
      return
    }
    try {
      backups.value = await api.listBackups(store.backupDir)
    } catch (e: any) {
      backups.value = []
      message.error(`读取备份列表失败: ${e}`)
    }
  }

  /// 将备份目录中的受管备份修剪到 `keep` 个，并刷新列表。
  async function pruneToLimit(keep: number) {
    if (!store.backupDir) return
    try {
      const r = await api.pruneBackups(store.backupDir, keep)
      await refreshBackups()
      if (r.failed.length > 0) {
        message.warning(`清理完成：已删除 ${r.deleted.length} 个，${r.failed.length} 个失败`)
      } else if (r.deleted.length > 0) {
        message.success(`已清理 ${r.deleted.length} 个旧备份`)
      }
    } catch (e: any) {
      message.error(`清理备份失败: ${e}`)
    }
  }

  /// 上限变更：调小时立即清理到新值（输入框 blur/回车提交，不弹确认，仅提示）。
  function onBackupMaxCountChange(n: number | null) {
    if (n == null) return
    const prev = store.backupMaxCount
    store.setBackupMaxCount(n)
    if (store.backupDir && n < prev) {
      void pruneToLimit(n)
    }
  }

  /// 手动立即清理：超过上限时弹确认后执行。
  async function manualPrune() {
    if (!store.backupDir) return
    const excess = Math.max(0, backups.value.length - store.backupMaxCount)
    if (excess === 0) {
      message.info('无需清理：备份数量未超过上限')
      return
    }
    const ok = await confirm(`将删除最旧的 ${excess} 个备份，删除后不可恢复。确定继续吗？`, {
      title: '确认清理',
      kind: 'warning',
    })
    if (!ok) return
    pruning.value = true
    try {
      await pruneToLimit(store.backupMaxCount)
    } finally {
      pruning.value = false
    }
  }

  async function doBackup(target: string) {
    backingUp.value = true
    try {
      const r = await api.createBackup(target)
      lastBackup.value = `${r.path}（${(r.size_bytes / 1024).toFixed(1)} KB）`
      message.success('备份成功')
      if (isManagedBackupPath(target, store.backupDir)) {
        // 受管备份写入后立即滚动清理（一键备份/另存为同规则）。
        await pruneToLimit(store.backupMaxCount)
      } else {
        await refreshBackups()
      }
    } catch (e: any) {
      message.error(`备份失败: ${e}`)
    } finally {
      backingUp.value = false
    }
  }

  async function backupOnce() {
    if (store.backupDir) {
      const { dir, sep } = normalizeBackupDir(store.backupDir)
      await doBackup(`${dir}${sep}${defaultBackupFileName()}`)
    } else {
      await backupAs()
    }
  }

  async function backupAs() {
    const path = await save({
      title: '备份到…',
      defaultPath: store.backupDir
        ? `${store.backupDir}/${defaultBackupFileName()}`
        : defaultBackupFileName(),
      filters: [{ name: 'Ledger 备份', extensions: ['zip'] }],
    })
    if (typeof path === 'string' && path) await doBackup(path)
  }

  async function pickRestore() {
    const path = await open({
      title: '从备份恢复…',
      directory: false,
      multiple: false,
      defaultPath: store.backupDir || undefined,
      filters: [{ name: 'Ledger 备份', extensions: ['zip', 'db'] }],
    })
    if (typeof path !== 'string' || !path) return
    const ok = await confirm(
      '恢复将替换当前全部数据，且不可撤销。\n\n系统会在恢复前自动备份当前数据；恢复成功后应用将自动重启。\n\n确定继续吗？',
      { title: '确认恢复', kind: 'warning' },
    )
    if (!ok) return
    restoring.value = true
    try {
      const r = await api.restoreBackup(path)
      message.success(`恢复成功（schema v${r.schema_version}），应用即将重启`)
      setTimeout(() => {
        api.restartApp()
      }, 800)
    } catch (e: any) {
      message.error(`恢复失败: ${e}`)
    } finally {
      restoring.value = false
    }
  }

  onMounted(refreshBackups)

  return {
    backingUp,
    restoring,
    lastBackup,
    backups,
    pruning,
    backupRows,
    pickBackupDir,
    clearBackupDir,
    onBackupMaxCountChange,
    manualPrune,
    backupOnce,
    backupAs,
    pickRestore,
  }
}
