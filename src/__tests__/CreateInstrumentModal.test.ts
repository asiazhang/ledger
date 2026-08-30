import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { NSelect } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import CreateInstrumentModal from '@/components/investments/CreateInstrumentModal.vue'
import type { Currency } from '@/types'

// NModal 内容 teleport 到 document.body，须在每个测试后卸载 wrapper 并清空 body，
// 否则上一个测试遗留的弹窗 DOM 会污染下一个测试（先例：InstrumentBrowser.test.ts）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

function baseInvoke() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

async function mountModal(onCreated?: (msg: string) => void) {
  const wrapper = mount(CreateInstrumentModal, {
    props: { show: true, ...(onCreated ? { onCreated } : {}) },
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

function submitButton(): HTMLButtonElement {
  return bodyQuery('[data-testid="submit-create-instrument"]') as HTMLButtonElement
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  await useReferenceStore().refresh()
})

describe('CreateInstrumentModal 新建标的弹窗（issue #290 / ADR-0036）', () => {
  it('类型下拉恰为白名单三选（债券/ETF/其他）：无股票、无基金', async () => {
    const wrapper = await mountModal()
    // 模板中第一个 NSelect 即类型选择（币种选择在后）
    const typeSelect = wrapper.findComponent(NSelect)
    const options = typeSelect.props('options') as { label: string; value: string }[]
    expect(options.map((o) => o.value)).toEqual(['bond', 'etf', 'other'])
    expect(options.map((o) => o.label)).toEqual(['债券', 'ETF', '其他'])
  })

  it('币种默认人民币；市场不设字段（固定未知）', async () => {
    const wrapper = await mountModal()
    const selects = wrapper.findAllComponents(NSelect)
    expect(selects[1].props('value')).toBe('CNY')
    expect((selects[1].props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'CNY',
      'USD',
    ])
    // 弹窗文案说明市场固定未知，表单无市场输入
    expect(bodyQuery('[data-testid="create-instrument-market"]')).toBeNull()
    expect(document.body.textContent).toContain('市场为未知')
  })

  it('名称/代码/类型未齐时提交禁用（名称必填的表单侧校验）', async () => {
    await mountModal()
    expect(submitButton().disabled).toBe(true)
    await setInput('create-instrument-symbol', '稳稳地幸福')
    await setInput('create-instrument-name', '稳稳地幸福')
    // 缺类型仍禁用
    expect(submitButton().disabled).toBe(true)
  })

  it('纯空白名称视为未填：提交保持禁用、不发请求', async () => {
    const wrapper = await mountModal()
    const typeSelect = wrapper.findComponent(NSelect)
    typeSelect.vm.$emit('update:value', 'other')
    await setInput('create-instrument-symbol', 'HW-VR')
    await setInput('create-instrument-name', '   ')
    expect(submitButton().disabled).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('create_instrument', expect.anything())
  })

  it('齐全后提交：调用 create_instrument（市场传 null 走后端缺省 unknown），emit created 并关弹窗', async () => {
    const created: string[] = []
    const wrapper = await mountModal((msg) => created.push(msg))
    const typeSelect = wrapper.findComponent(NSelect)
    typeSelect.vm.$emit('update:value', 'other')
    await setInput('create-instrument-symbol', ' 稳稳地幸福 ')
    await setInput('create-instrument-name', ' 稳稳地幸福 ')
    expect(submitButton().disabled).toBe(false)
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_instrument') return Promise.resolve('inst-new')
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await bodyQuery('[data-testid="submit-create-instrument"]')!.dispatchEvent(
      new MouseEvent('click', { bubbles: true }),
    )
    await flushPromises()
    // 代码/名称 trim 后入参；市场 null → 后端缺省 unknown；币种默认 CNY
    expect(mockInvoke).toHaveBeenCalledWith('create_instrument', {
      input: {
        symbol: '稳稳地幸福',
        type: 'other',
        name: '稳稳地幸福',
        currency_code: 'CNY',
        market: null,
      },
    })
    expect(created).toEqual(['已创建标的：稳稳地幸福（稳稳地幸福）'])
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('后端拒绝（白名单/名称校验）：弹窗内展示中文报错，弹窗不关、无 created 回执', async () => {
    const created: string[] = []
    const wrapper = await mountModal((msg) => created.push(msg))
    const typeSelect = wrapper.findComponent(NSelect)
    typeSelect.vm.$emit('update:value', 'stock')
    await setInput('create-instrument-symbol', '600519')
    await setInput('create-instrument-name', '贵州茅台')
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_instrument')
        return Promise.reject({
          kind: 'Invalid',
          message: '股票类标的不支持手动创建：股票字典由「全量同步」从东方财富维护',
        })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await bodyQuery('[data-testid="submit-create-instrument"]')!.dispatchEvent(
      new MouseEvent('click', { bubbles: true }),
    )
    await flushPromises()
    const err = bodyQuery('[data-testid="create-instrument-error"]')!
    expect(err.textContent).toContain('股票类标的不支持手动创建')
    expect(err.textContent).not.toContain('[object Object]')
    // 弹窗保持打开（未发 update:show=false），无成功回执
    expect(wrapper.emitted('update:show') ?? []).not.toContainEqual([false])
    expect(created).toEqual([])
  })
})
