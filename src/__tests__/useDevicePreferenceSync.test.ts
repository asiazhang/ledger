import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { useAppStore } from '@/stores/app'
import { useDevicePreferenceSync } from '@/composables/useDevicePreferenceSync'
import { stubReferenceInvoke } from './helpers/reference-stubs'

// 设备偏好镜像推送（issue #308 / ADR-0042；备份目录先例 ADR-0016 决策 3）：
// 真源在前端 localStorage（应用设置 store），应用根组件挂载一次本 composable，
// 启动（immediate）与变更时推给后端运行时消费。测试主缝与既有先例一致：
// mock invoke 断言命令调用，不触碰真实 Tauri。


/** 宿主组件：模拟 App.vue 在 setup 内挂载一次 composable。 */
const Host = defineComponent({
  setup() {
    useDevicePreferenceSync()
    return () => null
  },
})

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  stubReferenceInvoke({
    set_auto_backup_dir: () => Promise.resolve(),
    set_auto_execution_enabled: () => Promise.resolve(),
  })
  localStorage.clear()
})

describe('useDevicePreferenceSync（启动回放 + 变更推送）', () => {
  it('启动回放：挂载即把 localStorage 恢复出的设备偏好推给后端', async () => {
    localStorage.setItem('backup_dir', '"/tmp/ledger-backups"')
    localStorage.setItem('auto_execution_enabled', 'true')
    mount(Host)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_backup_dir', { dir: '/tmp/ledger-backups' })
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_execution_enabled', { enabled: true })
  })

  it('自动执行开关默认关：未推送过的后端镜像收到 false（镜像默认关语义一致）', async () => {
    mount(Host)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_execution_enabled', { enabled: false })
  })

  it('变更推送：开关变更即再次推送，无需重启', async () => {
    const store = useAppStore()
    mount(Host)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_execution_enabled', { enabled: false })

    store.setAutoExecutionEnabled(true)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_execution_enabled', { enabled: true })

    store.setAutoExecutionEnabled(false)
    await flushPromises()
    expect(mockInvoke).toHaveBeenLastCalledWith('set_auto_execution_enabled', { enabled: false })
  })

  it('变更推送：备份目录变更同样再次推送（既有行为随接缝收口不回退）', async () => {
    const store = useAppStore()
    mount(Host)
    await flushPromises()
    expect(mockInvoke).toHaveBeenNthCalledWith(1, 'set_auto_backup_dir', { dir: '' })

    store.setBackupDir('/Users/me/backups')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_backup_dir', { dir: '/Users/me/backups' })
  })
})
