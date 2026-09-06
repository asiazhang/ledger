import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import SettingsView from '@/views/SettingsView.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Currency } from '@/types'


const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  stubReferenceInvoke({
    list_currencies: mockCurrencies,
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
})

describe('SettingsView 不含同步入口（issue #111）', () => {
  it('设置页不渲染任何同步入口与同步文案', async () => {
    const wrapper = mount(SettingsView)
    expect(wrapper.html()).not.toContain('开始同步')
    expect(wrapper.html()).not.toContain('全量同步')
    expect(wrapper.html()).not.toContain('东方财富')
    expect(wrapper.html()).not.toContain('数据管理')
  })

  it('包含 CategoryManager 组件（「分类」Tab 内）', async () => {
    const wrapper = mount(SettingsView)
    // issue #157 后 CategoryManager 位于分类 pane（默认不激活、不挂载）；
    // ADR-0034 后原「分类与币种」更名「分类」。
    const tab = wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分类')
    expect(tab).toBeTruthy()
    await tab!.trigger('click')
    await nextTick()
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
  })
})
