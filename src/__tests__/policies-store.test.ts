import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { usePoliciesStore } from '@/stores/policies'
import type { Policy, PolicyInput } from '@/types'

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

function basePolicy(over: Partial<Policy> = {}): Policy {
  return {
    id: 'policy-1',
    merchant_id: 'm-1',
    policy_number: 'P2026-001',
    product_name: '重疾险',
    start_date: '2026-01-01',
    end_date: '2036-01-01',
    coverage_amount_cents: 30_000_000,
    coverage_currency_code: 'CNY',
    note: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...over,
  }
}

const createInput: PolicyInput = {
  merchant_id: 'm-1',
  policy_number: 'P2026-002',
  product_name: '医疗险',
  start_date: '2026-02-01',
  end_date: null,
  coverage_amount_cents: null,
  coverage_currency_code: null,
  note: null,
}

/** 捕获 ledger:changed 监听处理器（store 创建时注册） */
let handlers: Array<(evt: unknown) => void>

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  handlers = []
  mockListen.mockImplementation(async (_evt, handler) => {
    handlers.push(handler)
    return vi.fn()
  })
})

describe('usePoliciesStore', () => {
  it('首次访问自动加载（self-init），加载后 status=ready、version=1', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_policies') return Promise.resolve([basePolicy()])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.policies).toHaveLength(1)
    expect(store.policies[0].policy_number).toBe('P2026-001')
    expect(store.status).toBe('ready')
    expect(store.version).toBe(1)
  })

  it('加载失败时 status=error，不抛出（self-init 静默）', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_policies') return Promise.reject(new Error('boom'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.status).toBe('error')
    expect(store.policies).toEqual([])
  })

  it('ledger:changed 触发重拉（stale-while-revalidate：在途不闪空，成功后整体替换）', async () => {
    const initial = [basePolicy()]
    const fresh = [basePolicy(), basePolicy({ id: 'policy-2', policy_number: 'P2026-002' })]
    let resolveSecond: (items: Policy[]) => void = () => {}
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd !== 'list_policies') return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      listCalls++
      if (listCalls === 1) return Promise.resolve(initial)
      return new Promise<Policy[]>((resolve) => {
        resolveSecond = resolve
      })
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.policies).toEqual(initial)

    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    // 第二次拉取在途：旧数据保留，不闪空
    expect(store.policies).toEqual(initial)
    resolveSecond(fresh)
    await flushPromises()
    expect(store.policies).toEqual(fresh)
    expect(store.version).toBe(2)
  })

  it('create 成功后立即重拉并返回 id', async () => {
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_policies') {
        listCalls++
        return Promise.resolve(listCalls > 1 ? [basePolicy({ id: 'new-1', ...createInput })] : [])
      }
      if (cmd === 'create_policy') {
        expect(args).toMatchObject({ input: createInput })
        return Promise.resolve('new-1')
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = usePoliciesStore()
    await flushPromises()
    const id = await store.create(createInput)
    expect(id).toBe('new-1')
    await flushPromises()
    expect(store.policies).toHaveLength(1)
  })

  it('update / remove 成功后立即重拉', async () => {
    const current = [basePolicy()]
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_policies') {
        return Promise.resolve(current.filter((p) => !p.is_deleted))
      }
      if (cmd === 'update_policy') {
        const { id, input } = args as { id: string; input: PolicyInput }
        expect(id).toBe('policy-1')
        current[0] = { ...current[0], ...input, version: 2 }
        return Promise.resolve()
      }
      if (cmd === 'delete_policy') {
        const { id } = args as { id: string }
        current[0] = { ...current[0], is_deleted: true }
        expect(id).toBe('policy-1')
        return Promise.resolve()
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = usePoliciesStore()
    await flushPromises()
    await store.update('policy-1', { ...createInput, policy_number: 'P-EDIT' })
    expect(store.policies[0].policy_number).toBe('P-EDIT')
    await store.remove('policy-1')
    // 软删后不进列表（后端 WHERE is_deleted=0 过滤）
    expect(store.policies).toHaveLength(0)
  })

  it('create 失败向上抛（调用方展示错误，弹窗不关）', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_policies') return Promise.resolve([])
      if (cmd === 'create_policy') return Promise.reject(new Error('保单号不能为空'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = usePoliciesStore()
    await flushPromises()
    await expect(store.create(createInput)).rejects.toThrow('保单号不能为空')
  })
})
