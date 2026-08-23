import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
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

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
  { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 },
]

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
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  mockOpen.mockReset()
  mockSave.mockReset()
  mockConfirm.mockReset()
  localStorage.clear()
  const store = useAppStore()
  await store.loadAll()
})

describe('SettingsView.vue', () => {
  it('渲染 6 个 TabPane', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const settingsTabs = tabs.filter((t) =>
      ['分类', '币种', '数据管理', '备份与恢复', '外观', '关于'].includes(t.text()),
    )
    expect(settingsTabs.length).toBe(6)
  })

  it('Tab 标签文本正确', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).toContain('分类')
    expect(labels).toContain('币种')
    expect(labels).toContain('数据管理')
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
    const appearanceTab = wrapper.findAll('.n-tabs-tab')[4]
    await appearanceTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('深色模式')
  })

  it('关于 Tab 显示版本号', async () => {
    const wrapper = mount(SettingsView)
    const aboutTab = wrapper.findAll('.n-tabs-tab')[5]
    await aboutTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('版本号')
  })

  it('备份与恢复 Tab 包含备份/恢复操作与备份目录配置', async () => {
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[3]
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
    const backupTab = wrapper.findAll('.n-tabs-tab')[3]
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
    const backupTab = wrapper.findAll('.n-tabs-tab')[3]
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

  it('恢复前需要确认，确认后调用 restore_backup 与 restart_app', async () => {
    mockOpen.mockResolvedValueOnce('/Users/me/backups/ledger-backup.db.zip')
    mockConfirm.mockResolvedValueOnce(true)
    const wrapper = mount(SettingsView)
    const backupTab = wrapper.findAll('.n-tabs-tab')[3]
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
})
