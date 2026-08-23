<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import {
  NTabs,
  NTabPane,
  NCard,
  NDataTable,
  NSpace,
  NSelect,
  NSwitch,
  NText,
  NButton,
  NProgress,
  useMessage,
} from 'naive-ui'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open, save, confirm } from '@tauri-apps/plugin-dialog'
import { useAppStore } from '@/stores/app'
import { api } from '@/api'
import CategoryManager from '@/components/CategoryManager.vue'
import type { SyncProgress } from '@/types'
import pkg from '@/../package.json'

const store = useAppStore()
const message = useMessage()

const currencyColumns = [
  { title: '代码', key: 'code', width: 80 },
  { title: '名称', key: 'name' },
  { title: '符号', key: 'symbol', width: 80 },
  { title: '小数位', key: 'decimal_places', width: 80 },
]

const currencyOptions = computed(() =>
  store.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
)

const syncStatus = ref<'idle' | 'syncing' | 'done'>('idle')
const syncProgress = ref(0)
const syncResult = ref<{ inserted: number; updated: number } | null>(null)
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  await store.loadAll()
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

async function openLogDir() {
  try {
    await invoke('plugin:log|open_log_dir')
  } catch (e: any) {
    message.error(`打开日志目录失败: ${e}`)
  }
}

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

// ---------------------------------------------------------------------------
// 备份与恢复
// ---------------------------------------------------------------------------

const backingUp = ref(false)
const restoring = ref(false)
const lastBackup = ref('')

function defaultBackupFileName(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  return `ledger-backup-${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.db.zip`
}

async function pickBackupDir() {
  const dir = await open({ directory: true, multiple: false, title: '选择备份目录' })
  if (typeof dir === 'string' && dir) {
    store.setBackupDir(dir)
    message.success('备份目录已设置')
  }
}

async function doBackup(target: string) {
  backingUp.value = true
  try {
    const r = await api.createBackup(target)
    lastBackup.value = `${r.path}（${(r.size_bytes / 1024).toFixed(1)} KB）`
    message.success('备份成功')
  } catch (e: any) {
    message.error(`备份失败: ${e}`)
  } finally {
    backingUp.value = false
  }
}

async function backupOnce() {
  if (store.backupDir) {
    const dir = store.backupDir.replace(/[\\/]+$/, '')
    const sep = dir.includes('\\') ? '\\' : '/'
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
</script>

<template>
  <NTabs type="line">
    <NTabPane name="categories" tab="分类">
      <CategoryManager />
    </NTabPane>

    <NTabPane name="currencies" tab="币种">
      <NSpace vertical :size="16">
        <NCard title="默认币种" size="small">
          <NSelect
            :value="store.defaultCurrency"
            :options="currencyOptions"
            @update:value="(val: string) => store.setDefaultCurrency(val)"
            style="max-width: 280px"
          />
        </NCard>

        <NCard title="支持币种" size="small">
          <NDataTable :columns="currencyColumns" :data="store.currencies" :bordered="false" size="small" />
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="sync" tab="数据管理">
      <NSpace vertical :size="16">
        <NCard title="股票标的全量同步" size="small">
          <NSpace vertical :size="12">
            <NText depth="3">
              从东方财富 API 一键拉取沪市、深市、港股的股票标的信息和最新价格。
              已存在的标的名称或市场变更时会自动更新，不会删除已有数据。
            </NText>
            <NSpace align="center" :size="12">
              <NButton
                type="primary"
                :disabled="syncStatus === 'syncing'"
                :loading="syncStatus === 'syncing'"
                @click="startSync"
              >
                {{ syncStatus === 'syncing' ? '正在同步...' : '开始同步' }}
              </NButton>
              <NProgress
                v-if="syncStatus === 'syncing'"
                style="flex: 1; max-width: 300px"
                :percentage="syncProgress"
                :show-indicator="true"
                :indicator-placement="'inside'"
                status="success"
                :height="28"
              />
            </NSpace>
            <NText v-if="syncResult" type="success">
              同步完成：新增 {{ syncResult.inserted }} 只，更新 {{ syncResult.updated }} 只
            </NText>
          </NSpace>
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="backup" tab="备份与恢复">
      <NSpace vertical :size="16">
        <NCard title="备份目录" size="small">
          <NSpace vertical :size="12">
            <NText depth="3">
              配置默认备份目录后，“一键备份”将直接写入该目录，无需每次选择位置。
            </NText>
            <NSpace align="center" :size="12">
              <NText style="word-break: break-all">
                {{ store.backupDir || '未设置（备份时将弹出位置选择）' }}
              </NText>
              <NButton size="small" @click="pickBackupDir">
                {{ store.backupDir ? '更改目录' : '选择目录' }}
              </NButton>
              <NButton v-if="store.backupDir" size="small" quaternary type="error" @click="store.setBackupDir('')">
                清除
              </NButton>
            </NSpace>
          </NSpace>
        </NCard>

        <NCard title="备份" size="small">
          <NSpace vertical :size="12">
            <NText depth="3">
              将当前账本（账户、交易、预算、投资持仓、定时交易等全部数据）打包备份为一个文件。
              备份不含主题、默认币种等界面偏好。
            </NText>
            <NSpace align="center" :size="12">
              <NButton type="primary" :loading="backingUp" @click="backupOnce">一键备份</NButton>
              <NButton :loading="backingUp" @click="backupAs">另存为…</NButton>
            </NSpace>
            <NText v-if="lastBackup" type="success" style="word-break: break-all">
              最近备份：{{ lastBackup }}
            </NText>
          </NSpace>
        </NCard>

        <NCard title="恢复" size="small">
          <NSpace vertical :size="12">
            <NText type="warning" depth="3">
              从备份文件恢复将<strong>替换当前全部数据</strong>（破坏性操作）。
              恢复前系统会自动备份当前数据到应用数据目录；恢复成功后应用将自动重启。
            </NText>
            <NButton type="error" :loading="restoring" @click="pickRestore">从备份恢复…</NButton>
          </NSpace>
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="appearance" tab="外观">
      <NSpace vertical :size="16">
        <NCard title="主题模式" size="small">
          <NSpace align="center" :size="12">
            <NText>深色模式</NText>
            <NSwitch
              :value="store.theme === 'dark'"
              @update:value="(val: boolean) => store.setTheme(val ? 'dark' : 'light')"
            />
          </NSpace>
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="about" tab="关于">
      <NCard title="关于 Ledger" size="small">
        <NSpace vertical :size="8">
          <NText>应用名称：Ledger</NText>
          <NText>版本号：{{ pkg.version }}</NText>
          <NText>构建平台：Tauri + Vue 3 + TypeScript</NText>
          <NButton size="small" @click="openLogDir">打开日志目录</NButton>
        </NSpace>
      </NCard>
    </NTabPane>
  </NTabs>
</template>
