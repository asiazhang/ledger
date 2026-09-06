import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { captureListenHandlers, mockListen, type CapturedListener } from './helpers/listen-mock'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { usePoliciesStore } from '@/stores/policies'
import { makePolicy, makePolicyStats } from './factories'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Policy, PolicyInput, PolicyStats } from '@/types'

function basePolicy(over: Partial<Policy> = {}): Policy {
  return makePolicy({ id: 'policy-1', ...over })
}

function baseStats(over: Partial<PolicyStats> = {}): PolicyStats {
  return makePolicyStats(over)
}

const createInput: PolicyInput = {
  insurer_id: 'm-1',
  policy_number: 'P2026-002',
  product_name: '医疗险',
  start_date: '2026-02-01',
  end_date: null,
  coverage_amount_cents: null,
  coverage_currency_code: null,
  note: null,
}

/** 捕获 ledger:changed 监听处理器（store 创建时注册） */
let handlers: CapturedListener[]

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  handlers = captureListenHandlers()
})

describe('usePoliciesStore', () => {
  it('首次访问自动加载（self-init），加载后 status=ready、version=1', async () => {
    stubReferenceInvoke({
      list_policies: [basePolicy()],
      list_policy_stats: [baseStats()],
      list_insurers: [],
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.policies).toHaveLength(1)
    expect(store.policies[0].policy_number).toBe('P2026-001')
    expect(store.status).toBe('ready')
    expect(store.version).toBe(1)
  })

  it('加载失败时 status=error，不抛出（self-init 静默）', async () => {
    stubReferenceInvoke({
      list_policies: () => Promise.reject(new Error('boom')),
      list_policy_stats: [],
      list_insurers: [],
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
      if (cmd === 'list_policy_stats') return Promise.resolve([baseStats()])
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
    stubReferenceInvoke({
      list_policy_stats: [],
      list_policies: () => {
        listCalls++
        return listCalls > 1 ? [basePolicy({ id: 'new-1', ...createInput })] : []
      },
      create_policy: (args) => {
        expect(args).toMatchObject({ input: createInput })
        return 'new-1'
      },
      list_insurers: [],
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
    stubReferenceInvoke({
      list_policy_stats: [],
      list_policies: () => current.filter((p) => !p.is_deleted),
      update_policy: (args) => {
        const { id, input } = args as { id: string; input: PolicyInput }
        expect(id).toBe('policy-1')
        current[0] = { ...current[0], ...input, version: 2 }
      },
      delete_policy: (args) => {
        const { id } = args as { id: string }
        current[0] = { ...current[0], is_deleted: true }
        expect(id).toBe('policy-1')
      },
      list_insurers: [],
    })
    const store = usePoliciesStore()
    await flushPromises()
    await store.update('policy-1', { ...createInput, policy_number: 'P-EDIT' })
    expect(store.policies[0].policy_number).toBe('P-EDIT')
    await store.remove('policy-1')
    // 软删后不进列表（后端 WHERE is_deleted=0 过滤）
    expect(store.policies).toHaveLength(0)
  })

  it('统计与列表同批重拉，statsById 按保单 id 索引（issue #363）', async () => {
    const stats = [
      baseStats({ policy_id: 'policy-1', total_paid_native_cents: 600_000, next_charge_date: '2027-01-01' }),
    ]
    stubReferenceInvoke({
      list_policies: [basePolicy()],
      list_policy_stats: stats,
      list_insurers: [],
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.status).toBe('ready')
    expect(store.stats).toEqual(stats)
    expect(store.statsById.get('policy-1')?.total_paid_native_cents).toBe(600_000)
    expect(store.statsById.get('missing')).toBeUndefined()
  })

  it('统计拉取失败与列表失败同语义：status=error（整体失败，不部分更新）', async () => {
    stubReferenceInvoke({
      list_policies: [basePolicy()],
      list_policy_stats: () => Promise.reject(new Error('stats boom')),
      list_insurers: [],
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.status).toBe('error')
    expect(store.policies).toEqual([])
    expect(store.stats).toEqual([])
  })

  it('create 失败向上抛（调用方展示错误，弹窗不关）', async () => {
    stubReferenceInvoke({
      list_policies: [],
      list_policy_stats: [],
      create_policy: () => Promise.reject(new Error('保单号不能为空')),
      list_insurers: [],
    })
    const store = usePoliciesStore()
    await flushPromises()
    await expect(store.create(createInput)).rejects.toThrow('保单号不能为空')
  })
})
