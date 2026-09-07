import { describe, it, expect, beforeEach } from 'vitest'
import { wireInvokeSeam } from '../helpers/invoke-mock'
import { nextTick } from 'vue'
import { flushPromises } from '@vue/test-utils'
import { mountFlushed, mountWithDialog } from '../helpers/mount'
import AddInstrumentModal from '@/components/investments/AddInstrumentModal.vue'
import ManualPriceModal from '@/components/investments/ManualPriceModal.vue'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import { makeInstrument } from '../factories'

// 投资弹窗族排版统一（issue #638，spec #630）：五个弹窗的卡片外观收敛为
// AppModal cardSize 单一声明——添加投资标的（#697 收编原自建标的创建与
// 添加基金两入口）、手动报价、全量同步确认、同步进度均归 md；
// 显式 style 宽度由 cardSize 承担，无边框由 AppModal 默认承担（调用点不再
// 显式 :bordered="false"）。断言只看组件可观察输出（卡片宽度样式与边框类），
// 不深究 naive-ui 内部实现；开合编排与快捷键抑制（ADR-0035/ADR-0072）不在
// 本测试断言面内，由既有 InstrumentBrowser/CreateInstrumentModal/
// ManualPriceModal 测试保障。
// 布线走唯一接缝（issue #748）：标的清单契约进 defaults 表、同步动作为
// overrides；参考字典五命令由规范夹具兑底；store 层预热 opt-in 开启
// （币种选项为 self-init，弹窗内下拉依赖就绪后的渲染，先例：
// CreateInstrumentModal.test.ts）。清理四件套由全局壳层承担。

const mockInstruments = [
  makeInstrument({ id: 'inst-1' }),
  makeInstrument({ id: 'inst-2', symbol: '000001', name: '平安银行', market: 'sz' }),
]

// NModal 内容 teleport 到 document.body：弹窗残留的清理与卸载由全局壳层
// 每测自动执行（issue #748）。

beforeEach(async () => {
  const seam = wireInvokeSeam({
    defaults: {
      list_instruments: { items: mockInstruments, total: mockInstruments.length },
    },
    overrides: {
      sync_instruments: () => Promise.resolve(undefined),
    },
    refreshReferenceStores: true,
  })
  await seam.ready
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
  return mountWithDialog(InstrumentBrowser)
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
  it('添加投资标的弹窗（独立挂载）归 md 档且默认无边框', async () => {
    await mountFlushed(AddInstrumentModal, { props: { show: true } })
    expectCardSizeMd(modalCard())
  })

  it('手动报价弹窗归 md 档且默认无边框', async () => {
    await mountFlushed(ManualPriceModal, {
      props: { show: true, instrument: makeInstrument({ id: 'inst-quote-1' }) },
    })
    expectCardSizeMd(modalCard())
  })

  it('添加投资标的弹窗（经标的页工具栏入口打开）归 md 档且默认无边框', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    await clickToolbarButton(wrapper, 'add-instrument')
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
