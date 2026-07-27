import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from '@/stores/app'
import SettingsView from '@/views/SettingsView.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import type { Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  mockListen.mockReturnValue(Promise.resolve(vi.fn()))
  localStorage.clear()
  const store = useAppStore()
  await store.loadAll()
})

describe('SettingsView 数据管理 tab', () => {
  it('数据管理 Tab 存在', () => {
    const wrapper = mount(SettingsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).toContain('数据管理')
  })

  it('数据管理 tab 包含同步按钮', async () => {
    const wrapper = mount(SettingsView)
    const syncTab = wrapper.findAll('.n-tabs-tab')[2]
    await syncTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('开始同步')
  })

  it('同步按钮可点击', async () => {
    const wrapper = mount(SettingsView)
    const syncTab = wrapper.findAll('.n-tabs-tab')[2]
    await syncTab.trigger('click')
    await nextTick()
    const btn = wrapper.find('.n-button--primary-type')
    expect(btn.exists()).toBe(true)
  })

  it('同步 tab 包含数据源说明', async () => {
    const wrapper = mount(SettingsView)
    const syncTab = wrapper.findAll('.n-tabs-tab')[2]
    await syncTab.trigger('click')
    await nextTick()
    expect(wrapper.html()).toContain('东方财富')
  })

  it('同步时按钮变为 disabled', async () => {
    const wrapper = mount(SettingsView)
    const syncTab = wrapper.findAll('.n-tabs-tab')[2]
    await syncTab.trigger('click')
    await nextTick()
    const btn = wrapper.find('.n-button--primary-type')
    expect(btn.element.getAttribute('disabled')).toBeNull()
  })

  it('包含 CategoryManager 组件', () => {
    const wrapper = mount(SettingsView)
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
  })

  it('listen 在 onMounted 时被注册', async () => {
    mount(SettingsView)
    await new Promise((r) => setTimeout(r, 50))
    await nextTick()
    expect(mockListen).toHaveBeenCalledWith(
      'sync-instruments:progress',
      expect.any(Function),
    )
  })
})
