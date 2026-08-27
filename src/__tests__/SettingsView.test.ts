import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import SettingsView from '@/views/SettingsView.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import type { Currency } from '@/types'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))

import { open, save, confirm } from '@tauri-apps/plugin-dialog'

const mockOpen = vi.mocked(open)
const mockSave = vi.mocked(save)
const mockConfirm = vi.mocked(confirm)

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
  { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 },
]

/**
 * mock-invoke 桩分发（沿本文件既有模式收口样板）：默认覆盖公共桩，
 * 测试用 `overrides` 只覆写差异项，未命中走默认或 reject。
 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  const defaults: Record<string, unknown> = {
    list_currencies: mockCurrencies,
    list_accounts: [],
    list_categories: [],
    list_backups: [],
  }
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    if (cmd in defaults) return Promise.resolve(defaults[cmd])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'create_backup') {
      return Promise.resolve({
        path: '/tmp/ledger-backup.db.zip',
        size_bytes: 1024,
        schema_version: 4,
        created_at: '2026-01-01T00:00:00Z',
      })
    }
    if (cmd === 'restore_backup') {
      return Promise.resolve({ schema_version: 4, restored_at: '2026-01-01T00:00:00Z' })
    }
    if (cmd === 'restart_app') return Promise.resolve()
    if (cmd === 'list_backups') return Promise.resolve([])
    if (cmd === 'prune_backups') return Promise.resolve({ kept: 0, deleted: [], failed: [] })
    if (cmd === 'get_auto_backup_state') {
      return Promise.resolve({ enabled: true, last_backup_at: null })
    }
    if (cmd === 'set_auto_backup_enabled') return Promise.resolve()
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  mockOpen.mockReset()
  mockSave.mockReset()
  mockConfirm.mockReset()
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('SettingsView.vue', () => {
  it('渲染 5 个 TabPane', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const settingsTabs = tabs.filter((t) =>
      ['分类', '币种', '备份与恢复', '外观', '关于'].includes(t.text()),
    )
    expect(settingsTabs.length).toBe(5)
  })

  it('Tab 标签文本正确', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).not.toContain('数据管理')
    expect(labels).toContain('分类')
    expect(labels).toContain('币种')
    expect(labels).toContain('备份与恢复')
    expect(labels).toContain('外观')
    expect(labels).toContain('关于')
  })

  it('包含 CategoryManager 组件', () => {
    const wrapper = mount(SettingsView)
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
  })

  it('币种 Tab 包含默认币种选择器', async () => {
    const wrapper = mount(SettingsView)
    const currencyTab = wrapper.findAll('.n-tabs-tab')[1]
    await currencyTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('默认币种')
  })

  it('外观 Tab 包含主题切换开关', async () => {
    const wrapper = mount(SettingsView)
    const appearanceTab = wrapper.findAll('.n-tabs-tab')[3]
    await appearanceTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('深色模式')
  })

  it('关于 Tab 显示版本号', async () => {
    const wrapper = mount(SettingsView)
    const aboutTab = wrapper.findAll('.n-tabs-tab')[4]
    await aboutTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('版本号')
  })

  it('备份与恢复 Tab 包含备份/恢复操作与备份目录配置', async () => {
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    const html = wrapper.html()
    expect(html).toContain('一键备份')
    expect(html).toContain('另存为')
    expect(html).toContain('从备份恢复')
    expect(html).toContain('选择目录')
  })

  it('备份目录可配置并持久化到 localStorage', async () => {
    mockOpen.mockResolvedValue('/Users/me/ledger-backups')
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    await wrapper.find('.n-button').trigger('click')
    await nextTick()
    expect(mockOpen).toHaveBeenCalledWith({ directory: true, multiple: false, title: '选择备份目录' })
    expect(localStorage.getItem('backup_dir')).toBe('"/Users/me/ledger-backups"')
  })

  it('一键备份调用 create_backup 命令', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    const buttons = wrapper.findAll('button')
    const backupBtn = buttons.find((b) => b.text().includes('一键备份'))!
    await backupBtn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith(
      'create_backup',
      expect.objectContaining({ targetPath: expect.stringMatching(/ledger-backup-\d{8}-\d{6}\.db\.zip$/) }),
    )
    expect(wrapper.html()).toContain('最近备份')
  })

  it('一键备份写入受管目录后自动滚动清理', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_backups') return Promise.resolve([])
      if (cmd === 'create_backup') {
        return Promise.resolve({
          path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
          size_bytes: 1024,
          schema_version: 4,
          created_at: '2026-01-01T01:01:01Z',
        })
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    const backupBtn = wrapper.findAll('button').find((b) => b.text().includes('一键备份'))!
    await backupBtn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('prune_backups', { dir: '/Users/me/backups', keep: 30 })
  })

  it('备份文件列表展示数量与上限，手动清理需确认', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    store.setBackupMaxCount(1)
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_backups') {
        return Promise.resolve([
          {
            file_name: 'ledger-backup-20260102-010101.db.zip',
            path: '/Users/me/backups/ledger-backup-20260102-010101.db.zip',
            size_bytes: 2048,
            created_at: '2026-01-02T01:01:01Z',
          },
          {
            file_name: 'ledger-backup-20260101-010101.db.zip',
            path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
            size_bytes: 1024,
            created_at: '2026-01-01T01:01:01Z',
          },
        ])
      }
      if (cmd === 'prune_backups') {
        return Promise.resolve({ kept: 1, deleted: ['ledger-backup-20260101-010101.db.zip'], failed: [] })
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    mockConfirm.mockResolvedValueOnce(true)
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    await flushPromises()
    expect(wrapper.html()).toContain('当前共 2 个备份，上限 1 个')
    const pruneBtn = wrapper.findAll('button').find((b) => b.text().includes('立即清理'))!
    await pruneBtn.trigger('click')
    await flushPromises()
    expect(mockConfirm).toHaveBeenCalled()
    expect(mockInvoke).toHaveBeenCalledWith('prune_backups', { dir: '/Users/me/backups', keep: 1 })
  })

  it('备份保留上限可配置并持久化', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    const input = wrapper.find('.n-input-number input')
    await input.setValue('10')
    await input.trigger('blur')
    expect(store.backupMaxCount).toBe(10)
    expect(localStorage.getItem('backup_max_count')).toBe('10')
  })

  it('自动备份卡片展示开关与上次自动备份时间', async () => {
    stubInvoke({
      get_auto_backup_state: () => ({ enabled: false, last_backup_at: '2026-02-17T09:30:00Z' }),
    })
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()
    await nextTick()
    const html = wrapper.html()
    expect(html).toContain('自动备份')
    expect(html).toContain('上次自动备份：2026-02-17 09:30')
    // 用语义属性 aria-checked 断言开关状态，不依赖内部样式类。
    expect(wrapper.find('.n-switch').attributes('aria-checked')).toBe('false')
  })

  it('切换自动备份开关调用 set_auto_backup_enabled 并刷新展示', async () => {
    let enabledState = true
    stubInvoke({
      get_auto_backup_state: () => ({ enabled: enabledState, last_backup_at: null }),
      set_auto_backup_enabled: (args?: { enabled?: boolean }) => {
        enabledState = args?.enabled ?? false
        return Promise.resolve()
      },
    })
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()
    await wrapper.find('.n-switch').trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_backup_enabled', { enabled: false })
    expect(wrapper.find('.n-switch').attributes('aria-checked')).toBe('false')
  })

  it('未配置备份目录时提示引导，配置后提示消失', async () => {
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('设置备份目录后自动备份生效')

    useAppStore().setBackupDir('/Users/me/backups')
    await nextTick()
    expect(wrapper.html()).not.toContain('设置备份目录后自动备份生效')
  })

  it('从未自动备份时显示从未占位', async () => {
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('上次自动备份：从未')
  })

  it('恢复前需要确认，确认后调用 restore_backup 与 restart_app', async () => {
    mockOpen.mockResolvedValueOnce('/Users/me/backups/ledger-backup.db.zip')
    mockConfirm.mockResolvedValueOnce(true)
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await nextTick()
    const buttons = wrapper.findAll('button')
    const restoreBtn = buttons.find((b) => b.text().includes('从备份恢复'))!
    await restoreBtn.trigger('click')
    await flushPromises()
    expect(mockConfirm).toHaveBeenCalled()
    expect(mockInvoke).toHaveBeenCalledWith('restore_backup', {
      backupPath: '/Users/me/backups/ledger-backup.db.zip',
    })
  })

  it('备份文件列表展示来源列，区分自动与手动（issue #129）', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    stubInvoke({
      list_backups: () => [
        {
          file_name: 'ledger-auto-20260217-093000.db.zip',
          path: '/Users/me/backups/ledger-auto-20260217-093000.db.zip',
          size_bytes: 4096,
          created_at: '2026-02-17T09:30:00Z',
          kind: 'auto',
        },
        {
          file_name: 'ledger-backup-20260101-010101.db.zip',
          path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
          size_bytes: 1024,
          created_at: '2026-01-01T01:01:01Z',
          kind: 'manual',
        },
      ],
    })
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()

    const headers = wrapper.findAll('th').map((t) => t.text())
    expect(headers).toContain('来源')
    const cellTexts = wrapper.findAll('tbody td').map((t) => t.text())
    expect(cellTexts).toContain('自动')
    expect(cellTexts).toContain('手动')
  })

  it('ledger:backups-changed 到达后自动刷新备份列表（issue #129）', async () => {
    useAppStore().setBackupDir('/Users/me/backups')
    mockListen.mockReset()
    let backupsChangedHandler: (...args: unknown[]) => void = () => {}
    mockListen.mockImplementation((_event: string, handler: never) => {
      backupsChangedHandler = handler
      return Promise.resolve(vi.fn())
    })
    let backupList: unknown[] = []
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_backups') return Promise.resolve(backupList)
      if (cmd === 'get_auto_backup_state') {
        return Promise.resolve({ enabled: true, last_backup_at: null })
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })

    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[2]
    await backupTab.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')

    // 后端自动备份完成 → 发出无 payload 信号 → 列表自动重拉。
    backupList = [
      {
        file_name: 'ledger-auto-20260217-093000.db.zip',
        path: '/Users/me/backups/ledger-auto-20260217-093000.db.zip',
        size_bytes: 4096,
        created_at: '2026-02-17T09:30:00Z',
        kind: 'auto',
      },
    ]
    backupsChangedHandler()
    await flushPromises()

    expect(wrapper.html()).toContain('当前共 1 个备份，上限 30 个')
    const cellTexts = wrapper.findAll('tbody td').map((t) => t.text())
    expect(cellTexts).toContain('ledger-auto-20260217-093000.db.zip')
    expect(cellTexts).toContain('自动')
  })
})
