import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'

const mockInvoke = vi.mocked(invoke)

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
})

describe('useAppStore theme', () => {
  it('默认值为 dark', () => {
    const store = useAppStore()
    expect(store.theme).toBe('dark')
  })

  it('setTheme 切换并持久化到 localStorage', () => {
    const store = useAppStore()
    store.setTheme('light')
    expect(store.theme).toBe('light')
    expect(localStorage.getItem('appearance')).toBe('"light"')
  })

  it('setTheme 切回 dark', () => {
    const store = useAppStore()
    store.setTheme('light')
    store.setTheme('dark')
    expect(store.theme).toBe('dark')
    expect(localStorage.getItem('appearance')).toBe('"dark"')
  })

  it('从 localStorage 恢复主题', () => {
    localStorage.setItem('appearance', '"light"')
    const store = useAppStore()
    expect(store.theme).toBe('light')
  })
})

describe('useAppStore defaultCurrency', () => {
  it('默认值为 CNY', () => {
    const store = useAppStore()
    expect(store.defaultCurrency).toBe('CNY')
  })

  it('setDefaultCurrency 切换并持久化', () => {
    const store = useAppStore()
    store.setDefaultCurrency('USD')
    expect(store.defaultCurrency).toBe('USD')
    expect(localStorage.getItem('default_currency')).toBe('"USD"')
  })

  it('从 localStorage 恢复默认币种', () => {
    localStorage.setItem('default_currency', '"JPY"')
    const store = useAppStore()
    expect(store.defaultCurrency).toBe('JPY')
  })
})

describe('useAppStore backupDir', () => {
  it('默认值为空字符串', () => {
    const store = useAppStore()
    expect(store.backupDir).toBe('')
  })

  it('setBackupDir 切换并持久化', () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    expect(store.backupDir).toBe('/Users/me/backups')
    expect(localStorage.getItem('backup_dir')).toBe('"/Users/me/backups"')
  })

  it('从 localStorage 恢复备份目录', () => {
    localStorage.setItem('backup_dir', '"/tmp/ledger-backups"')
    const store = useAppStore()
    expect(store.backupDir).toBe('/tmp/ledger-backups')
  })

  it('setBackupDir 可清除', () => {
    const store = useAppStore()
    store.setBackupDir('/tmp/x')
    store.setBackupDir('')
    expect(store.backupDir).toBe('')
    expect(localStorage.getItem('backup_dir')).toBe('""')
  })
})

describe('useAppStore backupMaxCount', () => {
  it('默认值为 30', () => {
    const store = useAppStore()
    expect(store.backupMaxCount).toBe(30)
  })

  it('setBackupMaxCount 修改并持久化', () => {
    const store = useAppStore()
    store.setBackupMaxCount(10)
    expect(store.backupMaxCount).toBe(10)
    expect(localStorage.getItem('backup_max_count')).toBe('10')
  })

  it('从 localStorage 恢复保留上限', () => {
    localStorage.setItem('backup_max_count', '5')
    const store = useAppStore()
    expect(store.backupMaxCount).toBe(5)
  })
})

describe('useAppStore 收缩契约（issue #85）', () => {
  it('仅暴露 UI 设置，不再暴露参考数据 / 派生映射 / load 函数', () => {
    const store = useAppStore() as unknown as Record<string, unknown>
    // UI 设置（主题 / 默认币种 / 备份设置）保留
    expect(store.theme).toBeDefined()
    expect(store.defaultCurrency).toBeDefined()
    expect(store.backupDir).toBeDefined()
    expect(store.backupMaxCount).toBeDefined()
    expect(store.setTheme).toBeDefined()
    expect(store.setDefaultCurrency).toBeDefined()
    expect(store.setBackupDir).toBeDefined()
    expect(store.setBackupMaxCount).toBeDefined()
    // 参考数据 getters / 派生映射 / load 函数已移除
    const removed = [
      'currencies', 'accounts', 'categories',
      'currencyMap', 'accountMap', 'categoryMap',
      'rootCategories', 'expenseCategories', 'incomeCategories',
      'categoryChildren', 'categoryPath', 'treeCategoryOptions',
      'getCurrency',
      'loadAll', 'loadCurrencies', 'loadAccounts', 'loadCategories',
    ]
    for (const key of removed) {
      expect(store[key]).toBeUndefined()
    }
  })
})
