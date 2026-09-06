import { describe, it, expect, vi, beforeEach } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import {
  stubReferenceInvoke,
  refCurrencies,
  refAccounts,
  refCategories,
  refMerchants,
  refInsurers,
} from './reference-stubs'

const mockInvoke = vi.mocked(invoke)

beforeEach(() => {
  mockInvoke.mockReset()
})

describe('stubReferenceInvoke 默认行为', () => {
  it('参考 store 重拉的全部 list_* 命令默认返回规范夹具', async () => {
    stubReferenceInvoke()
    await expect(mockInvoke('list_currencies')).resolves.toBe(refCurrencies)
    await expect(mockInvoke('list_accounts')).resolves.toBe(refAccounts)
    await expect(mockInvoke('list_categories')).resolves.toBe(refCategories)
    await expect(mockInvoke('list_merchants')).resolves.toBe(refMerchants)
    await expect(mockInvoke('list_insurers')).resolves.toBe(refInsurers)
  })

  it('规范夹具含软删行：软删表各有恰一行 is_deleted=true', () => {
    expect(refAccounts.filter((a) => a.is_deleted)).toHaveLength(1)
    expect(refCategories.filter((c) => c.is_deleted)).toHaveLength(1)
    expect(refMerchants.filter((m) => m.is_deleted)).toHaveLength(1)
    expect(refInsurers.filter((i) => i.is_deleted)).toHaveLength(1)
    expect(refAccounts.some((a) => !a.is_deleted)).toBe(true)
    expect(refCategories.some((c) => !c.is_deleted)).toBe(true)
    expect(refMerchants.some((m) => !m.is_deleted)).toBe(true)
    expect(refInsurers.some((i) => !i.is_deleted)).toBe(true)
  })

  it('未覆写的非参考命令保持 unexpected invoke 拒绝', async () => {
    stubReferenceInvoke()
    await expect(mockInvoke('list_transactions')).rejects.toThrow('unexpected invoke: list_transactions')
    await expect(mockInvoke('create_account')).rejects.toThrow('unexpected invoke: create_account')
  })
})

describe('stubReferenceInvoke 覆写', () => {
  it('覆写参考命令：固定值优先于规范夹具', async () => {
    const custom = [{ code: 'USD', name: '美元', symbol: '$', decimal_places: 2 }]
    stubReferenceInvoke({ list_currencies: custom })
    await expect(mockInvoke('list_currencies')).resolves.toBe(custom)
    // 未覆写的参考命令仍走规范夹具
    await expect(mockInvoke('list_merchants')).resolves.toBe(refMerchants)
  })

  it('覆写参考命令：函数型覆写在派发时以 args 调用', async () => {
    let merchants = refMerchants
    stubReferenceInvoke({
      list_merchants: () => merchants,
      list_accounts: (args) => ({ echoed: args ?? null }),
    })
    await expect(mockInvoke('list_merchants')).resolves.toBe(refMerchants)
    const emptied: typeof refMerchants = []
    merchants = emptied
    // 派发时取值：可变库改写后重拉读到最新值
    await expect(mockInvoke('list_merchants')).resolves.toBe(emptied)
    await expect(mockInvoke('list_accounts', { id: 'acc-1' })).resolves.toEqual({ echoed: { id: 'acc-1' } })
  })

  it('函数型覆写可返回 Promise（在途/拒绝场景原样透传）', async () => {
    stubReferenceInvoke({
      list_insurers: () => Promise.reject(new Error('db 错误')),
      list_categories: () => new Promise(() => {}), // 永不 resolve（在途）
    })
    await expect(mockInvoke('list_insurers')).rejects.toThrow('db 错误')
    let settled = false
    void mockInvoke('list_categories').then(() => { settled = true })
    await Promise.resolve()
    expect(settled).toBe(false)
  })

  it('覆写非参考命令：领域数据命令照常覆写', async () => {
    const txns = [{ id: 'txn-1' }]
    stubReferenceInvoke({ list_transactions: txns })
    await expect(mockInvoke('list_transactions', { filter: {} })).resolves.toBe(txns)
    await expect(mockInvoke('list_policies')).rejects.toThrow('unexpected invoke: list_policies')
  })

  it('返回派发函数：一次性桩可把其余命令委托回基础桩', async () => {
    const base = stubReferenceInvoke({ list_transactions: [{ id: 'txn-1' }] })
    mockInvoke.mockImplementation((cmd, args) =>
      cmd === 'create_transaction'
        ? Promise.resolve('new-id')
        : base(cmd, args as Record<string, unknown>),
    )
    await expect(mockInvoke('create_transaction')).resolves.toBe('new-id')
    // 未覆写的参考命令经委托仍走规范夹具
    await expect(mockInvoke('list_insurers')).resolves.toBe(refInsurers)
    await expect(mockInvoke('unknown_cmd')).rejects.toThrow('unexpected invoke: unknown_cmd')
  })
})
