import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import PlanDetailModal from '@/components/scheduled/PlanDetailModal.vue'
import type {
  Account,
  Category,
  Currency,
  Merchant,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
} from '@/types'

const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '招商银行',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-1',
    name: '订阅服务',
    kind: 'expense',
    parent_id: null,
    icon: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

const mockMerchants: Merchant[] = []

function makeCore(partial: Partial<ScheduledTransaction> & { id: string }): ScheduledTransaction {
  return {
    kind: 'subscription',
    status: 'active',
    account_id: 'acc-1',
    category_id: 'cat-1',
    amount_cents: 1500,
    currency_code: 'CNY',
    recurrence_type: 'monthly',
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: '2026-01-01',
    note: '视频会员',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: 'plan-1',
    scheduled_date: '2026-03-01',
    status: 'pending',
    transaction_id: null,
    amount_cents: 1500,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

interface DetailParts {
  pending?: ScheduledTransactionOccurrence[]
  failed?: ScheduledTransactionOccurrence[]
  completed?: ScheduledTransactionOccurrence[]
  extension?: ScheduledTransactionDetail['extension']
}

function makeDetail(
  core: ScheduledTransaction,
  parts: DetailParts = {},
): ScheduledTransactionDetail {
  return {
    core,
    extension:
      parts.extension ?? {
        scheduled_transaction_id: core.id,
        merchant_id: null,
      },
    pending_occurrences: parts.pending ?? [],
    completed_occurrences: (parts.completed ?? []).length,
    failed_occurrences: parts.failed ?? [],
    completed_occurrence_list: parts.completed ?? [],
  }
}

// —— invoke mock：可变数据源，重试/展开后重载读得到最新值 ——
let mockDetails = new Map<string, ScheduledTransactionDetail>()

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'execute_scheduled_occurrence') {
      const { occurrence_id } = (args?.input ?? {}) as { occurrence_id: string }
      // 重试语义：failed 期次 → completed
      for (const [id, d] of mockDetails) {
        const failed = d.failed_occurrences.find((o) => o.id === occurrence_id)
        if (!failed) continue
        mockDetails.set(id, {
          ...d,
          failed_occurrences: d.failed_occurrences.filter((o) => o.id !== occurrence_id),
          completed_occurrence_list: [
            ...d.completed_occurrence_list,
            { ...failed, status: 'completed' as const },
          ],
        })
      }
      return Promise.resolve('txn-new')
    }
    if (cmd === 'expand_scheduled_occurrences') {
      const planId = String(args?.id)
      const d = mockDetails.get(planId)
      if (!d) return Promise.reject(new Error('无此计划详情'))
      const last = [...d.pending_occurrences].sort((a, b) =>
        b.scheduled_date.localeCompare(a.scheduled_date),
      )[0]
      const newDate = `${Number(last?.scheduled_date.slice(0, 4) ?? '2026') + 1}-01-01`
      const occ = makeOccurrence({
        id: 'occ-expanded',
        scheduled_transaction_id: planId,
        scheduled_date: newDate,
      })
      mockDetails.set(planId, {
        ...d,
        pending_occurrences: [...d.pending_occurrences, occ],
      })
      return Promise.resolve([occ.id])
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

// NModal 内容 teleport 到 body：内容断言与交互直接走 document.body
function q(sel: string): HTMLElement | null {
  return document.body.querySelector(sel)
}

function exists(sel: string): boolean {
  return q(sel) !== null
}

async function click(sel: string) {
  const el = q(sel)
  expect(el, `元素 ${sel} 应存在`).not.toBeNull()
  ;(el as HTMLElement).click()
  await flushPromises()
}

async function mountModal() {
  const wrapper = mount(PlanDetailModal)
  await flushPromises()
  return wrapper
}

async function openModal(wrapper: ReturnType<typeof mount>, id = 'plan-1') {
  const vm = wrapper.vm as unknown as { open: (id: string) => Promise<void> }
  await vm.open(id)
  await flushPromises()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockDetails = new Map()
  baseInvoke()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('PlanDetailModal 期次列表（issue #205）', () => {
  it('展示日期、金额、状态，待执行/失败/已完成按日期升序合并', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1' }), {
        pending: [makeOccurrence({ id: 'o2', scheduled_date: '2026-04-01' })],
        failed: [makeOccurrence({ id: 'f1', scheduled_date: '2026-02-01', status: 'failed' })],
        completed: [
          makeOccurrence({
            id: 'c1',
            scheduled_date: '2026-01-01',
            status: 'completed',
            transaction_id: 'txn-1',
          }),
        ],
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    const text = document.body.textContent ?? ''
    // 三种状态的期次都在列表中，按日期升序
    expect(text.indexOf('2026-01-01')).toBeLessThan(text.indexOf('2026-02-01'))
    expect(text.indexOf('2026-02-01')).toBeLessThan(text.indexOf('2026-04-01'))
    expect(q('[data-testid="occ-status-c1"]')!.textContent).toBe('已完成')
    expect(q('[data-testid="occ-status-f1"]')!.textContent).toBe('失败')
    expect(q('[data-testid="occ-status-o2"]')!.textContent).toBe('待执行')
    // 金额按计划币种展示（1500 分 = ¥15）
    expect(text).toContain('¥15')
  })

  it('重试按钮状态门控：仅 failed 期次有重试入口', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1' }), {
        pending: [makeOccurrence({ id: 'o1' })],
        failed: [makeOccurrence({ id: 'f1', scheduled_date: '2026-02-01', status: 'failed' })],
        completed: [makeOccurrence({ id: 'c1', status: 'completed' })],
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    expect(exists('[data-testid="occ-retry-f1"]')).toBe(true)
    expect(exists('[data-testid="occ-retry-o1"]')).toBe(false)
    expect(exists('[data-testid="occ-retry-c1"]')).toBe(false)
  })

  it('点击重试走既有单期执行命令，成功后期次状态更新', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1' }), {
        failed: [makeOccurrence({ id: 'f1', scheduled_date: '2026-02-01', status: 'failed' })],
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    await click('[data-testid="occ-retry-f1"]')
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'execute_scheduled_occurrence'),
    ).toBe(true)
    // 重试成功后重拉详情：期次已转为已完成，重试入口消失
    expect(q('[data-testid="occ-status-f1"]')!.textContent).toBe('已完成')
    expect(exists('[data-testid="occ-retry-f1"]')).toBe(false)
  })

  it('详情加载失败时显示加载失败占位', async () => {
    const wrapper = await mountModal()
    await openModal(wrapper, 'missing')
    expect(exists('[data-testid="occ-load-failed"]')).toBe(true)
  })
})

describe('PlanDetailModal 展开更多期次（issue #205）', () => {
  it('active 计划点击展开走既有期次展开命令并刷新，窗口外期次可见', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1' }), {
        pending: [makeOccurrence({ id: 'o1', scheduled_date: '2026-12-01' })],
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    await click('[data-testid="occ-expand"]')
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'expand_scheduled_occurrences'),
    ).toBe(true)
    // 展开后重拉详情：窗口外新期次出现在列表
    expect(exists('[data-testid="occ-date-occ-expanded"]')).toBe(true)
  })

  it('非 active 计划不显示展开按钮（后端同口径）', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1', status: 'cancelled' }), {
        pending: [],
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    expect(exists('[data-testid="occ-expand"]')).toBe(false)
  })

  it('有限期数计划期次已全部生成后不显示展开按钮', async () => {
    mockDetails.set(
      'plan-1',
      makeDetail(makeCore({ id: 'plan-1', kind: 'installment' }), {
        pending: [makeOccurrence({ id: 'o1' })],
        failed: [makeOccurrence({ id: 'f1', scheduled_date: '2026-02-01', status: 'failed' })],
        completed: [
          makeOccurrence({ id: 'c1', scheduled_date: '2026-01-01', status: 'completed' }),
        ],
        extension: {
          scheduled_transaction_id: 'plan-1',
          merchant_id: null,
          total_amount_cents: 4500,
          total_occurrences: 3,
        },
      }),
    )
    const wrapper = await mountModal()
    await openModal(wrapper)
    expect(exists('[data-testid="occ-expand"]')).toBe(false)
  })
})
