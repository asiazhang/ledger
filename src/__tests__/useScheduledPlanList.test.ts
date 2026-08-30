import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, watch } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import {
  earliestPendingOccurrence,
  scheduledRecurrenceLabel,
  SCHEDULED_RECURRENCE_OPTIONS,
  useScheduledPlanList,
  type ScheduledPlanRow,
  type UseScheduledPlanListReturn,
} from '@/composables/useScheduledPlanList'
import type {
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from '@/types'

// 提示捕获：模块的生命周期成功/失败提示是接口行为的一部分。
// setup.ts 的全局 useMessage mock 每次调用返回新对象，无法跨调用断言；
// 此处以共享记录器覆盖（壳内不渲染 naive-ui 组件，其余导出保留原样）。
const messageCalls: Array<{ method: string; text: string }> = []
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  const record =
    (method: string) =>
    (...args: unknown[]) =>
      messageCalls.push({ method, text: String(args[0]) })
  return {
    ...actual,
    useMessage: () => ({
      success: record('success'),
      warning: record('warning'),
      error: record('error'),
      info: record('info'),
      loading: record('loading'),
      destroyAll: () => {},
    }),
  }
})

const mockInvoke = vi.mocked(invoke)

// ---------------------------------------------------------------------------
// 数据工厂：计划（core.kind 可覆写）、期次、详情
// ---------------------------------------------------------------------------

function makePlan(
  partial: Partial<ScheduledTransactionWithExt['core']> & { id: string },
  ext: Partial<ScheduledTransactionWithExt> = {},
): ScheduledTransactionWithExt {
  const core = {
    kind: 'scheduled_transfer' as const,
    status: 'active' as const,
    account_id: 'acc-cny1',
    category_id: null,
    amount_cents: 50000,
    currency_code: 'CNY',
    recurrence_type: 'monthly' as const,
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: '2026-01-01',
    note: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
  return {
    core,
    merchant_id: null,
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
    ...ext,
  }
}

function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: 'unknown',
    scheduled_date: '2026-03-01',
    status: 'pending',
    transaction_id: null,
    amount_cents: 50000,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

function makeDetail(
  plan: ScheduledTransactionWithExt,
  pending_occurrences: ScheduledTransactionOccurrence[],
): ScheduledTransactionDetail {
  return {
    core: plan.core,
    extension: {
      scheduled_transaction_id: plan.core.id,
      to_account_id: plan.to_account_id ?? '',
      total_occurrences: plan.total_occurrences,
    },
    pending_occurrences,
    completed_occurrences: 0,
    completed_amount_cents: 0,
    occurrences: pending_occurrences,
  }
}

// ---------------------------------------------------------------------------
// invoke mock：可变数据源，状态操作后重载读得到最新值
// ---------------------------------------------------------------------------

let mockPlans: ScheduledTransactionWithExt[] = []
const mockDetails = new Map<string, ScheduledTransactionDetail>()
let failList = false

function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_scheduled_transactions') {
      return failList ? Promise.reject(new Error('数据库不可用')) : Promise.resolve(mockPlans)
    }
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'update_scheduled_transaction_status') {
      const { id, new_status } = args?.input as { id: string; new_status: string }
      mockPlans = mockPlans.map((p) =>
        p.core.id === id ? { ...p, core: { ...p.core, status: new_status as never } } : p,
      )
      return Promise.resolve()
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

// ---------------------------------------------------------------------------
// 消费契约镜像（ADR-0030 先例）：最小挂载壳直打工厂实例；模块内化请求发起，
// 镜像 = 监听 refreshVersion，每次重拉完成时记录行快照（断言「动作 → 重拉次数
// 与行终态」），同步批次内多次 bump 由 watcher 天然逐次记录。
// ---------------------------------------------------------------------------

/** 转账行扩展：下期 = 最早 pending 期次（详情失败时为 null）。 */
interface TransferExt {
  next: ScheduledTransactionOccurrence | null
}
type TransferRow = ScheduledPlanRow<TransferExt>

let harness: {
  list: UseScheduledPlanListReturn<TransferExt>
  pulls: TransferRow[][]
  detailOpened: string[]
  counters: { statusChanged: number }
} | null = null

const Harness = defineComponent({
  setup() {
    const detailOpened: string[] = []
    const counters = { statusChanged: 0 }
    const list = useScheduledPlanList<TransferExt>({
      kind: 'scheduled_transfer',
      expandDetail: (_plan, detail) => ({
        next: detail ? earliestPendingOccurrence(detail) : null,
      }),
      loadErrorText: '加载定时转账失败',
      cancelConfirmText: '取消后不再自动转账，已生成的交易与历史期次保留。确认取消？',
      onStatusChanged: () => {
        counters.statusChanged += 1
      },
      onOpenDetail: (row) => {
        detailOpened.push(row.plan.core.id)
      },
    })
    const pulls: TransferRow[][] = []
    watch(list.refreshVersion, () => {
      pulls.push(JSON.parse(JSON.stringify(list.rows.value)) as TransferRow[])
    })
    harness = { list, pulls, detailOpened, counters }
    return () => null
  },
})

function mountHarness() {
  mount(Harness)
  return harness!
}

function lastMessage(method: string): string {
  const found = [...messageCalls].reverse().find((c) => c.method === method)
  expect(found, `应有一次 ${method} 提示`).toBeDefined()
  return found!.text
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockPlans = []
  mockDetails.clear()
  failList = false
  messageCalls.length = 0
  baseInvoke()
})

describe('useScheduledPlanList 初始状态', () => {
  it('空行、不在加载、默认过滤「进行中」、版本号 0', () => {
    const { list } = mountHarness()
    expect(list.rows.value).toEqual([])
    expect(list.loading.value).toBe(false)
    expect(list.statusFilter.value).toBe('active')
    expect(list.refreshVersion.value).toBe(0)
    expect(list.filteredRows.value).toEqual([])
  })

  it('状态过滤选项集按形态：转账含「已完成」（能力有无，不重定义状态语义）', () => {
    const { list } = mountHarness()
    expect(list.statusFilterOptions).toEqual([
      { key: 'active', label: '进行中' },
      { key: 'paused', label: '已暂停' },
      { key: 'cancelled', label: '已取消' },
      { key: 'completed', label: '已完成' },
    ])
  })
})

describe('useScheduledPlanList 清单加载', () => {
  it('按形态过滤清单：只加载本形态计划，其他形态不进行', async () => {
    mockPlans = [
      makePlan({ id: 't1', note: '月度储蓄' }),
      makePlan({ id: 's1', note: '某订阅', kind: 'subscription' }),
      makePlan({ id: 'i1', note: '某分期', kind: 'installment' }),
    ]
    mockDetails.set('t1', makeDetail(mockPlans[0], []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    expect(list.rows.value.map((r) => r.plan.core.id)).toEqual(['t1'])
    expect(list.refreshVersion.value).toBe(1)
  })

  it('详情扩展：下期取最早 pending 期次（乱序输入取日期最早，不现场推算）；无 pending 为 null', async () => {
    const plan = makePlan({ id: 't1', note: '月度储蓄' })
    mockPlans = [plan]
    mockDetails.set(
      't1',
      makeDetail(plan, [
        makeOccurrence({ id: 'o2', scheduled_date: '2026-04-01' }),
        makeOccurrence({ id: 'o1', scheduled_date: '2026-03-01' }),
      ]),
    )
    const plan2 = makePlan({ id: 't2', note: '已完成一次性' })
    mockPlans.push(plan2)
    mockDetails.set('t2', makeDetail(plan2, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    const rows = list.rows.value
    expect(rows.find((r) => r.plan.core.id === 't1')!.ext.next!.scheduled_date).toBe('2026-03-01')
    expect(rows.find((r) => r.plan.core.id === 't2')!.ext.next).toBeNull()
    expect(rows.every((r) => !r.detailFailed)).toBe(true)
  })

  it('详情命令失败的行标记 detailFailed（与「无数据」区分，不静默），其余行照常', async () => {
    const ok = makePlan({ id: 'ok1', note: '正常行' })
    const bad = makePlan({ id: 'bad1', note: '详情失败行' })
    mockPlans = [ok, bad]
    mockDetails.set('ok1', makeDetail(ok, []))
    // bad1 详情命令将失败
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    expect(list.rows.value.find((r) => r.plan.core.id === 'bad1')!.detailFailed).toBe(true)
    expect(list.rows.value.find((r) => r.plan.core.id === 'bad1')!.ext.next).toBeNull()
    expect(list.rows.value.find((r) => r.plan.core.id === 'ok1')!.detailFailed).toBe(false)
    // 详情失败不拖垮整单加载：版本号照常 bump
    expect(list.refreshVersion.value).toBe(1)
  })

  it('清单命令失败：错误提示（形态文案归一）、loading 收尾、版本号不 bump、行保持旧值', async () => {
    const plan = makePlan({ id: 't1' })
    mockPlans = [plan]
    mockDetails.set('t1', makeDetail(plan, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    expect(list.rows.value).toHaveLength(1)
    failList = true
    await list.load()
    await flushPromises()
    expect(lastMessage('error')).toBe('加载定时转账失败: 数据库不可用')
    expect(list.loading.value).toBe(false)
    expect(list.refreshVersion.value).toBe(1)
    expect(list.rows.value).toHaveLength(1)
  })
})

describe('useScheduledPlanList 状态过滤', () => {
  it('前端过滤即时生效：completed 行经「已完成」过滤可见；切换过滤不发请求', async () => {
    const done = makePlan({ id: 'd1', note: '一次性转账', status: 'completed' })
    const active = makePlan({ id: 'a1', note: '循环转账' })
    mockPlans = [done, active]
    mockDetails.set('d1', makeDetail(done, []))
    mockDetails.set('a1', makeDetail(active, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    expect(list.filteredRows.value.map((r) => r.plan.core.id)).toEqual(['a1'])
    list.setStatusFilter('completed')
    await flushPromises()
    expect(list.statusFilter.value).toBe('completed')
    expect(list.filteredRows.value.map((r) => r.plan.core.id)).toEqual(['d1'])
    // 状态过滤是纯前端过滤：不产生重拉
    expect(list.refreshVersion.value).toBe(1)
  })

  it('已暂停与已取消行经对应过滤可见（迁自原组件测试，承接覆盖）', async () => {
    const paused = makePlan({ id: 'p1', note: '已暂停转账', status: 'paused' })
    const cancelled = makePlan({ id: 'c1', note: '已取消转账', status: 'cancelled' })
    mockPlans = [paused, cancelled]
    mockDetails.set('p1', makeDetail(paused, []))
    mockDetails.set('c1', makeDetail(cancelled, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    list.setStatusFilter('paused')
    await flushPromises()
    expect(list.filteredRows.value.map((r) => r.plan.core.id)).toEqual(['p1'])
    list.setStatusFilter('cancelled')
    await flushPromises()
    expect(list.filteredRows.value.map((r) => r.plan.core.id)).toEqual(['c1'])
  })
})

describe('useScheduledPlanList Plan Lifecycle 操作', () => {
  it('暂停：走既有状态命令（参数正确）、成功提示、重拉一次反映状态终态、回调被通知', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const { list, counters, pulls } = mountHarness()
    await list.load()
    await flushPromises()
    await list.changeStatus('a1', 'paused')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { id: string; new_status: string } }).input.id === 'a1' &&
          (args as { input: { new_status: string } }).input.new_status === 'paused',
      ),
    ).toBe(true)
    expect(lastMessage('success')).toBe('已暂停')
    expect(list.refreshVersion.value).toBe(2)
    expect(pulls).toHaveLength(2)
    expect(list.rows.value[0]!.plan.core.status).toBe('paused')
    expect(counters.statusChanged).toBe(1)
  })

  it('恢复：paused → active，成功提示「已恢复」', async () => {
    const plan = makePlan({ id: 'p1', status: 'paused' })
    mockPlans = [plan]
    mockDetails.set('p1', makeDetail(plan, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    await list.changeStatus('p1', 'active')
    await flushPromises()
    expect(lastMessage('success')).toBe('已恢复')
    expect(list.rows.value[0]!.plan.core.status).toBe('active')
  })

  it('取消：→ cancelled，成功提示「已取消」', async () => {
    const plan = makePlan({ id: 'c1' })
    mockPlans = [plan]
    mockDetails.set('c1', makeDetail(plan, []))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    await list.changeStatus('c1', 'cancelled')
    await flushPromises()
    expect(lastMessage('success')).toBe('已取消')
    expect(list.rows.value[0]!.plan.core.status).toBe('cancelled')
  })

  it('状态命令失败：「操作失败」错误提示、不重拉', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const { list, pulls } = mountHarness()
    await list.load()
    await flushPromises()
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'update_scheduled_transaction_status')
        return Promise.reject(new Error('状态不允许变更'))
      if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    await list.changeStatus('a1', 'paused')
    await flushPromises()
    expect(lastMessage('error')).toBe('操作失败: 状态不允许变更')
    expect(list.refreshVersion.value).toBe(1)
    expect(pulls).toHaveLength(1)
  })
})

describe('useScheduledPlanList 行操作描述符', () => {
  it('可用性矩阵：active = 期次/暂停/取消；paused = 期次/恢复/取消；completed 与 cancelled 仅期次', async () => {
    const plans = [
      makePlan({ id: 'a1', status: 'active' }),
      makePlan({ id: 'p1', status: 'paused' }),
      makePlan({ id: 'd1', status: 'completed' }),
      makePlan({ id: 'c1', status: 'cancelled' }),
    ]
    mockPlans = plans
    plans.forEach((p) => mockDetails.set(p.core.id, makeDetail(p, [])))
    const { list } = mountHarness()
    await list.load()
    await flushPromises()
    const availableKeys = (row: ScheduledPlanRow<TransferExt>) =>
      list
        .rowActions(row)
        .filter((a) => a.available)
        .map((a) => a.key)
    const row = (id: string) => list.rows.value.find((r) => r.plan.core.id === id)!
    expect(availableKeys(row('a1'))).toEqual(['detail', 'pause', 'cancel'])
    expect(availableKeys(row('p1'))).toEqual(['detail', 'resume', 'cancel'])
    expect(availableKeys(row('d1'))).toEqual(['detail'])
    expect(availableKeys(row('c1'))).toEqual(['detail'])
  })

  it('描述符自带标签、确认文案（确认弹层留适配器）与 run 动作；run 接通详情回调与状态命令', async () => {
    const plan = makePlan({ id: 'a1' })
    mockPlans = [plan]
    mockDetails.set('a1', makeDetail(plan, []))
    const { list, detailOpened } = mountHarness()
    await list.load()
    await flushPromises()
    const row = list.rows.value[0]!
    const actions = Object.fromEntries(list.rowActions(row).map((a) => [a.key, a]))
    expect(actions.detail!.label).toBe('期次')
    expect(actions.pause!.label).toBe('暂停')
    expect(actions.resume!.label).toBe('恢复')
    expect(actions.cancel!.label).toBe('取消')
    expect(actions.cancel!.confirm).toBe(
      '取消后不再自动转账，已生成的交易与历史期次保留。确认取消？',
    )
    expect(actions.pause!.confirm).toBeNull()

    actions.detail!.run()
    expect(detailOpened).toEqual(['a1'])
    actions.cancel!.run()
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(
        ([cmd, args]) =>
          cmd === 'update_scheduled_transaction_status' &&
          (args as { input: { new_status: string } }).input.new_status === 'cancelled',
      ),
    ).toBe(true)
  })
})

describe('useScheduledPlanList 工厂形态', () => {
  it('每次调用返回独立实例：状态与版本号互不串扰', async () => {
    const first = mountHarness()
    await first.list.load()
    await flushPromises()
    const second = mountHarness()
    expect(first.list.refreshVersion.value).toBe(1)
    expect(second.list.refreshVersion.value).toBe(0)
    expect(second.list.rows.value).toEqual([])
    second.list.setStatusFilter('paused')
    await flushPromises()
    expect(second.list.statusFilter.value).toBe('paused')
    expect(first.list.statusFilter.value).toBe('active')
  })
})

describe('周期选项与周期标签单源（#309 显式可见变化：转账下拉统一为「每天/每周/每月/每年」）', () => {
  it('周期选项表单源：四项与 RecurrenceType 一一对应', () => {
    expect(SCHEDULED_RECURRENCE_OPTIONS).toEqual([
      { label: '每天', value: 'daily' },
      { label: '每周', value: 'weekly' },
      { label: '每月', value: 'monthly' },
      { label: '每年', value: 'yearly' },
    ])
  })

  it('周期标签：interval=1「每X」、interval>1「每N X」、未知类型兜底显示原值', () => {
    expect(scheduledRecurrenceLabel('monthly', 1)).toBe('每月')
    expect(scheduledRecurrenceLabel('monthly', 2)).toBe('每2月')
    expect(scheduledRecurrenceLabel('weekly', 1)).toBe('每周')
    expect(scheduledRecurrenceLabel('daily', 3)).toBe('每3天')
    expect(scheduledRecurrenceLabel('yearly', 1)).toBe('每年')
    expect(scheduledRecurrenceLabel('unknown', 1)).toBe('每unknown')
  })

  it('earliestPendingOccurrence：空 pending 返回 null', () => {
    const plan = makePlan({ id: 't1' })
    expect(earliestPendingOccurrence(makeDetail(plan, []))).toBeNull()
  })
})
