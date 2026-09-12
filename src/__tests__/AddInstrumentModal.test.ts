import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { NSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import AddInstrumentModal from '@/components/investments/AddInstrumentModal.vue'
import type { Currency } from '@ledger/types'

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

/** 弹窗布线：list_currencies 参考命令本场景需自定义值（CNY+USD）。 */
const BASE_OVERRIDES = { list_currencies: mockCurrencies }

/** 命中回显的股票侧结果（桩：东财回填 + 类型识别 etf）。 */
const stockHit = {
  instrument_id: 'inst-1',
  symbol: '159915',
  name: '创业板ETF',
  type: 'etf',
  market: 'sz',
  currency_code: 'CNY',
  price_cents: 23450,
  price_date: '2026-09-04',
  price_written: true,
}

/** 命中回显的基金侧结果（桩：按代码即拉，语义不变）。 */
const fundHit = {
  instrument_id: 'inst-fund',
  symbol: '000001',
  name: '华夏成长混合',
  fund_class: '混合型-灵活',
  nav_cents: 13180,
  nav_date: '2026-08-28',
  price_written: true,
}

/** 查询未命中（查无此码）的码化错误（股票通道）。 */
function stockNotFound(code: string) {
  return Promise.reject({
    kind: 'Invalid',
    code: 'sync.stock-not-found',
    message: `查无股票代码 ${code}，请核对后重试`,
    params: [code],
  })
}

async function mountModal(onAdded?: (msg: string) => void) {
  const wrapper = mount(AddInstrumentModal, {
    props: { show: true, ...(onAdded ? { onAdded } : {}) },
  })
  await flushPromises()
  return wrapper
}

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

/** 弹窗内 NInput 的受控是内部 input 元素：原生赋值 + 冒泡 input 事件驱动 v-model */
async function setInput(testid: string, value: string) {
  const input = bodyQuery(`[data-testid="${testid}"]`)!.querySelector('input')!
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await nextTick()
  await flushPromises()
}

async function clickBody(testid: string) {
  bodyQuery(`[data-testid="${testid}"]`)!.dispatchEvent(
    new MouseEvent('click', { bubbles: true, cancelable: true }),
  )
  await nextTick()
  await flushPromises()
}

type MountedModal = Awaited<ReturnType<typeof mountModal>>

/** 选择市场通道（模板中第一个 NSelect 即市场下拉） */
async function selectMarket(wrapper: MountedModal, value: string) {
  wrapper.findAllComponents(NSelect)[0].vm.$emit('update:value', value)
  await nextTick()
  await flushPromises()
}

function submitButton(): HTMLButtonElement {
  return bodyQuery('[data-testid="submit-add-instrument"]') as HTMLButtonElement
}

beforeEach(async () => {
  wireInvokeSeam({ overrides: BASE_OVERRIDES })
  await useReferenceStore().refresh()
})

describe('AddInstrumentModal 添加投资标的弹窗（issue #826 / spec #690）', () => {
  it('市场必选：未选市场时提交禁用；选市场+输代码后启用', async () => {
    const wrapper = await mountModal()
    expect(submitButton().disabled).toBe(true)
    await setInput('add-instrument-code', '600519')
    // 已输代码但未选市场：仍禁用（市场必选校验）
    expect(submitButton().disabled).toBe(true)
    await selectMarket(wrapper, 'sh')
    expect(submitButton().disabled).toBe(false)
  })

  it('沪市通道提交：调用 add_instrument_by_code（market=sh），成功回执回显识别类型与现价并关弹窗', async () => {
    const added: string[] = []
    const wrapper = await mountModal((msg) => added.push(msg))
    await selectMarket(wrapper, 'sh')
    await setInput('add-instrument-code', '600519')
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        add_instrument_by_code: () => Promise.resolve(stockHit),
      },
    })
    await clickBody('submit-add-instrument')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('add_instrument_by_code', {
      market: 'sh',
      code: '600519',
    })
    // 识别回显：类型标签 + 东财名称 + 现价（万分之一元 → 元展示）
    expect(added[0]).toContain('创业板ETF')
    expect(added[0]).toContain('ETF')
    expect(added).toHaveLength(1)
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('场外基金通道提交：走既有 add_fund_by_code（语义不变），成功回执回显基金形态', async () => {
    const added: string[] = []
    const wrapper = await mountModal((msg) => added.push(msg))
    await selectMarket(wrapper, 'fund')
    await setInput('add-instrument-code', '000001')
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        add_fund_by_code: () => Promise.resolve(fundHit),
      },
    })
    await clickBody('submit-add-instrument')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('add_fund_by_code', { code: '000001' })
    expect(added[0]).toContain('华夏成长混合')
    expect(added[0]).toContain('混合型-灵活')
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('基金通道非 6 位代码：前端提前拦截，提交禁用不发请求', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'fund')
    await setInput('add-instrument-code', '12345')
    expect(submitButton().disabled).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('add_fund_by_code', expect.anything())
  })

  it('自定义标的通道：选中即展开建档表单（零网络请求），按钮切为「创建」', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'custom')
    // 建档表单直接展开：名称/类型/币种可见，无需先查询
    expect(bodyQuery('[data-testid="add-instrument-name"]')).not.toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-type"]')).not.toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-currency"]')).not.toBeNull()
    // 主按钮切为创建语义；代码/名称/类型未齐时禁用
    expect(submitButton().textContent).toContain('创建')
    await setInput('add-instrument-code', 'HW-VR')
    expect(submitButton().disabled).toBe(true)
    await setInput('add-instrument-name', '华为虚拟股')
    expect(submitButton().disabled).toBe(true)
    // 零网络请求：表单展开全程未发起任何查询/创建命令
    expect(mockInvoke).not.toHaveBeenCalledWith('add_instrument_by_code', expect.anything())
    expect(mockInvoke).not.toHaveBeenCalledWith('add_fund_by_code', expect.anything())
    expect(mockInvoke).not.toHaveBeenCalledWith('create_instrument', expect.anything())
  })

  it('自定义标的通道类型白名单恰三选（债券/ETF/其他）：无股票、无基金', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'custom')
    // 自定义通道展开后：[0]=市场、[1]=类型、[2]=币种
    const typeSelect = wrapper.findAllComponents(NSelect)[1]
    const options = typeSelect.props('options') as { label: string; value: string }[]
    expect(options.map((o) => o.value)).toEqual(['bond', 'etf', 'other'])
    expect(options.map((o) => o.label)).toEqual(['债券', 'ETF', '其他'])
  })

  it('自定义标的通道提交：代码自由文本即可（无 6 位约束），create_instrument 市场 null 落 unknown', async () => {
    const added: string[] = []
    const wrapper = await mountModal((msg) => added.push(msg))
    await selectMarket(wrapper, 'custom')
    await setInput('add-instrument-code', 'HW-VR')
    await setInput('add-instrument-name', '华为虚拟股')
    wrapper.findAllComponents(NSelect)[1].vm.$emit('update:value', 'other')
    await nextTick()
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        create_instrument: Promise.resolve('inst-new'),
      },
    })
    expect(submitButton().disabled).toBe(false)
    await clickBody('submit-add-instrument')
    await flushPromises()
    // 市场恒未知（不透传——自定义标的无真实市场，ADR-0081 口径）；币种默认 CNY
    expect(mockInvoke).toHaveBeenCalledWith('create_instrument', {
      input: {
        symbol: 'HW-VR',
        type: 'other',
        name: '华为虚拟股',
        currency_code: 'CNY',
        market: null,
      },
    })
    expect(added[0]).toContain('华为虚拟股')
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('股票通道未命中：只报错+引导切换自定义标的通道，不展开建档表单、弹窗不关', async () => {
    const added: string[] = []
    const wrapper = await mountModal((msg) => added.push(msg))
    await selectMarket(wrapper, 'us')
    await setInput('add-instrument-code', 'NOPE')
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        add_instrument_by_code: () => stockNotFound('NOPE'),
      },
    })
    await clickBody('submit-add-instrument')
    await flushPromises()
    // 未命中显式报错 + 引导文案；建档表单不展开（全对话框仅一份表单，属自定义通道）
    expect(bodyQuery('[data-testid="add-instrument-error"]')!.textContent).toContain('查无股票代码')
    expect(bodyQuery('[data-testid="add-instrument-not-found-hint"]')!.textContent).toContain(
      '自定义标的',
    )
    expect(bodyQuery('[data-testid="add-instrument-name"]')).toBeNull()
    expect(wrapper.emitted('update:show') ?? []).not.toContainEqual([false])
    expect(added).toEqual([])
  })

  it('行情临时不可达：报错但无引导文案（临时故障非数据源未覆盖）', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'sh')
    await setInput('add-instrument-code', '600519')
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        add_instrument_by_code: () =>
          Promise.reject({ kind: 'Io', message: '东财临时不可达' }),
      },
    })
    await clickBody('submit-add-instrument')
    await flushPromises()
    expect(bodyQuery('[data-testid="add-instrument-error"]')!.textContent).toContain('东财临时不可达')
    expect(bodyQuery('[data-testid="add-instrument-not-found-hint"]')).toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-name"]')).toBeNull()
  })

  it('基金通道未命中：报错但无引导（fund 唯一创建入口仍为按代码即拉）', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'fund')
    await setInput('add-instrument-code', '999999')
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        add_fund_by_code: () =>
          Promise.reject({
            kind: 'Invalid',
            code: 'sync.fund-not-found',
            message: '查无基金代码 999999，请核对后重试',
            params: ['999999'],
          }),
      },
    })
    await clickBody('submit-add-instrument')
    await flushPromises()
    expect(bodyQuery('[data-testid="add-instrument-error"]')!.textContent).toContain('查无基金代码')
    expect(bodyQuery('[data-testid="add-instrument-not-found-hint"]')).toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-name"]')).toBeNull()
  })

  it('通道切换：自定义标的 ↔ 股票通道，建档表单随通道收放、报错清空', async () => {
    const wrapper = await mountModal()
    await selectMarket(wrapper, 'custom')
    expect(bodyQuery('[data-testid="add-instrument-name"]')).not.toBeNull()
    // 切回股票通道：建档表单收起（表单只属于自定义通道）
    await selectMarket(wrapper, 'sh')
    expect(bodyQuery('[data-testid="add-instrument-name"]')).toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-custom-hint"]')).toBeNull()
    // 再切回：表单再现
    await selectMarket(wrapper, 'custom')
    expect(bodyQuery('[data-testid="add-instrument-name"]')).not.toBeNull()
    expect(bodyQuery('[data-testid="add-instrument-custom-hint"]')).not.toBeNull()
  })
})
