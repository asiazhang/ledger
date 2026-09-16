import { describe, it, expect } from 'vitest'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { usePoliciesStore } from '@/policy/policies'
import { makePolicy, makePolicyStats } from './factories'
import type { Policy, PolicyInput, PolicyStats } from '@ledger/types'

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

/**
 * 机制断言（self-init / SWR / 在途合并 / 事件重拉 / status-version / 部分失败不落位）
 * 已收口到 push-first-list.test.ts 工厂单点（ADR-0123 决策 6）；本文件只留领域动作断言。
 */

describe('usePoliciesStore', () => {
  it('create 成功后立即重拉并返回 id', async () => {
    let listCalls = 0
    wireInvokeSeam({
      overrides: {
        list_policy_stats: [],
        list_policies: () => {
          listCalls++
          return listCalls > 1 ? [basePolicy({ id: 'new-1', ...createInput })] : []
        },
        create_policy: (args) => {
          expect(args).toMatchObject({ input: createInput })
          return 'new-1'
        },
      },
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
    wireInvokeSeam({
      overrides: {
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
      },
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
    wireInvokeSeam({
      overrides: {
        list_policies: [basePolicy()],
        list_policy_stats: stats,
      },
    })
    const store = usePoliciesStore()
    await flushPromises()
    expect(store.status).toBe('ready')
    expect(store.stats).toEqual(stats)
    expect(store.statsById.get('policy-1')?.total_paid_native_cents).toBe(600_000)
    expect(store.statsById.get('missing')).toBeUndefined()
  })

  it('create 失败向上抛（调用方展示错误，弹窗不关）', async () => {
    wireInvokeSeam({
      overrides: {
        list_policies: [],
        list_policy_stats: [],
        create_policy: () => Promise.reject(new Error('保单号不能为空')),
      },
    })
    const store = usePoliciesStore()
    await flushPromises()
    await expect(store.create(createInput)).rejects.toThrow('保单号不能为空')
  })

  it('写入成功后重拉失败不反转写动作成败：动作正常返回，失败信号由 status 承载（ADR-0123 决策 3）', async () => {
    let listCalls = 0
    wireInvokeSeam({
      overrides: {
        list_policies: () => {
          listCalls++
          return listCalls === 1 ? Promise.resolve([basePolicy()]) : Promise.reject(new Error('重拉失败'))
        },
        list_policy_stats: [],
        create_policy: () => 'new-1',
      },
    })
    const store = usePoliciesStore()
    await flushPromises()

    // 已落库的建档不因重拉失败误报「保存失败」，动作正常返回 id
    await expect(store.create(createInput)).resolves.toBe('new-1')
    expect(store.status).toBe('error')
  })
})
