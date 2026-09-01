import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import AccountLink from '@/components/AccountLink.vue'
import type { Account, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

// AccountLink 经 useRouter 跳转（pushMock 断言导航目标，issue #97/#99）
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '现金',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
})

describe('AccountLink 账户名下钻（issue #97/#99）', () => {
  it('渲染为真实 button（键盘可达）并带 title 提示', async () => {
    const wrapper = mount(AccountLink, { props: { accountId: 'acc-1' } })
    await flushPromises()
    const btn = wrapper.find('button.account-link')
    expect(btn.exists()).toBe(true)
    expect(btn.attributes('title')).toBe('查看该账户的交易')
    expect(btn.text()).toBe('现金')
  })

  it('默认态使用主题强调色（琥珀）', async () => {
    const wrapper = mount(AccountLink, { props: { accountId: 'acc-1' } })
    await flushPromises()
    const btn = wrapper.find('button')
    // 强调色注入到 style（暗色主题琥珀 #F59E0B，jsdom 归一化为 rgb）。
    // hover 亮琥珀 + 下划线 + 背景微亮、focus-visible 焦点环为组件静态 CSS
    // （jsdom 不注入 scoped 样式，无法在此断言；见 AccountLink.vue 样式块）。
    expect(btn.attributes('style')).toContain('rgb(245, 158, 11)')
  })

  it('点击跳转 /transactions?account=<id>', async () => {
    const wrapper = mount(AccountLink, { props: { accountId: 'acc-1' } })
    await flushPromises()
    await wrapper.find('button').trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'transactions', query: { account: 'acc-1' } })
  })

  it('账户不在参考数据中（黑洞/隐藏账户）渲染纯文本「-」，不可点击、无跳转', async () => {
    const wrapper = mount(AccountLink, { props: { accountId: 'ghost-acc' } })
    await flushPromises()
    // 黑洞/隐藏账户渲染为 span（非 button）：无下钻、无强调色
    expect(wrapper.find('button').exists()).toBe(false)
    const ph = wrapper.find('span.account-placeholder')
    expect(ph.exists()).toBe(true)
    expect(ph.text()).toBe('-')
    expect(ph.attributes('title')).toBeUndefined()
    // 纯文本点击不触发跳转
    await ph.trigger('click')
    expect(pushMock).not.toHaveBeenCalled()
  })

  it('外部传入的布局样式透传到根按钮（转账行内容宽度/左对齐依赖，issue #99）', async () => {
    const wrapper = mount(AccountLink, {
      props: { accountId: 'acc-1' },
      attrs: { style: 'flex: 0 1 auto; min-width: 0; text-align: left;' },
    })
    await flushPromises()
    const btn = wrapper.find('button')
    const style = (btn.element as HTMLElement).style
    // 内容宽度不增长（flex-grow:0），可收缩省略（flex-shrink:1），文本左对齐顶列左缘
    expect(style.flexGrow).toBe('0')
    expect(style.flexShrink).toBe('1')
    expect(style.minWidth).toBe('0px')
    expect(style.textAlign).toBe('left')
    // 组件内部强调色与外部布局样式合并，互不覆盖（暗色琥珀归一化为 rgb）
    expect(btn.attributes('style')).toContain('rgb(245, 158, 11)')
  })
})
