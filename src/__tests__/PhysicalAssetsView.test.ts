import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount, DOMWrapper } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import PhysicalAssetsView from '@/views/PhysicalAssetsView.vue'
import PhysicalAssetFormModal from '@/components/PhysicalAssetFormModal.vue'
import { makePhysicalAsset, makePhysicalAssetList } from './factories'
import type { Currency, PhysicalAsset, PhysicalAssetList } from '@/types'

const mockInvoke = vi.mocked(invoke)

// NModal 内容 teleport 到 document.body：测试在 body 中查询/触发（同 PoliciesView 先例）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

/** 弹窗内输入框：NInput 外层带 data-testid，真 input 在内部（先例 PoliciesView formInput）。 */
function formInput(testid: string) {
  const modal = bodyQuery('[data-testid="physical-asset-form-modal"]')!
  return new DOMWrapper<HTMLInputElement>(modal.querySelector(`[data-testid="${testid}"] input`))
}

function saveButton() {
  return new DOMWrapper<HTMLButtonElement>(bodyQuery('[data-testid="physical-asset-save"]')!)
}

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

function baseAsset(over: Partial<PhysicalAsset> = {}): PhysicalAsset {
  return makePhysicalAsset({ id: 'asset-1', ...over })
}

let list: PhysicalAssetList

function setupInvoke() {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_physical_assets') return Promise.resolve(list)
    if (cmd === 'create_physical_asset') {
      const { input } = args as { input: { name: string; initial_valuation_cents: number } }
      const id = `asset-new-${input.name}`
      list = makePhysicalAssetList({
        assets: [
          baseAsset({
            id,
            name: input.name,
            current_valuation_cents: input.initial_valuation_cents,
            current_valuation_native_cents: input.initial_valuation_cents,
          }),
        ],
        holding_total_native_cents: input.initial_valuation_cents,
      })
      return Promise.resolve(id)
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  list = makePhysicalAssetList()
  setupInvoke()
})

describe('PhysicalAssetsView 实物资产视图冒烟（issue #466）', () => {
  it('挂载即拉取：合计卡显示在持估值合计（折本位币），列表渲染名称/估值/日期/状态', async () => {
    list = makePhysicalAssetList({
      assets: [baseAsset()],
      holding_total_native_cents: 5_000_000,
    })
    const wrapper = mount(PhysicalAssetsView)
    await flushPromises()
    expect(wrapper.find('[data-testid="physical-asset-holding-total"]').text()).toContain('5,0000')
    expect(wrapper.text()).toContain('客厅油画')
    expect(wrapper.text()).toContain('2026-01-01')
    expect(wrapper.text()).toContain('在持')
  })

  it('空列表显示空态引导', async () => {
    const wrapper = mount(PhysicalAssetsView)
    await flushPromises()
    expect(wrapper.find('[data-testid="physical-asset-empty-guide"]').exists()).toBe(true)
  })

  it('点「新建资产」打开建档弹窗：表单字段就位', async () => {
    const wrapper = mount(PhysicalAssetsView)
    await flushPromises()
    await wrapper.find('[data-testid="physical-asset-new"]').trigger('click')
    await flushPromises()
    const modal = bodyQuery('[data-testid="physical-asset-form-modal"]')
    expect(modal).not.toBeNull()
    expect(formInput('physical-asset-name').exists()).toBe(true)
    expect(formInput('physical-asset-valuation').exists()).toBe(true)
  })

  it('建档成功：调用 create_physical_asset 后列表与合计刷新、弹窗关闭', async () => {
    const wrapper = mount(PhysicalAssetsView)
    await flushPromises()
    await wrapper.find('[data-testid="physical-asset-new"]').trigger('click')
    await flushPromises()
    expect(bodyQuery('[data-testid="physical-asset-form-modal"]')).not.toBeNull()
    await formInput('physical-asset-name').setValue('代步车')
    await formInput('physical-asset-valuation').setValue('80000')
    await saveButton().trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'create_physical_asset')
    expect(call).toBeTruthy()
    expect(call![1]).toMatchObject({
      input: {
        name: '代步车',
        initial_valuation_cents: 8_000_000,
        initial_valuation_currency_code: 'CNY',
        purchase_price_cents: null,
        purchase_currency_code: null,
      },
    })
    // 列表经 store 重拉出现新资产，合计随之更新
    expect(wrapper.text()).toContain('代步车')
    expect(wrapper.find('[data-testid="physical-asset-holding-total"]').text()).toContain('8,0000')
    // 弹窗关闭（update:show = false）
    expect(
      wrapper.findComponent(PhysicalAssetFormModal).emitted('update:show'),
    ).toContainEqual([false])
  })

  it('建档缺名称：客户端校验拦截，不调用 create_physical_asset', async () => {
    const wrapper = mount(PhysicalAssetsView)
    await flushPromises()
    await wrapper.find('[data-testid="physical-asset-new"]').trigger('click')
    await flushPromises()
    await formInput('physical-asset-valuation').setValue('80000')
    let createCalls = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_physical_assets') return Promise.resolve(list)
      if (cmd === 'create_physical_asset') {
        createCalls++
        return Promise.resolve('x')
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await saveButton().trigger('click')
    await flushPromises()
    expect(createCalls).toBe(0)
  })
})
