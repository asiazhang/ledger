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
