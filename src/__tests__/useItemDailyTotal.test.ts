import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { useItemsStore } from '@/stores/items'
import { useItemDailyTotal } from '@/composables/useItemDailyTotal'
import { registerToastSink } from '@/composables/useLoadable'
import {
  invokeHandler,
  makeFakeSink,
  makeItemDailyTotal,
  resetToastSink,
} from './factories'


const mockTotal = makeItemDailyTotal()

/** 默认 invoke mock：物品列表（store self-init）+ 日成本合计 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_items: [],
        item_daily_total: mockTotal,
      },
      extra,
    ),
  )
}

/** 宿主组件：模拟 DashboardView 在 setup 内使用 composable（首跑与 version watch 时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: useItemDailyTotal() }
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

describe('useItemDailyTotal 物品日成本数据层（issue #122）', () => {
  it('挂载即首跑：拉取 item_daily_total 并装配出合计', async () => {
    const wrapper = mount(Host)
    await flushPromises()
    const { total, loading, error } = wrapper.vm.shell
    expect(loading.value).toBe(false)
    expect(error.value).toBeNull()
    expect(total.value).toEqual(mockTotal)
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'item_daily_total')).toBe(true)
  })

  it('命令报错（如缺汇率）时进入兜底状态：total 置空、error 带后端中文错误信息，不抛异常', async () => {
    baseInvoke({
      item_daily_total: () => Promise.reject(new Error('缺少 JPY→CNY 汇率，无法折算')),
    })
    const { total, loading, error, refresh } = useItemDailyTotal()
    await expect(refresh()).resolves.not.toThrow()
    expect(loading.value).toBe(false)
    expect(total.value).toBeNull()
    expect(error.value).toBe('缺少 JPY→CNY 汇率，无法折算')
  })

  it('非 Error 抛出值（如 Tauri 字符串错误）也能兜底为文案', async () => {
    baseInvoke({ item_daily_total: () => Promise.reject('缺汇率') })
    const { error, refresh } = useItemDailyTotal()
    await refresh()
    expect(error.value).toBe('缺汇率')
  })

  it('成功后再次报错：total 清空并切换到错误态；再次成功则恢复', async () => {
    const { total, error, refresh } = useItemDailyTotal()
    await refresh()
    expect(total.value).not.toBeNull()

    baseInvoke({
      item_daily_total: () => Promise.reject(new Error('缺少 HKD→CNY 汇率')),
    })
    await refresh()
    expect(total.value).toBeNull()
    expect(error.value).toBe('缺少 HKD→CNY 汇率')

    baseInvoke()
    await refresh()
    expect(total.value).toEqual(mockTotal)
    expect(error.value).toBeNull()
  })

  it('失败时弹默认 toast（归一文案），成功不弹——error 状态与 toast 双通道共存', async () => {
    baseInvoke({ item_daily_total: () => Promise.reject(new Error('缺少 USD→CNY 汇率')) })
    const { error, refresh } = useItemDailyTotal()
    // 先让 store self-init 的 version 首跳与跟随重拉落定（此时 sink 仍为 no-op），再开始计数
    await flushPromises()

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

  it('物品写入失效（store version 变化）后自动重拉合计', async () => {
    let total = { native_currency: 'CNY', per_day_cents: 10000, item_count: 1 }
    baseInvoke({ item_daily_total: () => Promise.resolve(total) })
    const wrapper = mount(Host)
    await flushPromises()
    expect(wrapper.vm.shell.total.value).toEqual({
      native_currency: 'CNY',
      per_day_cents: 10000,
      item_count: 1,
    })

    // 物品写入 → 物品 store 重拉（模拟 ledger:changed 路径）→ version 自增 → 合计跟随重拉
    total = { native_currency: 'CNY', per_day_cents: 30000, item_count: 2 }
    await useItemsStore().refresh()
    await flushPromises()
    expect(wrapper.vm.shell.total.value).toEqual({
      native_currency: 'CNY',
      per_day_cents: 30000,
      item_count: 2,
    })
  })
})
