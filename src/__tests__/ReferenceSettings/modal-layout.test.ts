import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import MerchantEditModal from '@/components/merchants/MerchantEditModal.vue'
import CategoryEditModal from '@/components/categories/CategoryEditModal.vue'
import type { Merchant, Category } from '@/types'

// 参考数据管理弹窗排版统一（issue #637，spec #630）：商户编辑与分类编辑
// 两弹窗卡片外观收敛为 AppModal cardSize 单一声明——均归 sm（420），
// 显式 style 宽度由 cardSize 承担，无边框由 AppModal 默认承担；按钮行从
// 单颗全宽 block 主键改为右对齐单主键（保留无取消键的轻量语义）。断言
// 只看组件可观察输出（卡片宽度样式、边框类、按钮行排列），不深究
// naive-ui 内部实现。

const { messageMock } = vi.hoisted(() => ({
  messageMock: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    loading: vi.fn(),
    destroyAll: vi.fn(),
  },
}))

vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return {
    ...actual,
    useMessage: () => messageMock,
  }
})

// NModal 内容传送至 document.body：每测后卸载，避免前一用例的弹窗残留污染查询
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

beforeEach(() => {
  setActivePinia(createPinia())
})

const mockMerchant: Merchant = {
  id: 'mch-1',
  name: '京东',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
}

const mockCategory: Category = {
  id: 'cat-1',
  name: '餐饮',
  kind: 'expense',
  parent_id: null,
  icon: null,
  sort_order: 0,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  version: 1,
  device_id: 'test',
  is_deleted: false,
}

/** 卡片根元素（preset="card" 下卡片即 NCard 根；单测内同时只挂一个弹窗）。 */
function modalCard(): HTMLElement {
  const card = document.body.querySelector<HTMLElement>('.n-card')
  expect(card, '弹窗卡片（NCard）应存在').not.toBeNull()
  return card!
}

/** 断言弹窗卡片：宽度归 sm 档（420）+ 默认无边框（AppModal 默认，调用点不再显式声明）。 */
function expectCardSizeSm(card: HTMLElement) {
  expect(card.style.width).toBe('420px')
  expect(card.classList.contains('n-card--bordered')).toBe(false)
}

/** 断言按钮行：保存主键右对齐（NSpace justify="end"），且不再是全宽 block。 */
function expectRightAlignedSinglePrimary(card: HTMLElement, text: string) {
  const btn = Array.from(card.querySelectorAll('button')).find(
    (b) => b.textContent?.trim() === text,
  )
  expect(btn, `「${text}」主键应存在`).toBeTruthy()
  expect(btn!.classList.contains('n-button--primary-type'), '保存应为 primary 主键').toBe(true)
  expect(btn!.classList.contains('n-button--block'), '保存不应再是全宽 block').toBe(false)
  const row = btn!.closest('.n-space') as HTMLElement | null
  expect(row, '保存应包在 NSpace 按钮行内').not.toBeNull()
  expect(row!.style.justifyContent).toBe('flex-end')
}

describe('参考数据管理弹窗排版统一（issue #637）', () => {
  it('商户编辑弹窗卡片归 sm 档、默认无边框，保存键右对齐非全宽', async () => {
    mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    })
    await flushPromises()

    const card = modalCard()
    expectCardSizeSm(card)
    expectRightAlignedSinglePrimary(card, '保存')
  })

  it('分类编辑弹窗卡片归 sm 档、默认无边框，保存键右对齐非全宽', async () => {
    mount(CategoryEditModal, {
      props: { show: true, category: mockCategory },
    })
    await flushPromises()

    const card = modalCard()
    expectCardSizeSm(card)
    expectRightAlignedSinglePrimary(card, '保存')
  })
})
