import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { usePhysicalAssetsStore } from '@/stores/physicalAssets'
import { makePhysicalAsset, makePhysicalAssetList } from './factories'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type {
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetList,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
} from '@/types'

const mockListen = vi.mocked(listen)

function baseAsset(over: Partial<PhysicalAsset> = {}): PhysicalAsset {
  return makePhysicalAsset({ id: 'asset-1', ...over })
}

const createInput: PhysicalAssetInput = {
  name: '代步车',
  purchase_date: '2023-05-01',
  purchase_price_cents: 12_000_000_00,
  purchase_currency_code: 'CNY',
  initial_valuation_cents: 8_000_000_00,
  initial_valuation_currency_code: 'CNY',
  initial_valuation_date: null,
}

/** 捕获 ledger:changed 监听处理器（store 创建时注册） */
let handlers: Array<(evt: unknown) => void>

/** 基础派发：各测试领域链处理完自己的命令后委托回它（参考命令同老链保持空保司表） */
let base: ReturnType<typeof stubReferenceInvoke>

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  handlers = []
  mockListen.mockImplementation(async (_evt, handler) => {
    handlers.push(handler)
    return vi.fn()
  })
  base = stubReferenceInvoke({ list_insurers: [] })
})

describe('usePhysicalAssetsStore', () => {
  it('首次访问自动加载（self-init）：列表与在持合计同批就位，status=ready', async () => {
    const asset = baseAsset()
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'list_physical_assets'
        ? Promise.resolve(
            makePhysicalAssetList({ assets: [asset], holding_total_native_cents: 5_000_000 }),
          )
        : base(cmd, args),
    )
    const store = usePhysicalAssetsStore()
    await flushPromises()
    expect(store.assets).toHaveLength(1)
    expect(store.assets[0].name).toBe('客厅油画')
    expect(store.holdingTotalNativeCents).toBe(5_000_000)
    expect(store.nativeCurrency).toBe('CNY')
    expect(store.status).toBe('ready')
    expect(store.version).toBe(1)
  })

  it('加载失败时 status=error，不抛出（self-init 静默；缺汇率报错走同一通道）', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'list_physical_assets'
        ? Promise.reject(new Error('未找到 USD -> CNY 的汇率'))
        : base(cmd, args),
    )
    const store = usePhysicalAssetsStore()
    await flushPromises()
    expect(store.status).toBe('error')
    expect(store.assets).toEqual([])
  })

  it('ledger:changed 触发静默重拉（stale-while-revalidate：在途不闪空，成功后整体替换）', async () => {
    const initial = [baseAsset()]
    const fresh = [baseAsset(), baseAsset({ id: 'asset-2', name: '代步车' })]
    let resolveSecond: (list: PhysicalAssetList) => void = () => {}
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd !== 'list_physical_assets') return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      listCalls++
      if (listCalls === 1) return Promise.resolve(makePhysicalAssetList({ assets: initial, holding_total_native_cents: 5_000_000 }))
      return new Promise<PhysicalAssetList>((resolve) => {
        resolveSecond = resolve
      })
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    expect(store.assets).toEqual(initial)

    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    // 第二次拉取在途：旧数据保留，不闪空
    expect(store.assets).toEqual(initial)
    resolveSecond(makePhysicalAssetList({ assets: fresh, holding_total_native_cents: 13_000_000 }))
    await flushPromises()
    expect(store.assets).toEqual(fresh)
    expect(store.holdingTotalNativeCents).toBe(13_000_000)
    expect(store.version).toBe(2)
  })

  it('create 成功后立即重拉并返回 id（建档后列表与合计随之更新）', async () => {
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        listCalls++
        return Promise.resolve(
          listCalls > 1
            ? makePhysicalAssetList({
                assets: [baseAsset({ id: 'new-1', name: '代步车', current_valuation_cents: 8_000_000_00, current_valuation_native_cents: 8_000_000_00 })],
                holding_total_native_cents: 8_000_000_00,
              })
            : makePhysicalAssetList(),
        )
      }
      if (cmd === 'create_physical_asset') {
        expect(args).toMatchObject({ input: createInput })
        return Promise.resolve('new-1')
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    expect(store.assets).toHaveLength(0)
    const id = await store.create(createInput)
    expect(id).toBe('new-1')
    await flushPromises()
    expect(store.assets).toHaveLength(1)
    expect(store.holdingTotalNativeCents).toBe(8_000_000_00)
  })

  it('create 失败不重拉、错误上抛（由调用方 toast 展示）', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'list_physical_assets') return Promise.resolve(makePhysicalAssetList())
      if (cmd === 'create_physical_asset') return Promise.reject(new Error('资产名称不能为空'))
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await expect(store.create({ ...createInput, name: '' })).rejects.toThrow('资产名称不能为空')
    expect(store.version).toBe(1)
  })

  it('updateValuation 成功后立即重拉（当前估值变为最新一条，issue #467 T2）', async () => {
    const valuationInput: PhysicalAssetValuationInput = {
      amount_cents: 6_000_000_00,
      currency_code: 'CNY',
      valuation_date: null,
    }
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        listCalls++
        return Promise.resolve(
          listCalls > 1
            ? makePhysicalAssetList({
                assets: [baseAsset({ current_valuation_cents: 6_000_000_00, current_valuation_native_cents: 6_000_000_00 })],
                holding_total_native_cents: 6_000_000_00,
              })
            : makePhysicalAssetList({ assets: [baseAsset()] }),
        )
      }
      if (cmd === 'update_physical_asset_valuation') {
        expect(args).toMatchObject({ id: 'asset-1', input: valuationInput })
        return Promise.resolve()
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await store.updateValuation('asset-1', valuationInput)
    await flushPromises()
    expect(store.assets[0].current_valuation_cents).toBe(6_000_000_00)
    expect(store.version).toBe(2)
  })

  it('updateValuation 失败不重拉、错误上抛（未来日期守卫由后端报错）', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'list_physical_assets') return Promise.resolve(makePhysicalAssetList({ assets: [baseAsset()] }))
      if (cmd === 'update_physical_asset_valuation') return Promise.reject(new Error('估值日期 9999-12-31 不能是未来'))
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await expect(
      store.updateValuation('asset-1', { amount_cents: 1, currency_code: 'CNY', valuation_date: '9999-12-31' }),
    ).rejects.toThrow('不能是未来')
    expect(store.version).toBe(1)
  })

  it('update 成功后立即重拉（编辑名称 / 购买信息读回一致，issue #467 T2）', async () => {
    const updateInput: PhysicalAssetUpdateInput = {
      name: '家用代步车',
      purchase_date: '2023-06-01',
      purchase_price_cents: 11_000_000_00,
      purchase_currency_code: 'CNY',
    }
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        listCalls++
        return Promise.resolve(
          listCalls > 1
            ? makePhysicalAssetList({ assets: [baseAsset({ name: '家用代步车' })] })
            : makePhysicalAssetList({ assets: [baseAsset({ name: '代步车' })] }),
        )
      }
      if (cmd === 'update_physical_asset') {
        expect(args).toMatchObject({ id: 'asset-1', input: updateInput })
        return Promise.resolve()
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await store.update('asset-1', updateInput)
    await flushPromises()
    expect(store.assets[0].name).toBe('家用代步车')
    expect(store.version).toBe(2)
  })

  it('update 失败不重拉、错误上抛（由调用方 toast 展示）', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'list_physical_assets') return Promise.resolve(makePhysicalAssetList({ assets: [baseAsset()] }))
      if (cmd === 'update_physical_asset') return Promise.reject(new Error('资产名称不能为空'))
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await expect(store.update('asset-1', { name: '', purchase_date: null, purchase_price_cents: null, purchase_currency_code: null }))
      .rejects.toThrow('资产名称不能为空')
    expect(store.version).toBe(1)
  })

  it('dispose 成功后立即重拉（处置流：资产退出默认列表，issue #468 T3）', async () => {
    const disposeInput: PhysicalAssetDisposeInput = {
      disposal_date: '2026-08-01',
      disposal_price_cents: 60_000_000_00,
      disposal_currency_code: 'CNY',
    }
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        listCalls++
        return Promise.resolve(
          listCalls > 1
            ? makePhysicalAssetList({ assets: [], holding_total_native_cents: 0 })
            : makePhysicalAssetList({ assets: [baseAsset()] }),
        )
      }
      if (cmd === 'dispose_physical_asset') {
        expect(args).toMatchObject({ id: 'asset-1', input: disposeInput })
        return Promise.resolve()
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await store.dispose('asset-1', disposeInput)
    await flushPromises()
    expect(store.assets).toHaveLength(0)
    expect(store.holdingTotalNativeCents).toBe(0)
    expect(store.version).toBe(2)
  })

  it('dispose 失败不重拉、错误上抛（缺处置日期守卫由后端报错）', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'list_physical_assets') return Promise.resolve(makePhysicalAssetList({ assets: [baseAsset()] }))
      if (cmd === 'dispose_physical_asset') return Promise.reject(new Error('处置日期不能为空'))
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await expect(store.dispose('asset-1', { disposal_date: null, disposal_price_cents: null, disposal_currency_code: null }))
      .rejects.toThrow('处置日期不能为空')
    expect(store.version).toBe(1)
  })

  it('remove 成功后立即重拉（软删过滤：资产退出列表与合计，issue #468 T3）', async () => {
    let listCalls = 0
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        listCalls++
        return Promise.resolve(
          listCalls > 1
            ? makePhysicalAssetList({ assets: [], holding_total_native_cents: 0 })
            : makePhysicalAssetList({ assets: [baseAsset()] }),
        )
      }
      if (cmd === 'delete_physical_asset') {
        expect(args).toMatchObject({ id: 'asset-1' })
        return Promise.resolve()
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    await store.remove('asset-1')
    await flushPromises()
    expect(store.assets).toHaveLength(0)
    expect(store.holdingTotalNativeCents).toBe(0)
    expect(store.version).toBe(2)
  })

  it('setStatusFilter 切换状态筛选并重拉（已处置筛选回看档案，默认在持）', async () => {
    const holding = [baseAsset()]
    const disposed = [baseAsset({ id: 'asset-9', name: '旧车', status: 'disposed', current_valuation_native_cents: null })]
    const seenStatus: string[] = []
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'list_physical_assets') {
        seenStatus.push((args as { status: string | null }).status ?? 'holding')
        const status = (args as { status: string | null }).status
        return Promise.resolve(
          status === 'disposed'
            ? makePhysicalAssetList({ assets: disposed, holding_total_native_cents: 5_000_000 })
            : makePhysicalAssetList({ assets: holding, holding_total_native_cents: 5_000_000 }),
        )
      }
      return base(cmd, args)
    })
    const store = usePhysicalAssetsStore()
    await flushPromises()
    expect(seenStatus[0]).toBe('holding')
    expect(store.statusFilter).toBe('holding')
    await store.setStatusFilter('disposed')
    await flushPromises()
    expect(store.statusFilter).toBe('disposed')
    expect(store.assets[0].name).toBe('旧车')
    expect(store.assets[0].status).toBe('disposed')
    // 在持合计口径与筛选无关（回看已处置时合计不变）
    expect(store.holdingTotalNativeCents).toBe(5_000_000)
    // ledger:changed 重拉沿用当前筛选，不回退默认口径
    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    expect(seenStatus[seenStatus.length - 1]).toBe('disposed')
  })
})
