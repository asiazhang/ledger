// 路由替身经 common.ts 的 vi.mock 注册，必须先于任何直连组件导入（导入顺序即 mock 生效面）
import {
  mountView, mountMobile, makeTxn, setTxnDb, openCreateDropdown, openCreateFab,
} from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NModal, NSelect } from 'naive-ui'
import { resetOverlays } from '@/composables/overlayRegistry'
import { useFeatureToggleStore } from '@/stores/feature-toggles'
import AppSelect from '@/components/AppSelect.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import {
  INVESTMENT_CREATE_KINDS,
  availableCreateKinds,
  isCreateKindAvailable,
} from '@ledger/utils/create-entry-kinds'

/**
 * 关闭投资后交易页投资新建入口消失（issue #1245 / ADR-0116 决策 4「入口侧」）：
 * 新建买入/卖出是向投资功能写入新数据的入口，随投资关闭一并消失（桌面下拉与移动
 * 「记一笔」悬浮按钮同源、一处生效两处，`b`/`s` 裸键同属入口侧）；既有 buy/sell
 * 交易与引用侧（列表展示、来源列、按 kind 筛选）一律照常，重开即恢复。
 */

const FULL_CREATE_LABELS = ['支出 a', '收入 i', '转账 z', '买入 b', '卖出 s', '借出', '借入']
const CLOSED_CREATE_LABELS = ['支出 a', '收入 i', '转账 z', '借出', '借入']
const FULL_FAB_LABELS = ['支出', '收入', '转账', '买入', '卖出']
const CLOSED_FAB_LABELS = ['支出', '收入', '转账']

function closeInvestments() {
  useFeatureToggleStore().setFeatureClosed('investments', true)
}

function reopenInvestments() {
  useFeatureToggleStore().setFeatureClosed('investments', false)
}

function pressKey(key: string) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
}

describe('新建入口类型闭集（issue #1245：投资关闭 → 买入/卖出退出全部新建入口）', () => {
  it('投资开启：可用类型为 CREATE_KINDS 全量五类', () => {
    expect(availableCreateKinds(false)).toEqual(['expense', 'income', 'transfer', 'buy', 'sell'])
  })

  it('投资关闭：只余支出/收入/转账，买入/卖出退出（位置语义由清单序保留）', () => {
    expect(availableCreateKinds(true)).toEqual(['expense', 'income', 'transfer'])
  })

  it('单类型判定与列表同源：仅投资类 kind 在关闭时不可用', () => {
    for (const kind of INVESTMENT_CREATE_KINDS) {
      expect(isCreateKindAvailable(kind, true)).toBe(false)
      expect(isCreateKindAvailable(kind, false)).toBe(true)
    }
    expect(isCreateKindAvailable('expense', true)).toBe(true)
    expect(isCreateKindAvailable('transfer', true)).toBe(true)
  })
})

describe('关闭投资：桌面记一笔下拉（issue #1245）', () => {
  it.each<[label: string, close: boolean, reopen: boolean, expected: string[]]>([
    ['默认全开：下拉含买入/卖出', false, false, FULL_CREATE_LABELS],
    ['关闭投资：下拉不含买入/卖出，其余类型与借贷变体照常', true, false, CLOSED_CREATE_LABELS],
    ['重开投资：下拉恢复买入/卖出（回原位置）', true, true, FULL_CREATE_LABELS],
  ])('%s', async (_label, close, reopen, expected) => {
    if (close) closeInvestments()
    const wrapper = await mountView()
    if (reopen) {
      reopenInvestments()
      await flushPromises()
    }
    expect(await openCreateDropdown(wrapper)).toEqual(expected)
  })
})

describe('关闭投资：移动「记一笔」悬浮按钮（与桌面下拉同源，一处生效两处）', () => {
  it.each<[label: string, close: boolean, reopen: boolean, expected: string[]]>([
    ['默认全开：五类型', false, false, FULL_FAB_LABELS],
    ['关闭投资：不含买入/卖出，其余三项照常', true, false, CLOSED_FAB_LABELS],
    ['重开投资：恢复五类型', true, true, FULL_FAB_LABELS],
  ])('%s', async (_label, close, reopen, expected) => {
    if (close) closeInvestments()
    const wrapper = await mountMobile()
    if (reopen) {
      reopenInvestments()
      await flushPromises()
    }
    expect(await openCreateFab(wrapper)).toEqual(expected)
  })
})

describe('关闭投资：记一笔裸键（issue #1245 / ADR-0116「不占键位」）', () => {
  beforeEach(() => {
    resetOverlays()
  })

  it('关闭投资：b/s 不触发弹窗，a/z/i 照常直达', async () => {
    closeInvestments()
    const wrapper = await mountView()
    pressKey('b')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    pressKey('s')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
    expect(wrapper.findComponent(TransactionForm).props('kind')).toBe('expense')
  })

  it.each([
    ['b', '买入', 'buy'],
    ['s', '卖出', 'sell'],
  ] as const)('重开投资：裸键 %s 恢复直达「记一笔 · %s」弹窗', async (key, kindLabel, kind) => {
    closeInvestments()
    const wrapper = await mountView()
    reopenInvestments()
    await flushPromises()
    pressKey(key)
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    expect(wrapper.findComponent(NModal).props('title')).toBe(`记一笔 · ${kindLabel}`)
    expect(wrapper.findComponent(TransactionForm).props('kind')).toBe(kind)
  })
})

describe('关闭投资不影响引用侧（issue #1245 验收：引用照常）', () => {
  it('关闭投资：既有 buy/sell 交易照常展示，按 kind 筛选的选项仍含买入/卖出', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { kind: 'buy' }),
      makeTxn(2, 'acc-1', { kind: 'sell' }),
      makeTxn(3, 'acc-1', { kind: 'expense' }),
    ])
    closeInvestments()
    const wrapper = await mountView()
    // 列表照常展示既有 buy/sell 行（kind 列标签）
    const text = wrapper.text()
    expect(text).toContain('买入')
    expect(text).toContain('卖出')
    // 按 kind 筛选的选项未被入口过滤波及（引用侧不受影响）
    const kindFilter = wrapper
      .findAllComponents(AppSelect)
      .map((select) => select.findComponent(NSelect).props('options') as Array<{ value: string }>)
      .find((options) => options.some((option) => option.value === 'buy'))
    expect(kindFilter, '类型筛选应仍含买入项').toBeDefined()
    expect(kindFilter!.map((option) => option.value)).toEqual(
      expect.arrayContaining(['buy', 'sell']),
    )
  })
})
