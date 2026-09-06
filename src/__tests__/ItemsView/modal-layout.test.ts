import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import ItemsView from '@/views/ItemsView.vue'
import type { ItemWithDailyCost } from '@/types'

// 物品弹窗族排版统一（issue #634，spec #630）：三个弹窗的卡片外观收敛为
// AppModal cardSize 单一声明——编辑/处置归 sm（420）、详情归 md（480）；
// 显式 style 宽度由 cardSize 承担，无边框由 AppModal 默认承担。断言只看
// 组件可观察输出（卡片宽度样式与边框类），不深究 naive-ui 内部实现。

const mockInvoke = vi.mocked(invoke)

vi.mock('vue-router', () => ({
  useRouter: () => ({ push: vi.fn() }),
}))

// NModal 内容 teleport 到 document.body：测试在 body 中查询（同 ItemsView.test.ts 先例）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockItem: ItemWithDailyCost = {
  id: 'item-1',
  name: '手机',
  purchase_date: '2025-01-01',
  total_cost_cents: 1_000_000,
  currency_code: 'CNY',
  cost_native_cents: 1_000_000,
  status: 'in_use',
  disposal_date: null,
  residual_value_cents: null,
  purchase_transaction_id: null,
  note: null,
  created_at: '2025-01-01T00:00:00Z',
  updated_at: '2025-01-01T00:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
  used_days: 1000,
  per_day_cents: 1000,
}

const mockCurrencies = [{ code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    // 本测试不依赖交易候选，任意 kind 一律空列表
    if (cmd === 'list_transactions') return Promise.resolve({ items: [], total: 0 })
    if (cmd === 'list_items') return Promise.resolve([mockItem])
    if (cmd === 'list_insurers') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  // 参考数据（币种选项）与物品 store 均为 self-init，提前预热
  await flushPromises()
})

/** 取弹窗卡片元素：preset="card" 下 $attrs（含 data-testid）落在 NCard 根元素。 */
function modalCard(testid: string): HTMLElement {
  const modal = document.body.querySelector<HTMLElement>(`[data-testid="${testid}"]`)
  expect(modal, `应能找到弹窗 [${testid}]`).not.toBeNull()
  expect(modal!.classList.contains('n-card'), '弹窗根应为 NCard 卡片').toBe(true)
  return modal!
}

/** 断言弹窗卡片：宽度档位 + 无边框（AppModal 默认，调用点不再显式声明）。 */
function expectCardSize(card: HTMLElement, width: string) {
  expect(card.style.width).toBe(width)
  expect(card.classList.contains('n-card--bordered')).toBe(false)
}

async function openModalByButton(wrapper: ReturnType<typeof mount>, text: string) {
  const btn = wrapper.findAll('button').find((b) => b.text() === text)
  expect(btn, `应能找到「${text}」按钮`).toBeTruthy()
  await btn!.trigger('click')
  await flushPromises()
}

describe('ItemsView 物品弹窗族排版统一（issue #634）', () => {
  it.each([
    ['编辑', 'item-edit-modal', '420px'],
    ['处置', 'item-dispose-modal', '420px'],
    ['详情', 'item-detail-modal', '480px'],
  ])('「%s」弹窗卡片宽度归对应档位且默认无边框', async (buttonText, testid, width) => {
    const wrapper = mount(ItemsView)
    await flushPromises()

    await openModalByButton(wrapper, buttonText)
    expectCardSize(modalCard(testid), width)
  })
})
