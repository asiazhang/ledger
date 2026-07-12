import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import SettingsView from '@/views/SettingsView.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import type { Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
  { code: 'JPY', name: '日元', symbol: '¥', decimal_places: 0 },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useAppStore()
  await store.loadAll()
})

describe('SettingsView.vue', () => {
  it('渲染 4 个 TabPane', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const settingsTabs = tabs.filter(
      (t) => ['分类', '币种', '外观', '关于'].includes(t.text()),
    )
    expect(settingsTabs.length).toBe(4)
  })

  it('Tab 标签文本正确', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).toContain('分类')
    expect(labels).toContain('币种')
    expect(labels).toContain('外观')
    expect(labels).toContain('关于')
  })

  it('包含 CategoryManager 组件', () => {
    const wrapper = mount(SettingsView)
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
  })

  it('币种 Tab 包含默认币种选择器', async () => {
    const wrapper = mount(SettingsView)
    const currencyTab = wrapper.findAll('.n-tabs-tab')[1]
    await currencyTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('默认币种')
  })

  it('外观 Tab 包含主题切换开关', async () => {
    const wrapper = mount(SettingsView)
    const appearanceTab = wrapper.findAll('.n-tabs-tab')[2]
    await appearanceTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('深色模式')
  })

  it('关于 Tab 显示版本号', async () => {
    const wrapper = mount(SettingsView)
    const aboutTab = wrapper.findAll('.n-tabs-tab')[3]
    await aboutTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('版本号')
  })
})
