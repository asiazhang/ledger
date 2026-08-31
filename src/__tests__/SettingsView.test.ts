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
 * 「数据」pane 用 display-directive='show:lazy'，首次激活即同时挂载
 * BackupSettings 与 DataLocationSettings，故 get_data_location_info 进默认桩。
 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  const defaults: Record<string, unknown> = {
    list_currencies: mockCurrencies,
    list_accounts: [],
    list_categories: [],
    list_merchants: [],
    list_backups: [],
    get_data_location_info: {
      active_dir: '/Users/me/Library/Application Support/ledger',
      configured_dir: null,
      pending_restart: false,
      fallback_reason: null,
    },
    create_backup: {
      path: '/tmp/ledger-backup.db.zip',
      size_bytes: 1024,
      schema_version: 4,
      created_at: '2026-01-01T00:00:00Z',
    },
    restore_backup: { schema_version: 4, restored_at: '2026-01-01T00:00:00Z' },
    restart_app: null,
    prune_backups: { kept: 0, deleted: [], failed: [] },
    get_auto_backup_state: { enabled: true, last_backup_at: null },
    set_auto_backup_enabled: null,
  }
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    if (cmd in defaults) return Promise.resolve(defaults[cmd])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

/** 定位标题为指定文本的卡片（Naive UI 卡片头主标题元素）。 */
function findCardByTitle(wrapper: ReturnType<typeof mount>, title: string) {
  return wrapper.findAll('.n-card').find((c) => c.find('.n-card-header__main').text() === title)
}

/** 按标签文本点击设置页 Tab（避免依赖不稳定的位置下标）。 */
async function openTab(wrapper: ReturnType<typeof mount>, label: string) {
  const tab = wrapper.findAll('.n-tabs-tab').find((t) => t.text() === label)
  expect(tab, `设置页应存在「${label}」Tab`).toBeTruthy()
  await tab!.trigger('click')
  await nextTick()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockOpen.mockReset()
  mockSave.mockReset()
  mockConfirm.mockReset()
  localStorage.clear()
  // 默认桩收口到 stubInvoke（含备份全链路与 get_data_location_info）。
  stubInvoke()
  const store = useReferenceStore()
  await store.refresh()
})

describe('SettingsView.vue（issue #157：Tab 分域重构 6 → 4）', () => {
  it('Tab 格局为 通用 → 分类 → 商户 → 数据 → 定时 → 关于，共 6 个，关于在末位（#189 商户；#308 定时，按 ADR-0022 插在关于之前）', () => {
    const wrapper = mount(SettingsView)
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).toEqual(['通用', '分类', '商户', '数据', '定时', '关于'])
  })

  it('旧 Tab（备份与恢复 / 外观 / 存储位置）全部消失', () => {
    // 「分类」「币种」不再列入：ADR-0034 后「分类」是现役 Tab 名（原「分类与币种」更名），
    // 币种只读展示已移除，不再有独立币种 Tab。
    const wrapper = mount(SettingsView)
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).not.toContain('币种')
    expect(labels).not.toContain('备份与恢复')
    expect(labels).not.toContain('外观')
    expect(labels).not.toContain('存储位置')
  })

  it('「通用」默认激活，含深色模式开关与默认币种下拉，行为不变', async () => {
    const wrapper = mount(SettingsView)
    // 通用是首个 Tab，无需点击即挂载（show:lazy 语义）。
    const html = wrapper.html()
    expect(html).toContain('深色模式')
    expect(html).toContain('默认币种')
    // 深色模式开关反映当前主题（默认暗色）。
    expect(wrapper.find('.n-switch').attributes('aria-checked')).toBe('true')
  })

  it('「通用」内切换深色模式开关更新 app store 主题', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    await wrapper.find('.n-switch').trigger('click')
    expect(store.theme).toBe('light')
  })

  it('「分类」含分类管理器，不再展示支持币种表格（ADR-0034）', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '分类')
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
    const html = wrapper.html()
    expect(html).not.toContain('支持币种')
    expect(html).not.toContain('默认币种')
  })

  it('「数据」同时承载备份与恢复、数据存储位置两组件', async () => {
    stubInvoke({
      get_data_location_info: () =>
        Promise.resolve({
          active_dir: '/Users/me/Library/Application Support/ledger',
          configured_dir: null,
          pending_restart: false,
          fallback_reason: null,
        }),
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('get_data_location_info')
    const html = wrapper.html()
    expect(html).toContain('一键备份')
    expect(html).toContain('从备份恢复')
    expect(html).toContain('数据存储位置')
    expect(html).toContain('/Users/me/Library/Application Support/ledger')
  })

  it('「关于」在末位，显示版本号', async () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    expect(tabs[tabs.length - 1].text()).toBe('关于')
    await openTab(wrapper, '关于')
    expect(wrapper.html()).toContain('版本号')
  })

  it('备份列表在 Tab 切换间保留缓存，不随切换重拉', async () => {
    useAppStore().setBackupDir('/Users/me/backups')
    let listBackupsCalls = 0
    stubInvoke({
      list_backups: () => {
        listBackupsCalls++
        return Promise.resolve([])
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')

    // 切走再切回：数据 pane 保持挂载（display-directive='show:lazy'），不重拉。
    await openTab(wrapper, '通用')
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')
  })

  it('备份与恢复：目录选择持久化到 localStorage', async () => {
    mockOpen.mockResolvedValue('/Users/me/ledger-backups')
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
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
    await openTab(wrapper, '数据')
    await nextTick()
    const backupBtn = wrapper.findAll('button').find((b) => b.text().includes('一键备份'))!
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
    stubInvoke({
      create_backup: () => ({
        path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
        size_bytes: 1024,
        schema_version: 4,
        created_at: '2026-01-01T01:01:01Z',
      }),
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
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
    stubInvoke({
      list_backups: () => [
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
      ],
      prune_backups: () => ({ kept: 1, deleted: ['ledger-backup-20260101-010101.db.zip'], failed: [] }),
    })
    mockConfirm.mockResolvedValueOnce(true)
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
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
    await openTab(wrapper, '数据')
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
    await openTab(wrapper, '数据')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('自动备份')
    expect(html).toContain('上次自动备份：2026-02-17 09:30')
    // 用语义属性 aria-checked 断言开关状态，不依赖内部样式类。
    const backupSwitch = findCardByTitle(wrapper, '自动备份')!.find('.n-switch')
    expect(backupSwitch.attributes('aria-checked')).toBe('false')
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
    await openTab(wrapper, '数据')
    await flushPromises()
    const backupSwitch = findCardByTitle(wrapper, '自动备份')!.find('.n-switch')
    await backupSwitch.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_backup_enabled', { enabled: false })
    expect(backupSwitch.attributes('aria-checked')).toBe('false')
  })

  it('未配置备份目录时提示引导，配置后提示消失', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(wrapper.html()).toContain('设置备份目录后自动备份生效')

    useAppStore().setBackupDir('/Users/me/backups')
    await nextTick()
    expect(wrapper.html()).not.toContain('设置备份目录后自动备份生效')
  })

  it('从未自动备份时显示从未占位', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(wrapper.html()).toContain('上次自动备份：从未')
  })

  it('恢复前需要确认，确认后调用 restore_backup 与 restart_app', async () => {
    mockOpen.mockResolvedValueOnce('/Users/me/backups/ledger-backup.db.zip')
    mockConfirm.mockResolvedValueOnce(true)
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    const restoreBtn = wrapper.findAll('button').find((b) => b.text().includes('从备份恢复'))!
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
    await openTab(wrapper, '数据')
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
    stubInvoke({
      list_backups: () => Promise.resolve(backupList),
      get_auto_backup_state: () => ({ enabled: true, last_backup_at: null }),
    })

    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
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

  it('存储位置异常态文案不变：待重启提示与回退告警照常展示', async () => {
    stubInvoke({
      get_data_location_info: () =>
        Promise.resolve({
          active_dir: '/Users/me/Library/Application Support/ledger',
          configured_dir: '/Users/me/ledger-data',
          pending_restart: true,
          fallback_reason: null,
        }),
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    let html = wrapper.html()
    expect(html).toContain('数据存储位置')
    expect(html).toContain('/Users/me/ledger-data')
    expect(html).toContain('下次启动')

    // 回退告警：fallback_reason 非空时展示回退提示，原路径仍可见。
    stubInvoke({
      get_data_location_info: () =>
        Promise.resolve({
          active_dir: '/Users/me/Library/Application Support/ledger',
          configured_dir: null,
          pending_restart: false,
          fallback_reason: '配置的位置不可用：权限不足',
        }),
    })
    const wrapper2 = mount(SettingsView)
    await openTab(wrapper2, '数据')
    await flushPromises()
    html = wrapper2.html()
    expect(html).toContain('已回退到默认位置')
    expect(html).toContain('权限不足')
  })
})

describe('SettingsView.vue 定时 Tab：设备级自动执行开关（issue #308 / ADR-0042）', () => {
  it('「定时」内含自动执行卡片：默认关，附「只应在一台机器开启」与设备偏好说明', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '定时')
    const card = findCardByTitle(wrapper, '自动执行')
    expect(card, '「定时」Tab 应存在「自动执行」卡片').toBeTruthy()
    const text = card!.text()
    expect(text).toContain('自动执行只应在一台机器开启')
    expect(text).toContain('不随备份')
    // 默认关（ADR-0042：设备级开关默认关，换新机器或恢复备份后保持本机值）。
    expect(card!.find('.n-switch').attributes('aria-checked')).toBe('false')
  })

  it('切换开关更新 store 并持久化 localStorage（设备偏好落点，不经后端持久化）', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '定时')
    const sw = findCardByTitle(wrapper, '自动执行')!.find('.n-switch')
    await sw.trigger('click')
    expect(store.autoExecutionEnabled).toBe(true)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('true')
    expect(findCardByTitle(wrapper, '自动执行')!.find('.n-switch').attributes('aria-checked')).toBe('true')
    await sw.trigger('click')
    expect(store.autoExecutionEnabled).toBe(false)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('false')
  })
})
