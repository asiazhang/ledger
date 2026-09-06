import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount, type VueWrapper } from '@vue/test-utils'
import { NInputNumber } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import ManualPriceModal from '@/components/investments/ManualPriceModal.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { makeInstrument } from './factories'
import { todayStr } from '@/utils/date'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Instrument } from '@/types'

// NModal 内容 teleport 到 document.body，须在每个测试后卸载 wrapper 并清空 body，
// 否则上一个测试遗留的弹窗 DOM 会污染下一个测试（先例：CreateInstrumentModal.test.ts）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})


/** 基础派发：beforeEach 安装；中途重桩处理完自己的领域命令后委托回它 */
let base: ReturnType<typeof stubReferenceInvoke>

function baseInvoke() {
  base = stubReferenceInvoke({
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
  })
}

const instrument: Instrument = makeInstrument({
  id: 'inst-quote-1',
  symbol: '稳稳地幸福',
  type: 'other',
  name: '稳稳地幸福',
  market: 'unknown',
  source: 'manual',
})

async function mountModal(onQuoted?: (msg: string) => void) {
  const wrapper = mount(ManualPriceModal, {
    props: { show: true, instrument, ...(onQuoted ? { onQuoted } : {}) },
  })
  await flushPromises()
  return wrapper
}

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

function submitButton(): HTMLButtonElement {
  return bodyQuery('[data-testid="submit-manual-quote"]') as HTMLButtonElement
}

/** 弹窗内价格输入经 NInputNumber 组件 emit 驱动（受控值在组件态，非裸 DOM） */
async function setPrice(wrapper: VueWrapper, value: number | null) {
  wrapper.findComponent(NInputNumber).vm.$emit('update:value', value)
  await flushPromises()
}

/** 点击 body 中的提交按钮（弹窗内容 teleport 到 body） */
async function clickSubmit() {
  bodyQuery('[data-testid="submit-manual-quote"]')!.dispatchEvent(
    new MouseEvent('click', { bubbles: true, cancelable: true }),
  )
  await flushPromises()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  await useReferenceStore().refresh()
})

describe('ManualPriceModal 手动报价弹窗（issue #291 / ADR-0036）', () => {
  it('打开时日期默认今天、价格为空；标题带标的代码；价格未填时提交禁用、不发请求', async () => {
    const _wrapper = await mountModal()
    expect(document.body.textContent).toContain('录价 — 稳稳地幸福')
    // 日期默认今天（录价当日即生效为主形态）
    const dateInput = bodyQuery('[data-testid="manual-quote-date"]')!.querySelector('input')!
    expect((dateInput as HTMLInputElement).value).toBe(todayStr())
    expect(submitButton().disabled).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('record_manual_price', expect.anything())
  })

  it('价格为 0 或负数时提交禁用（价格 > 0 校验，与后端同款）', async () => {
    const wrapper = await mountModal()
    await setPrice(wrapper, 0)
    expect(submitButton().disabled).toBe(true)
    await setPrice(wrapper, -1.5)
    expect(submitButton().disabled).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('record_manual_price', expect.anything())
  })

  it('提交：价格换算万分之一元、日期为今天 ISO；emit quoted（现价更新回执）并关弹窗', async () => {
    const quoted: string[] = []
    const wrapper = await mountModal((msg) => quoted.push(msg))
    await setPrice(wrapper, 1.318)
    expect(submitButton().disabled).toBe(false)
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'record_manual_price'
        ? Promise.resolve({ history_written: true, current_price_written: true })
        : base(cmd, args),
    )
    await clickSubmit()
    // 价格 1.318 元 → 13180 万分之一元（价格刻度 ADR-0038）；日期为今天 ISO
    expect(mockInvoke).toHaveBeenCalledWith('record_manual_price', {
      input: {
        instrument_id: 'inst-quote-1',
        date: todayStr(),
        price_cents: 13180,
      },
    })
    expect(quoted).toEqual(['已录价：稳稳地幸福 现价更新为 1.318'])
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('回填旧价（current_price_written=false）：回执提示只沉淀历史、现价保持不变', async () => {
    const quoted: string[] = []
    const wrapper = await mountModal((msg) => quoted.push(msg))
    await setPrice(wrapper, 0.9)
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'record_manual_price'
        ? Promise.resolve({ history_written: true, current_price_written: false })
        : base(cmd, args),
    )
    await clickSubmit()
    expect(quoted).toEqual(['已沉淀历史价格（早于最新价格点，稳稳地幸福 现价保持不变）'])
    expect(wrapper.emitted('update:show')).toContainEqual([false])
  })

  it('后端拒绝（如价格校验）：弹窗内展示中文报错，弹窗不关、无 quoted 回执', async () => {
    const quoted: string[] = []
    const wrapper = await mountModal((msg) => quoted.push(msg))
    await setPrice(wrapper, 1.318)
    mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
      cmd === 'record_manual_price'
        ? Promise.reject({ kind: 'Invalid', message: '价格必须大于 0' })
        : base(cmd, args),
    )
    await clickSubmit()
    const err = bodyQuery('[data-testid="manual-quote-error"]')!
    expect(err.textContent).toContain('价格必须大于 0')
    expect(err.textContent).not.toContain('[object Object]')
    expect(wrapper.emitted('update:show') ?? []).not.toContainEqual([false])
    expect(quoted).toEqual([])
  })

  it('日期经 AppDatePicker 封装接入（弹层注册表，ADR-0035）；不直接用裸 NDatePicker', async () => {
    const wrapper = await mountModal()
    expect(wrapper.findComponent(AppDatePicker).exists()).toBe(true)
  })
})
