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
