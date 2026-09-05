import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// 覆写 setup.ts 的 useMessage mock（useBackup 内 useMessage 需要消息提供器）。
const messageApi = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
}))
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return { ...actual, useMessage: () => messageApi }
})

import BackupSettings from '@/components/settings/BackupSettings.vue'
import { useAppStore } from '@/stores/app'
import type { BackupFileInfo } from '@/types'

/** 按出现顺序取全部卡片标题（卡片顺序即页签内信息架构）。 */
function cardTitles(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('.n-card-header__main').map((c) => c.text())
}

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

const encryptedBackup: BackupFileInfo = {
  file_name: 'ledger-auto-20260217-093000.db.zip',
  path: '/Users/me/backups/ledger-auto-20260217-093000.db.zip',
  size_bytes: 4096,
  created_at: '2026-02-17T09:30:00Z',
  kind: 'auto',
  encrypted: true,
}

const plaintextBackup: BackupFileInfo = {
  file_name: 'ledger-backup-20260101-010101.db.zip',
  path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
  size_bytes: 1024,
  created_at: '2026-01-01T01:01:01Z',
  kind: 'manual',
  encrypted: false,
}

function stubList(list: BackupFileInfo[]) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_backups') return Promise.resolve(list)
    if (cmd === 'get_auto_backup_state')
      return Promise.resolve({ enabled: true, last_backup_at: null })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(vi.fn() as unknown as UnlistenFn)
  localStorage.clear()
  // 备份列表以配置目录为前提（未配置时列表恒空、不发 IPC）。
  useAppStore().setBackupDir('/Users/me/backups')
})

describe('BackupSettings 备份列表加密列（issue #572 / ADR-0075 决策 7）', () => {
  it('密文备份行渲染锁形标记，明文行不渲染', async () => {
    stubList([encryptedBackup, plaintextBackup])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    const html = wrapper.html()
    // 列头存在（用户可见的「加密」列）。
    expect(html).toContain('加密')
    // 锁形标记随标记显隐：密文行有图标（svg），明文行无。
    const icons = wrapper.findAll('.n-data-table-tr svg')
    expect(icons.length).toBe(1)
    // 标记带可读名（title 提示「密文备份」）。
    expect(wrapper.html()).toContain('密文备份')
  })

  it('全部为明文备份（含旧备份缺标记）时不渲染任何锁形标记', async () => {
    stubList([plaintextBackup])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    expect(wrapper.findAll('.n-data-table-tr svg')).toHaveLength(0)
  })
})

describe('BackupSettings 备份页签重排与危险视觉修正（issue #651 / ADR-0078 决策 4）', () => {
  it('卡片顺序为 备份（动作）→ 备份目录 → 自动备份 → 备份文件列表 → 恢复（最常用一键备份免滚动直达）', async () => {
    stubList([])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    expect(cardTitles(wrapper)).toEqual([
      '备份',
      '备份目录',
      '自动备份',
      '备份文件列表',
      '恢复',
    ])
  })

  it('恢复入口按钮降为 default 形态：红色警示由确认弹窗承载，双闸不变（ADR-0078 决策 4）', async () => {
    stubList([])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    const restoreBtn = wrapper.findAll('button').find((b) => b.text().includes('从备份恢复'))!
    expect(restoreBtn).toBeTruthy()
    expect(restoreBtn.classes()).not.toContain('n-button--error-type')
  })

  it('恢复卡破坏性警示升为显著警示块（NAlert）：不再是最低对比度灰字', async () => {
    stubList([])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    const alert = wrapper.find('.n-alert')
    expect(alert.exists()).toBe(true)
    // 显著形态：带图标；内容保留「替换当前全部数据」加粗后果句。
    expect(alert.classes()).toContain('n-alert--show-icon')
    expect(alert.text()).toContain('替换当前全部数据')
    expect(alert.find('strong').exists()).toBe(true)
  })

  it('清理按钮升为 warning 次要形态（不可恢复的删除带警示色，ADR-0078 决策 4）', async () => {
    stubList([plaintextBackup])
    const wrapper = mount(BackupSettings)
    await flushPromises()

    const pruneBtn = wrapper.findAll('button').find((b) => b.text().includes('立即清理'))!
    expect(pruneBtn).toBeTruthy()
    expect(pruneBtn.classes()).toContain('n-button--warning-type')
    expect(pruneBtn.classes()).toContain('n-button--secondary')
  })

  it('手动刷新按钮重拉备份列表：文件管理器手动增删后可使列表与磁盘一致（issue #651）', async () => {
    let disk: BackupFileInfo[] = [plaintextBackup]
    let listCalls = 0
    let resolveList: (() => void) | null = null
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_backups') {
        listCalls++
        return new Promise((resolve) => {
          resolveList = () => resolve(disk)
        })
      }
      if (cmd === 'get_auto_backup_state')
        return Promise.resolve({ enabled: true, last_backup_at: null })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(BackupSettings)
    // 首刷在途：按钮在场，先放行首次拉取。
    const refreshBtn = wrapper.find('[data-testid="backup-list-refresh"]')
    expect(refreshBtn.exists()).toBe(true)
    resolveList!()
    await flushPromises()
    expect(listCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 1 个备份')

    // 用户在文件管理器手动放入另一个备份 → 点刷新 → 列表与磁盘一致。
    disk = [encryptedBackup, plaintextBackup]
    await wrapper.find('[data-testid="backup-list-refresh"]').trigger('click')
    resolveList!()
    await flushPromises()
    expect(listCalls).toBe(2)
    expect(wrapper.html()).toContain('当前共 2 个备份')
    const cellTexts = wrapper.findAll('tbody td').map((td) => td.text())
    expect(cellTexts).toContain('ledger-auto-20260217-093000.db.zip')
  })
})
