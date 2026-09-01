import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useFinancialFreedom } from '@/composables/useFinancialFreedom'
import { registerToastSink } from '@/composables/useLoadable'
import { invokeHandler, makeFakeSink, makeFinancialFreedom, resetToastSink } from './factories'

const mockInvoke = vi.mocked(invoke)

const mockFreedom = makeFinancialFreedom()

/** 默认 invoke mock：财务自由度聚合 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        financial_freedom: mockFreedom,
      },
      extra,
    ),
  )
}

/** 宿主组件：模拟 DashboardView 在 setup 内使用 composable（首跑时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: useFinancialFreedom() }
  },
  template: '<div />',
})

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
})

describe('useFinancialFreedom 财务自由度数据层（issue #344）', () => {
  it('挂载即首跑：拉取 financial_freedom 并装配出总览', async () => {
    const wrapper = mount(Host)
    await flushPromises()
    const { data, loading, error } = wrapper.vm.shell
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
    expect(data.value).toEqual(mockFreedom)
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'financial_freedom')).toBe(true)
  })

  it('命令报错（如缺汇率）时进入兜底状态：data 置空、error 带后端中文错误信息，不抛异常', async () => {
    baseInvoke({
      financial_freedom: () => Promise.reject(new Error('缺少 JPY→CNY 汇率，无法折算')),
    })
    const { data, loading, error, refresh } = useFinancialFreedom()
    await expect(refresh()).resolves.not.toThrow()
    expect(loading.value).toBe(false)
    expect(data.value).toBeNull()
    expect(error.value).toBe('缺少 JPY→CNY 汇率，无法折算')
  })

  it('非 Error 抛出值（如 Tauri 字符串错误）也能兜底为文案', async () => {
    baseInvoke({ financial_freedom: () => Promise.reject('缺汇率') })
    const { error, refresh } = useFinancialFreedom()
    await refresh()
    expect(error.value).toBe('缺汇率')
  })

  it('成功后再次报错：data 清空并切换到错误态；再次成功则恢复（重试即 refresh）', async () => {
    const { data, error, refresh } = useFinancialFreedom()
    await refresh()
    expect(data.value).not.toBeNull()

    baseInvoke({
      financial_freedom: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
    })
    await refresh()
    expect(data.value).toBeNull()
    expect(error.value).toBe('缺少 HKD→CNY 汇率')

    baseInvoke()
    await refresh()
    expect(data.value).toEqual(mockFreedom)
    expect(error.value).toBeNull()
  })

  it('失败时弹默认 toast（归一文案），成功不弹——error 状态与 toast 双通道共存', async () => {
    baseInvoke({ financial_freedom: () => Promise.reject(new Error('缺少 USD→CNY 汇率')) })
    const { error, refresh } = useFinancialFreedom()
    const sink = makeFakeSink()
    registerToastSink(sink)
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('缺少 USD→CNY 汇率')
    expect(error.value).toBe('缺少 USD→CNY 汇率')

    baseInvoke()
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
  })
})
