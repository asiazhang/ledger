import { describe, it, expect } from 'vitest'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { useItemsStore } from '@/item/items'
import type { ItemInput, ItemWithDailyCost } from '@ledger/types'

function baseItem(over: Partial<ItemWithDailyCost> = {}): ItemWithDailyCost {
  return {
    id: 'item-1',
    name: '手机',
    purchase_date: '2025-01-01',
    total_cost_cents: 1_000_000,
    currency_code: 'CNY',
    cost_native_cents: 1_000_000,
    status: 'in_use',
    disposal_date: null,
    residual_value_cents: null,
    note: null,
    purchase_transaction_id: null,
    created_at: '2025-01-01T00:00:00Z',
    updated_at: '2025-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    used_days: 1000,
    numerator_cents: 1_000_000,
    per_day_cents: 1000,
    ...over,
  }
}

const createInput: ItemInput = {
  name: '笔记本',
  purchase_date: '2026-01-01',
  total_cost_cents: 500_000,
  currency_code: 'CNY',
  note: null,
}

/**
 * 机制断言（self-init / SWR / 在途合并 / 事件重拉 / status-version）已收口到
 * push-first-list.test.ts 工厂单点（ADR-0123 决策 6）；本文件只留领域动作断言。
 */

describe('useItemsStore', () => {
  it('create 调用 create_item 后立即重拉，创建返回即可见新物品', async () => {
    const initial = [baseItem()]
    const created = baseItem({ id: 'item-new', name: '笔记本', total_cost_cents: 500_000 })
    let listCalls = 0
    wireInvokeSeam({
      overrides: {
        list_items: () => {
          listCalls++
          return Promise.resolve(listCalls === 1 ? initial : [...initial, created])
        },
        create_item: (args) => {
          expect(args).toEqual({ input: createInput })
          return 'item-new'
        },
      },
    })
    const store = useItemsStore()
    await flushPromises()

    const id = await store.create(createInput)
    expect(id).toBe('item-new')
    expect(listCalls).toBe(2)
    expect(store.items).toHaveLength(2)
    expect(store.items.some((i) => i.id === 'item-new')).toBe(true)
  })

  it('update 按 id 调用 update_item 后立即重拉，修改即可见', async () => {
    const before = [baseItem()]
    const after = [baseItem({ name: '手机 Pro', version: 2 })]
    let listCalls = 0
    const updateInput: ItemInput = {
      name: '手机 Pro',
      purchase_date: '2025-02-02',
      total_cost_cents: 1_200_000,
      currency_code: 'CNY',
      note: '顶配',
    }
    wireInvokeSeam({
      overrides: {
        list_items: () => {
          listCalls++
          return Promise.resolve(listCalls === 1 ? before : after)
        },
        update_item: (args) => {
          expect(args).toEqual({ id: 'item-1', input: updateInput })
          return null
        },
      },
    })
    const store = useItemsStore()
    await flushPromises()

    await store.update('item-1', updateInput)
    expect(listCalls).toBe(2)
    expect(store.items[0].name).toBe('手机 Pro')
    expect(store.items[0].version).toBe(2)
  })

  it('update 失败时抛出错误且不重拉', async () => {
    const initial = [baseItem()]
    wireInvokeSeam({
      defaults: { list_items: initial },
      overrides: { update_item: () => Promise.reject(new Error('物品不存在')) },
    })
    const store = useItemsStore()
    await flushPromises()
    const versionBefore = store.version

    await expect(
      store.update('no-such', {
        name: 'x',
        purchase_date: '2025-01-01',
        total_cost_cents: 100,
        currency_code: 'CNY',
      }),
    ).rejects.toThrow('物品不存在')
    expect(store.version).toBe(versionBefore)
    expect(store.items[0].name).toBe('手机')
  })

  it('dispose 按 id 调用 dispose_item 后立即重拉，处置信息即可见（issue #120）', async () => {
    const before = [baseItem()]
    const after = [
      baseItem({
        status: 'disposed',
        disposal_date: '2026-01-10',
        residual_value_cents: 20_000,
        version: 2,
      }),
    ]
    let listCalls = 0
    const disposeInput = { disposal_date: '2026-01-10', residual_value_cents: 20_000 }
    wireInvokeSeam({
      overrides: {
        list_items: () => {
          listCalls++
          return Promise.resolve(listCalls === 1 ? before : after)
        },
        dispose_item: (args) => {
          expect(args).toEqual({ id: 'item-1', input: disposeInput })
          return null
        },
      },
    })
    const store = useItemsStore()
    await flushPromises()

    await store.dispose('item-1', disposeInput)
    expect(listCalls).toBe(2)
    expect(store.items[0].status).toBe('disposed')
    expect(store.items[0].disposal_date).toBe('2026-01-10')
    expect(store.items[0].residual_value_cents).toBe(20_000)
  })

  it('dispose 失败时抛出错误且不重拉', async () => {
    const initial = [baseItem()]
    wireInvokeSeam({
      defaults: { list_items: initial },
      overrides: { dispose_item: () => Promise.reject(new Error('处置日期早于购买日期')) },
    })
    const store = useItemsStore()
    await flushPromises()
    const versionBefore = store.version

    await expect(
      store.dispose('item-1', { disposal_date: '2024-12-31', residual_value_cents: null }),
    ).rejects.toThrow('处置日期早于购买日期')
    expect(store.version).toBe(versionBefore)
    expect(store.items[0].status).toBe('in_use')
  })

  it('remove 调用 delete_item 后立即重拉，已删物品从列表消失', async () => {
    const initial = [baseItem()]
    const after = [] as ItemWithDailyCost[]
    let listCalls = 0
    wireInvokeSeam({
      overrides: {
        list_items: () => {
          listCalls++
          return Promise.resolve(listCalls === 1 ? initial : after)
        },
        delete_item: (args) => {
          expect(args).toEqual({ id: 'item-1' })
        },
      },
    })
    const store = useItemsStore()
    await flushPromises()

    await store.remove('item-1')
    expect(listCalls).toBe(2)
    expect(store.items).toHaveLength(0)
    expect(store.status).toBe('ready')
  })

  it('remove 失败时抛出错误且不重拉', async () => {
    const initial = [baseItem()]
    wireInvokeSeam({
      defaults: { list_items: initial },
      overrides: { delete_item: () => Promise.reject(new Error('物品不存在')) },
    })
    const store = useItemsStore()
    await flushPromises()

    await expect(store.remove('item-1')).rejects.toThrow('物品不存在')
    expect(store.items).toEqual(initial)
  })

  it('写入成功后重拉失败不反转写动作成败：动作正常返回，失败信号由 status 承载（ADR-0123 决策 3）', async () => {
    let listCalls = 0
    wireInvokeSeam({
      overrides: {
        list_items: () => {
          listCalls++
          return listCalls === 1 ? Promise.resolve([baseItem()]) : Promise.reject(new Error('重拉失败'))
        },
        create_item: () => 'item-new',
      },
    })
    const store = useItemsStore()
    await flushPromises()

    // 已落库的建档不因重拉失败误报「保存失败」，动作正常返回 id
    await expect(store.create(createInput)).resolves.toBe('item-new')
    expect(store.status).toBe('error')
  })
})
