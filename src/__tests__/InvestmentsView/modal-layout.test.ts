import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import CreateInstrumentModal from '@/components/investments/CreateInstrumentModal.vue'
import ManualPriceModal from '@/components/investments/ManualPriceModal.vue'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import { makeInstrument } from '../factories'
import type { Currency } from '@/types'

// 投资弹窗族排版统一（issue #638，spec #630）：五个弹窗的卡片外观收敛为
// AppModal cardSize 单一声明——自建标的创建、手动报价、添加基金、全量同步
// 确认、同步进度均归 md（480；前三个 440 归档、后两个 480 原档归位）；
// 显式 style 宽度由 cardSize 承担，无边框由 AppModal 默认承担（调用点不再
// 显式 :bordered="false"）。断言只看组件可观察输出（卡片宽度样式与边框类），
// 不深究 naive-ui 内部实现；开合编排与快捷键抑制（ADR-0035/ADR-0072）不在
// 本测试断言面内，由既有 InstrumentBrowser/CreateInstrumentModal/
// ManualPriceModal 测试保障。

const mockListen = vi.mocked(listen)
const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockInstruments = [
  makeInstrument({ id: 'inst-1' }),
  makeInstrument({ id: 'inst-2', symbol: '000001', name: '平安银行', market: 'sz' }),
]

// NModal 内容 teleport 到 document.body：每测后卸载并清空 body，
// 避免前一用例的弹窗残留污染查询（先例：InstrumentBrowser.test.ts）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockImplementation(() => Promise.resolve(() => {}))
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_instruments')
      return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
    if (cmd === 'sync_instruments') return Promise.resolve(undefined)
    if (cmd === 'list_insurers') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  // 参考数据（币种选项）为 self-init，提前预热（先例：CreateInstrumentModal.test.ts）
  await useReferenceStore().refresh()
})

/** 卡片根元素（preset="card" 下卡片即 NCard 根；单测内同时只开一个弹窗）。 */
function modalCard(): HTMLElement {
  const card = document.body.querySelector<HTMLElement>('.n-card')
  expect(card, '弹窗卡片（NCard）应存在').not.toBeNull()
  return card!
}

/** 断言弹窗卡片：宽度归 md 档（480）+ 默认无边框（AppModal 默认，调用点不再显式声明）。 */
function expectCardSizeMd(card: HTMLElement) {
  expect(card.style.width).toBe('480px')
  expect(card.classList.contains('n-card--bordered')).toBe(false)
}

/** 组件顶层调用 useAppDialog（删除二次确认），与 App.vue 同构需 NDialogProvider 包裹（先例：InstrumentBrowser.test.ts）。 */
function mountBrowser() {
  return mount(NDialogProvider, {
    slots: { default: () => h(InstrumentBrowser) },
  })
}

async function clickToolbarButton(wrapper: ReturnType<typeof mountBrowser>, testid: string) {
  await wrapper.find(`[data-testid="${testid}"]`).trigger('click')
  await nextTick()
  await flushPromises()
}

/** 弹窗内容 teleport 到 document.body：弹窗内元素（确认键）用原生事件触发（先例：InstrumentBrowser.test.ts）。 */
async function clickBody(testid: string) {
  const el = document.body.querySelector(`[data-testid="${testid}"]`)
  if (!el) throw new Error(`body 中未找到: ${testid}`)
  el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
  await nextTick()
  await flushPromises()
}

describe('投资弹窗族排版统一（issue #638）', () => {
  it('自建标的创建弹窗归 md 档且默认无边框', async () => {
    mount(CreateInstrumentModal, { props: { show: true } })
    await flushPromises()
    expectCardSizeMd(modalCard())
  })

  it('手动报价弹窗归 md 档且默认无边框', async () => {
    mount(ManualPriceModal, {
      props: { show: true, instrument: makeInstrument({ id: 'inst-quote-1' }) },
    })
    await flushPromises()
    expectCardSizeMd(modalCard())
  })

  it('添加基金弹窗归 md 档且默认无边框', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    await clickToolbarButton(wrapper, 'add-fund')
    expectCardSizeMd(modalCard())
  })

  it('全量同步确认弹窗归 md 档且默认无边框', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    await clickToolbarButton(wrapper, 'full-sync')
    expectCardSizeMd(modalCard())
  })

  it('同步进度弹窗归 md 档且默认无边框', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    await clickToolbarButton(wrapper, 'full-sync')
    await clickBody('confirm-full-sync')
    // 确认后同步发起、进度框打开（中断按钮是进度框的标志元素）
    expect(document.body.querySelector('[data-testid="cancel-full-sync"]')).not.toBeNull()
    expectCardSizeMd(modalCard())
  })
})
