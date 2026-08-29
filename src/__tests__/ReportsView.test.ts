import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NSelect } from 'naive-ui'
import ReportsView from '@/views/ReportsView.vue'
import { invokeHandler } from './factories'
import type { YearRange } from '@/types'

const mockInvoke = vi.mocked(invoke)

const currentYear = new Date().getFullYear()

/** 范围夹具：起点比当前年早 6 年、终点在未来年——平铺全集须覆盖滑动窗口之外的年份 */
const mockRange: YearRange = { min_year: currentYear - 6, max_year: currentYear + 1 }

/** 默认 invoke mock：参考数据（reference store self-init）+ 年份范围 + 三报表查询（空集即可） */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: [],
        list_accounts: [],
        list_categories: [],
        list_merchants: [],
        report_year_range: mockRange,
        monthly_summary: [],
        category_shares: [],
        merchant_shares: [],
      },
      extra,
    ),
  )
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

async function mountReports() {
  const wrapper = mount(ReportsView)
  await flushPromises()
  return wrapper
}

function yearOptionsOf(wrapper: ReturnType<typeof mount>) {
  return wrapper.findComponent(NSelect).props('options') as {
    label: string
    value: number
  }[]
}

describe('ReportsView 年份筛选（issue #267）', () => {
  it('挂载时拉取范围一次，选项为范围内全部年份升序平铺、纯数字 label', async () => {
    const wrapper = await mountReports()
    const expected = Array.from({ length: 8 }, (_, i) => currentYear - 6 + i)
    expect(yearOptionsOf(wrapper).map((o) => o.value)).toEqual(expected)
    expect(yearOptionsOf(wrapper).map((o) => o.label)).toEqual(expected.map(String))
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_year_range')
    expect(rangeCalls).toHaveLength(1)
  })

  it('±2 滑动窗口已删除：远早于当前年的年份一击直达', async () => {
    const wrapper = await mountReports()
    const values = yearOptionsOf(wrapper).map((o) => o.value)
    expect(values).toContain(currentYear - 6)
    expect(values).toContain(currentYear + 1)
    // 选项全集恰为范围内年份，不多不少
    expect(values).toHaveLength(8)
  })

  it('默认选中当前年（范围内天然包含，无需钳制）', async () => {
    const wrapper = await mountReports()
    expect(wrapper.findComponent(NSelect).props('value')).toBe(currentYear)
  })

  it('切换年份触发三个报表查询且年份参数正确（联动刷新），范围不重复拉取', async () => {
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    wrapper.findComponent(NSelect).vm.$emit('update:value', currentYear - 6)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', { year: currentYear - 6 })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', { year: currentYear - 6 })
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', { kind: 'expense', month: null })
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_year_range')
    expect(rangeCalls).toHaveLength(0)
  })
})
