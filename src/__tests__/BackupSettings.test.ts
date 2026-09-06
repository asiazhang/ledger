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
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { BackupFileInfo } from '@/types'

/** 按出现顺序取全部卡片标题（卡片顺序即页签内信息架构）。 */
function cardTitles(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('.n-card-header__main').map((c) => c.text())
}

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

// 剧本剪贴板（issue #653）：断言复制内容与成功/失败提示分支（父 spec 测试决策：
// 组件测试中 mock 剪贴板对象，断言写入内容与成功提示）。
const writeText = vi.fn().mockResolvedValue(undefined)

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
  stubReferenceInvoke({
    list_backups: list,
    get_auto_backup_state: { enabled: true, last_backup_at: null },
    list_insurers: [],
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(vi.fn() as unknown as UnlistenFn)
  writeText.mockClear()
  Object.assign(navigator, { clipboard: { writeText } })
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
    stubReferenceInvoke({
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      list_insurers: [],
      list_backups: () => {
        listCalls++
        return new Promise((resolve) => {
          resolveList = () => resolve(disk)
        })
      },
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

describe('BackupSettings 手动清理确认弹窗（issue #652 / ADR-0078）', () => {
  /** 弹窗（teleport 到 body）内按 data-testid 找按钮。 */
  function bodyButton(testid: string): HTMLButtonElement {
    const btn = document.body.querySelector(`[data-testid="${testid}"]`) as HTMLButtonElement | null
    if (!btn) throw new Error(`未找到 testid=${testid} 的按钮`)
    return btn
  }

  function stubWithPrune(list: BackupFileInfo[], keep = 1) {
    stubReferenceInvoke({
      list_backups: list,
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      prune_backups: { kept: keep, deleted: ['/a', '/b'], failed: [] },
      list_insurers: [],
    })
  }

  it('超上限点「立即清理」：弹 warning 级应用内弹窗，待删数量与不可恢复后果在场，不立即删除', async () => {
    stubWithPrune([encryptedBackup, plaintextBackup])
    useAppStore().setBackupMaxCount(1)
    const wrapper = mount(BackupSettings)
    await flushPromises()

    await wrapper.findAll('button').find((b) => b.text().includes('立即清理'))!.trigger('click')
    await flushPromises()

    // warning 级形态（ADR-0078 决策 2）：琥珀警示块承载待删数量与不可恢复后果
    const alert = document.body.querySelector('.n-modal .n-alert')
    expect(alert, '警示块应存在').toBeTruthy()
    expect(document.body.textContent).toContain('将删除最旧的 1 个备份')
    expect(document.body.textContent).toContain('删除后不可恢复')
    expect(bodyButton('danger-confirm').className).toContain('n-button--warning-type')
    // 未确认前零删除副作用
    expect(mockInvoke).not.toHaveBeenCalledWith('prune_backups', expect.anything())
  })

  it('确认清理：执行 prune_backups 后弹窗关闭并刷新列表', async () => {
    stubWithPrune([encryptedBackup, plaintextBackup])
    useAppStore().setBackupMaxCount(1)
    const wrapper = mount(BackupSettings)
    await flushPromises()

    await wrapper.findAll('button').find((b) => b.text().includes('立即清理'))!.trigger('click')
    await flushPromises()
    bodyButton('danger-confirm').click()
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('prune_backups', { dir: '/Users/me/backups', keep: 1 })
  })

  it('取消清理：弹窗关闭，不发任何删除调用（零副作用）', async () => {
    stubWithPrune([encryptedBackup, plaintextBackup])
    useAppStore().setBackupMaxCount(1)
    const wrapper = mount(BackupSettings)
    await flushPromises()

    await wrapper.findAll('button').find((b) => b.text().includes('立即清理'))!.trigger('click')
    await flushPromises()
    bodyButton('danger-cancel').click()
    await flushPromises()

    expect(mockInvoke).not.toHaveBeenCalledWith('prune_backups', expect.anything())
    void wrapper
  })
})

describe('BackupSettings 复制与访达定位通道（issue #653）', () => {
  it('备份列表行「在访达中显示」：以该行完整路径调用 reveal_in_file_manager，成功无错误提示', async () => {
    stubReferenceInvoke({
      list_backups: [encryptedBackup, plaintextBackup],
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      reveal_in_file_manager: () => Promise.resolve(undefined),
      list_insurers: [],
    })
    const wrapper = mount(BackupSettings)
    await flushPromises()

    await wrapper
      .find(`[data-testid="backup-reveal-${encryptedBackup.file_name}"]`)
      .trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('reveal_in_file_manager', {
      path: encryptedBackup.path,
    })
    expect(messageApi.error).not.toHaveBeenCalled()
  })

  it('访达定位失败：后端中文错误原样透传为错误提示', async () => {
    stubReferenceInvoke({
      list_backups: [plaintextBackup],
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      reveal_in_file_manager: () => Promise.reject(new Error('在访达中显示失败：boom')),
      list_insurers: [],
    })
    const wrapper = mount(BackupSettings)
    await flushPromises()

    await wrapper
      .find(`[data-testid="backup-reveal-${plaintextBackup.file_name}"]`)
      .trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('reveal_in_file_manager', {
      path: plaintextBackup.path,
    })
    expect(messageApi.error).toHaveBeenCalledWith('在访达中显示失败：boom')
  })

  it('最近备份路径「复制路径」：写入完整原始路径（不含大小括注）并成功提示', async () => {
    const backupPath = '/Users/me/backups/ledger-backup-20260217-120000.db.zip'
    stubReferenceInvoke({
      create_backup: { path: backupPath, size_bytes: 2048 },
      prune_backups: { kept: 1, deleted: [], failed: [] },
      list_backups: [],
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      list_insurers: [],
    })
    const wrapper = mount(BackupSettings)
    await flushPromises()
    // 未备份前无复制入口（无路径可复制）。
    expect(wrapper.find('[data-testid="copy-last-backup-path"]').exists()).toBe(false)

    await wrapper.findAll('button').find((b) => b.text().includes('一键备份'))!.trigger('click')
    await flushPromises()
    await wrapper.find('[data-testid="copy-last-backup-path"]').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith(backupPath)
    expect(messageApi.success).toHaveBeenCalledWith(expect.stringContaining('已复制完整路径'))
  })

  it('复制失败：错误提示，不静默', async () => {
    const backupPath = '/Users/me/backups/ledger-backup-20260217-120000.db.zip'
    stubReferenceInvoke({
      create_backup: { path: backupPath, size_bytes: 2048 },
      prune_backups: { kept: 1, deleted: [], failed: [] },
      list_backups: [],
      get_auto_backup_state: { enabled: true, last_backup_at: null },
      list_insurers: [],
    })
    const wrapper = mount(BackupSettings)
    await flushPromises()
    await wrapper.findAll('button').find((b) => b.text().includes('一键备份'))!.trigger('click')
    await flushPromises()
    writeText.mockRejectedValueOnce(new Error('剪贴板不可用'))
    // 备份流程本身已有一次成功提示；复制失败后不应再增成功提示。
    const successBefore = messageApi.success.mock.calls.length
    await wrapper.find('[data-testid="copy-last-backup-path"]').trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith(expect.stringContaining('复制路径失败'))
    expect(messageApi.success.mock.calls.length).toBe(successBefore)
    void wrapper
  })
})
