import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'
import {
  invokeHandler,
  mockAccounts,
  mockCurrencies,
  mockHoldings,
  mockInstruments,
} from './factories'

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

// NCard 内组件直接挂载在 wrapper 下，但统一沿用项目的清理约定
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

/** 默认 invoke mock：参考数据 + 持仓 + 持仓标的字典 + 增量同步 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [],
        list_merchants: [],
        list_holdings: mockHoldings,
        list_instruments: { items: mockInstruments, total: mockInstruments.length },
        sync_holding_prices: { synced: 2, skipped: 0, message: '已同步 2 只，跳过 0 只' },
      },
      extra,
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(() => {})
  baseInvoke()
  const store = useReferenceStore()
  await store.ensureFresh()
})

let wrapper: ReturnType<typeof mount> | undefined

async function cellText(colKey: string): Promise<string[]> {
  await nextTick()
  return wrapper!.findAll(`td[data-col-key="${colKey}"]`).map((c) => c.text())
}

describe('HoldingsOverview 当前持仓概览卡（issue #110）', () => {
  it('渲染总市值与未实现盈亏合计（排除无行情行）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.text()).toContain('当前持仓')
    expect(wrapper.text()).toContain('总市值')
    expect(wrapper.text()).toContain('¥1500')
    expect(wrapper.text()).toContain('未实现盈亏合计')
    expect(wrapper.text()).toContain('¥300')
  })

  it('渲染持仓明细表列：标的/数量/成本/现价/市值/未实现盈亏', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const headers = wrapper.findAll('th').map((th) => th.text())
    for (const h of ['标的', '名称', '账户', '数量', '成本', '现价', '市值', '未实现盈亏']) {
      expect(headers).toContain(h)
    }
    // 行数据来自 mock
    expect(await cellText('symbol')).toEqual(['600000', '000001'])
    expect(await cellText('quantity')).toEqual(['100', '10'])
    expect(await cellText('cost_basis')).toEqual(['¥1200', '¥80'])
    // 无行情行显示 -
    expect(await cellText('latest_price')).toEqual(['¥15', '-'])
    expect(await cellText('market_value')).toEqual(['¥1500', '-'])
    expect(await cellText('unrealized_pnl')).toEqual(['¥300', '-'])
  })

  it('无持仓时显示空态', async () => {
    baseInvoke({ list_holdings: [], list_instruments: { items: [], total: 0 } })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.find('.n-empty').exists()).toBe(true)
    expect(wrapper.text()).toContain('暂无持仓')
  })

  it('右上角「同步持仓价格」按钮触发增量同步命令，反馈与标的页一致', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const btn = wrapper.find('[data-testid="sync-holding-prices"]')
    expect(btn.exists()).toBe(true)
    await btn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('sync_holding_prices')
    // 同样的轻量反馈
    expect(wrapper.text()).toContain('已同步 2 只，跳过 0 只')
  })

  it('同步进行中按钮 loading', async () => {
    let resolveSync!: (v: unknown) => void
    baseInvoke({
      sync_holding_prices: () =>
        new Promise((res) => {
          resolveSync = res
        }),
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await nextTick()
    expect(wrapper.find('.n-button--loading').exists()).toBe(true)
    resolveSync({ synced: 2, skipped: 0, message: '已同步 2 只，跳过 0 只' })
    await flushPromises()
    expect(wrapper.find('.n-button--loading').exists()).toBe(false)
  })

  it('同步成功后重新拉取持仓（现价/市值随最新价刷新）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    const callsAfter = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    expect(callsAfter).toBeGreaterThan(callsBefore)
  })

  it('同步失败显示错误消息', async () => {
    baseInvoke({ sync_holding_prices: () => Promise.reject(new Error('网络错误')) })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('同步失败：网络错误')
  })
})
