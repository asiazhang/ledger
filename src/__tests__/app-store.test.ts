import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { setActivePinia, createPinia } from 'pinia'
import { useAppStore } from '@/stores/app'
import { formatAmount, amountPrivacyEnabled } from '@/utils/money'
import { stubReferenceInvoke } from './helpers/reference-stubs'


beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  stubReferenceInvoke({
    list_currencies: [],
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
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

describe('useAppStore autoExecutionEnabled（issue #308：设备级「自动执行」开关）', () => {
  it('默认值为 false（默认关，ADR-0042）', () => {
    const store = useAppStore()
    expect(store.autoExecutionEnabled).toBe(false)
  })

  it('setAutoExecutionEnabled 切换并持久化 localStorage', () => {
    const store = useAppStore()
    store.setAutoExecutionEnabled(true)
    expect(store.autoExecutionEnabled).toBe(true)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('true')
    store.setAutoExecutionEnabled(false)
    expect(store.autoExecutionEnabled).toBe(false)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('false')
  })

  it('从 localStorage 恢复开关（本机值，不随备份迁移的落点）', () => {
    localStorage.setItem('auto_execution_enabled', 'true')
    const store = useAppStore()
    expect(store.autoExecutionEnabled).toBe(true)
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

describe('useAppStore amountPrivacyEnabled（issue #566：金额隐私模式，轻量设置项）', () => {
  it('默认值为 false', () => {
    const store = useAppStore()
    expect(store.amountPrivacyEnabled).toBe(false)
  })

  it('setAmountPrivacyEnabled 切换并持久化 localStorage', () => {
    const store = useAppStore()
    store.setAmountPrivacyEnabled(true)
    expect(store.amountPrivacyEnabled).toBe(true)
    expect(localStorage.getItem('amount_privacy_enabled')).toBe('true')
    store.setAmountPrivacyEnabled(false)
    expect(store.amountPrivacyEnabled).toBe(false)
    expect(localStorage.getItem('amount_privacy_enabled')).toBe('false')
  })

  it('从 localStorage 恢复开关（跨启动水合，不随备份迁移的落点）', () => {
    localStorage.setItem('amount_privacy_enabled', 'true')
    const store = useAppStore()
    expect(store.amountPrivacyEnabled).toBe(true)
  })

  it('store 状态与格式化层消费同一 ref：切换即时反映到 formatAmount', () => {
    const store = useAppStore()
    expect(formatAmount(12345)).toBe('123.45')
    store.setAmountPrivacyEnabled(true)
    expect(formatAmount(12345)).toBe('••••')
    store.setAmountPrivacyEnabled(false)
    expect(formatAmount(12345)).toBe('123.45')
    expect(amountPrivacyEnabled.value).toBe(false)
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
