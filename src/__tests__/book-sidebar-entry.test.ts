import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import BookSidebarEntry from '@/components/BookSidebarEntry.vue'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam, type InvokeSeamOverride } from './helpers/invoke-mock'
import { clickDialogButton, dialogText, findBodyButton, visibleModalText } from './helpers/dom'
import { messageApi, messageCalls } from './helpers/message-mock'
import { resetToastSink } from './factories'
import { registerToastSink } from '@/composables/useLoadable'
import { hasOpenOverlay, resetOverlays } from '@/composables/overlayRegistry'
import type { BookListInfo } from '@/types'

// 侧栏左下角账本入口与弹层（issue #834 / ADR-0089）：弹层逻辑（清单渲染、切换
// 意图、新建/改名/移除交互、折叠态浮标、注册表损坏警示）的组件级行为测试。
// 命令应答一律走 invoke 测试接缝（ADR-0085）；useAppDialog 确认框与 AppModal
// 卡片 teleport 到 body，经 helpers/dom 的 body 查找家族断言。

/** 登记表初始现场：两本账，默认账本活动。 */
let registry: BookListInfo = {
  books: [
    { id: 'b1', name: '默认账本', dir: '/data/default' },
    { id: 'b2', name: '副业账本', dir: '/data/second' },
  ],
  active_id: 'b1',
  mutable: true,
  fallback_reason: null,
}

/** 登记命令的可变应答：命令函数直接改写 registry，模拟注册表落盘后的最新现场。 */
function mutableRegistrySeam(): void {
  const overrides: Record<string, InvokeSeamOverride> = {
    list_books: () => ({ ...registry, books: registry.books.map((b) => ({ ...b })) }),
    create_book: (args) => {
      const book = { id: 'b-new', name: String(args?.name ?? ''), dir: '/data/new' }
      registry = { ...registry, books: [...registry.books, book] }
      return book
    },
    rename_book: (args) => {
      const book = registry.books.find((b) => b.id === args?.id)
      if (!book) throw new Error('book not found')
      book.name = String(args?.name ?? '')
      return { ...book }
    },
    remove_book: (args) => {
      registry = { ...registry, books: registry.books.filter((b) => b.id !== args?.id) }
      return undefined
    },
    switch_book: (args) => {
      registry = { ...registry, active_id: String(args?.id ?? '') }
      const book = registry.books.find((b) => b.id === args?.id)
      if (!book) throw new Error('book not found')
      return { ...book }
    },
  }
  wireInvokeSeam({ overrides })
}

function mountEntry(props: { collapsed: boolean } = { collapsed: false }) {
  return mount(NDialogProvider, {
    slots: { default: () => h(BookSidebarEntry, props) },
  })
}

async function openPanel(wrapper: ReturnType<typeof mountEntry>): Promise<void> {
  await wrapper.find('[data-testid="book-entry"]').trigger('click')
  await flushPromises()
  await nextTick()
}

/** body 上渲染出的账本行文本（弹层 teleport 到 body 后的清单读取形态）。 */
function panelRows(): string[] {
  return Array.from(document.body.querySelectorAll('.book-row')).map((el) => el.textContent ?? '')
}

function bodySelector(selector: string): DOMWrapper<Element> | undefined {
  const el = document.body.querySelector(selector)
  return el ? new DOMWrapper(el) : undefined
}

function callsOf(cmd: string): number {
  return mockInvoke.mock.calls.filter(([c]) => c === cmd).length
}

beforeEach(() => {
  // Loadable 默认错误 toast 经模块级单点 sink；测试把它接到消息替身，供断言读取
  registerToastSink(messageApi)
  resetOverlays()
  registry = {
    books: [
      { id: 'b1', name: '默认账本', dir: '/data/default' },
      { id: 'b2', name: '副业账本', dir: '/data/second' },
    ],
    active_id: 'b1',
    mutable: true,
    fallback_reason: null,
  }
  mutableRegistrySeam()
})

afterEach(() => {
  resetToastSink()
})

describe('账本入口（issue #834）：当前账本名与清单弹层', () => {
  it('入口常驻显示当前账本名（list_books 的活动本）', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    expect(wrapper.find('[data-testid="book-entry"]').text()).toContain('默认账本')
  })

  it('点击入口弹层列出全部账本并勾示活动本（当前 Tag 只在活动行）；打开期间上报弹层注册表', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    await openPanel(wrapper)
    expect(panelRows().map((r) => r.trim())).toHaveLength(2)
    expect(panelRows()[0]).toContain('默认账本')
    expect(panelRows()[0]).toContain('当前')
    expect(panelRows()[1]).toContain('副业账本')
    expect(panelRows()[1]).not.toContain('当前')
    expect(hasOpenOverlay()).toBe(true)
  })

  it('面板开着时再次点击入口关闭（toggle 语义）；注册表上报随之撤销', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    expect(panelRows()).toHaveLength(2)
    expect(hasOpenOverlay()).toBe(true)
    await wrapper.find('[data-testid="book-entry"]').trigger('click')
    await flushPromises()
    await nextTick()
    expect(hasOpenOverlay()).toBe(false)
    expect(panelRows()).toHaveLength(0)
  })

  it('读取失败诚实呈现：入口回退「账本」字样，弹层内给错误行与重试；重试成功恢复清单', async () => {
    // 首次 list_books 拒绝，其后恢复应答（计数桩，重试路径同门）
    let failures = 1
    wireInvokeSeam({
      overrides: {
        list_books: () =>
          failures-- > 0
            ? Promise.reject(new Error('boom'))
            : { ...registry, books: registry.books.map((b) => ({ ...b })) },
      },
    })
    const wrapper = mountEntry()
    await flushPromises()
    expect(wrapper.find('[data-testid="book-entry"]').text()).not.toContain('默认账本')
    // 清单加载失败走 Loadable 默认策略（裸 errorMessage）；动作上下文由弹层错误行承载
    expect(messageCalls().some((m) => m.method === 'error' && m.text === 'boom')).toBe(true)

    await openPanel(wrapper)
    expect(document.body.textContent).toContain('账本清单读取失败')
    const retry = findBodyButton('重试', { exact: true })
    expect(retry).toBeDefined()
    await retry!.trigger('click')
    await flushPromises()
    expect(panelRows()).toHaveLength(2)
  })
})

describe('账本入口（issue #834）：切换经轻量确认后原地重载', () => {
  it('点击其他账本行弹轻量确认（提示应用将重载）；确认后调 switch_book 并提示切换', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await bodySelector('.book-row:nth-child(2)')!.trigger('click')
    await flushPromises()
    // 确认弹窗出现：目标账本名 + 重载提示（非破坏级轻量确认，ADR-0078 语义）
    expect(dialogText()).toContain('副业账本')
    expect(dialogText()).toContain('重载')
    await clickDialogButton('切换并重载')
    expect(lastInvokeArgs('switch_book')['id']).toBe('b2')
    expect(messageCalls().some((m) => m.method === 'success' && m.text.includes('正在切换到'))).toBe(true)
  })

  it('取消确认则留在当前账本（不调 switch_book，清单活动本不变）', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await bodySelector('.book-row:nth-child(2)')!.trigger('click')
    await flushPromises()
    await clickDialogButton('取消')
    expect(callsOf('switch_book')).toBe(0)
    expect(panelRows()[0]).toContain('当前')
  })

  it('点击当前活动本行不触发切换确认', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await bodySelector('.book-row:nth-child(1)')!.trigger('click')
    await flushPromises()
    expect(dialogText()).toBe('')
    expect(callsOf('switch_book')).toBe(0)
  })
})

describe('账本入口（issue #834）：弹层内新建 / 改名 / 移除', () => {
  it('新建：命名登记后 create_book 落位，清单即时更新（新账本出现在弹层）', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await findBodyButton('新建账本')!.trigger('click')
    await flushPromises()
    // 弹窗打开：空名时主操作禁用（后端 name-required 同口径的前端兜底）
    expect(visibleModalText()).toContain('新建账本')
    const confirm = findBodyButton('创建', { exact: true })!
    expect((confirm.element as HTMLButtonElement).disabled).toBe(true)
    const input = bodySelector('[data-testid="book-name-input"] input')!
    await input.setValue('家庭账本')
    expect((confirm.element as HTMLButtonElement).disabled).toBe(false)
    await confirm.trigger('click')
    await flushPromises()
    expect(lastInvokeArgs('create_book')['name']).toBe('家庭账本')
    expect(panelRows().some((r) => r.includes('家庭账本'))).toBe(true)
  })

  it('改名：弹窗回填原名，保存后 rename_book 落位、清单即时更新', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await bodySelector('[data-testid="book-rename-b2"]')!.trigger('click')
    await flushPromises()
    expect(visibleModalText()).toContain('重命名账本')
    const input = bodySelector('[data-testid="book-name-input"] input')!
    expect((input.element as HTMLInputElement).value).toBe('副业账本')
    await input.setValue('副业 2026')
    await findBodyButton('保存', { exact: true })!.trigger('click')
    await flushPromises()
    expect(lastInvokeArgs('rename_book')).toMatchObject({ id: 'b2', name: '副业 2026' })
    expect(panelRows().some((r) => r.includes('副业 2026'))).toBe(true)
  })

  it('当前活动本同样可改名：改名后入口按钮与清单同步更新', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    expect(wrapper.find('[data-testid="book-entry"]').text()).toContain('默认账本')
    await openPanel(wrapper)
    await bodySelector('[data-testid="book-rename-b1"]')!.trigger('click')
    await flushPromises()
    const input = bodySelector('[data-testid="book-name-input"] input')!
    expect((input.element as HTMLInputElement).value).toBe('默认账本')
    await input.setValue('个人账本')
    await findBodyButton('保存', { exact: true })!.trigger('click')
    await flushPromises()
    expect(lastInvokeArgs('rename_book')).toMatchObject({ id: 'b1', name: '个人账本' })
    expect(panelRows().some((r) => r.includes('个人账本'))).toBe(true)
    expect(wrapper.find('[data-testid="book-entry"]').text()).toContain('个人账本')
  })

  it('移除：行级气泡确认后 remove_book 落位（仅摘登记），清单即时更新', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    await bodySelector('[data-testid="book-remove-b2"]')!.trigger('click')
    await flushPromises()
    // 气泡确认文案说明兜底（可重新登记找回，文件保留）
    expect(document.body.textContent).toContain('重新登记')
    await findBodyButton('移除', { exact: true })!.trigger('click')
    await flushPromises()
    expect(lastInvokeArgs('remove_book')['id']).toBe('b2')
    expect(panelRows()).toHaveLength(1)
    expect(panelRows()[0]).toContain('默认账本')
  })

  it('活动本不可移除：活动行无移除入口', async () => {
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    expect(bodySelector('[data-testid="book-remove-b1"]')).toBeUndefined()
    expect(bodySelector('[data-testid="book-remove-b2"]')).toBeDefined()
  })
})

describe('账本入口（issue #834）：折叠态、不可变与回退警示', () => {
  it('侧栏折叠时入口以浮标图标形态可达，点击同样打开清单弹层', async () => {
    const wrapper = mountEntry({ collapsed: true })
    await flushPromises()
    expect(wrapper.find('[data-testid="book-entry"]').exists()).toBe(false)
    const float = wrapper.find('[data-testid="book-entry-float"]')
    expect(float.exists()).toBe(true)
    await float.trigger('click')
    await flushPromises()
    await nextTick()
    expect(panelRows()).toHaveLength(2)
    expect(hasOpenOverlay()).toBe(true)
  })

  it('登记不可变（mutable=false）时变更入口全部禁用，切换确认同样不触发', async () => {
    registry = { ...registry, mutable: false }
    const wrapper = mountEntry()
    await flushPromises()
    await openPanel(wrapper)
    const create = findBodyButton('新建账本')!
    expect((create.element as HTMLButtonElement).disabled).toBe(true)
    expect((bodySelector('[data-testid="book-rename-b2"]')!.element as HTMLButtonElement).disabled).toBe(true)
    await bodySelector('.book-row:nth-child(2)')!.trigger('click')
    await flushPromises()
    expect(dialogText()).toBe('')
    expect(callsOf('switch_book')).toBe(0)
  })

  it('注册表损坏回退：弹层显著警示（fallback_reason 随行），入口不显示账本名', async () => {
    registry = {
      books: [],
      active_id: null,
      mutable: false,
      fallback_reason: '账本注册表已损坏，已回退默认账本',
    }
    const wrapper = mountEntry()
    await flushPromises()
    expect(wrapper.find('[data-testid="book-entry"]').text()).not.toContain('默认账本')
    await openPanel(wrapper)
    const alert = document.body.querySelector('.book-panel-alert')
    expect(alert?.textContent).toContain('账本注册表异常')
    expect(alert?.textContent).toContain('账本注册表已损坏，已回退默认账本')
    // 清单不可信（空）且变更入口禁用
    const create = findBodyButton('新建账本')
    expect(create).toBeDefined()
    expect((create!.element as HTMLButtonElement).disabled).toBe(true)
  })
})
