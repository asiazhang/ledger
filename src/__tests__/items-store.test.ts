import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useItemsStore } from '@/stores/items'
import type { ItemInput, ItemWithDailyCost } from '@/types'

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

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
    created_at: '2025-01-01T00:00:00Z',
    updated_at: '2025-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    used_days: 1000,
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

describe('useItemsStore', () => {
  it('首次访问自动加载（self-init），加载后 status=ready、version=1', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_items') return Promise.resolve([baseItem()])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = useItemsStore()
    await flushPromises()
    expect(store.items).toHaveLength(1)
    expect(store.items[0].name).toBe('手机')
    expect(store.status).toBe('ready')
    expect(store.version).toBe(1)
  })

  it('加载失败时 status=error，不抛出（self-init 静默）', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_items') return Promise.reject(new Error('boom'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = useItemsStore()
    await flushPromises()
    expect(store.status).toBe('error')
    expect(store.items).toEqual([])
  })

  it('ledger:changed 触发重拉（stale-while-revalidate：在途不闪空，成功后整体替换）', async () => {
    const initial = [baseItem()]
    const fresh = [baseItem(), baseItem({ id: 'item-2', name: '笔记本' })]
    let resolveSecond: (items: ItemWithDailyCost[]) => void = () => {}
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd !== 'list_items') return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      listCalls++
      if (listCalls === 1) return Promise.resolve(initial)
      return new Promise<ItemWithDailyCost[]>((resolve) => {
        resolveSecond = resolve
      })
    })
    const store = useItemsStore()
    await flushPromises()
    expect(store.items).toEqual(initial)
    expect(store.version).toBe(1)

    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    // 重拉在途：旧数据保留（不闪空）
    expect(store.items).toEqual(initial)
    expect(store.status).toBe('loading')

    resolveSecond(fresh)
    await flushPromises()
    expect(store.items).toEqual(fresh)
    expect(store.version).toBe(2)
    expect(store.status).toBe('ready')
  })

  it('create 调用 create_item 后立即重拉，创建返回即可见新物品', async () => {
    const initial = [baseItem()]
    const created = baseItem({ id: 'item-new', name: '笔记本', total_cost_cents: 500_000 })
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_items') {
        listCalls++
        return Promise.resolve(listCalls === 1 ? initial : [...initial, created])
      }
      if (cmd === 'create_item') {
        expect(args).toEqual({ input: createInput })
        return Promise.resolve('item-new')
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_items') {
        listCalls++
        return Promise.resolve(listCalls === 1 ? before : after)
      }
      if (cmd === 'update_item') {
        expect(args).toEqual({ id: 'item-1', input: updateInput })
        return Promise.resolve(null)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_items') return Promise.resolve(initial)
      if (cmd === 'update_item') return Promise.reject(new Error('物品不存在'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_items') {
        listCalls++
        return Promise.resolve(listCalls === 1 ? before : after)
      }
      if (cmd === 'dispose_item') {
        expect(args).toEqual({ id: 'item-1', input: disposeInput })
        return Promise.resolve(null)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_items') return Promise.resolve(initial)
      if (cmd === 'dispose_item') return Promise.reject(new Error('处置日期早于购买日期'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_items') {
        listCalls++
        return Promise.resolve(listCalls === 1 ? initial : after)
      }
      if (cmd === 'delete_item') {
        expect(args).toEqual({ id: 'item-1' })
        return Promise.resolve()
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
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
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_items') return Promise.resolve(initial)
      if (cmd === 'delete_item') return Promise.reject(new Error('物品不存在'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const store = useItemsStore()
    await flushPromises()

    await expect(store.remove('item-1')).rejects.toThrow('物品不存在')
    expect(store.items).toEqual(initial)
  })
})
