import { vi, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import SubscriptionsPane from '@/components/scheduled/SubscriptionsPane.vue'
import type {
  Account,
  Category,
  Currency,
  Merchant,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
} from '@/types'

export const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

export const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

export const mockAccounts: Account[] = [
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

export const mockCategories: Category[] = [
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

export const mockMerchants: Merchant[] = [
  {
    id: 'mer-1',
    name: '视频平台',
    icon: null,
    color: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 订阅计划工厂：core.kind 固定 subscription，其余可覆写；merchant_id 为扩展字段。 */
export function makePlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  merchant_id: string | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
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
  return {
    core,
    merchant_id,
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
  }
}

export function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: 'unknown',
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

export function makeDetail(
  plan: ScheduledTransactionWithExt,
  pending_occurrences: ScheduledTransactionOccurrence[],
  failed_occurrences: ScheduledTransactionOccurrence[] = [],
): ScheduledTransactionDetail {
  return {
    core: plan.core,
    extension: {
      scheduled_transaction_id: plan.core.id,
      merchant_id: plan.merchant_id,
    },
    pending_occurrences,
    completed_occurrences: 0,
    occurrences: [...pending_occurrences, ...failed_occurrences],
  }
}

// —— invoke mock：可变数据源，状态操作后重载读得到最新值 ——
let mockPlans: ScheduledTransactionWithExt[] = []
export const mockDetails = new Map<string, ScheduledTransactionDetail>()
/** 订阅编辑失败开关（issue #162 拒绝路径测试用） */
let failSubscriptionUpdate = false
/** 商户字典 fixture（issue #190）：新建弹窗补全与列表商户列共用 */
let mockMerchantsState: Merchant[] = mockMerchants

/** 订阅花费总览 fixture（issue #160）：面板挂载即拉取，默认空数据 */
const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
}
let mockSpendOverview: SubscriptionSpendOverview = emptySpendOverview

// 拆分后主题测试文件对导入绑定只读，可变模块态经 setter 改写。
export function setMockPlans(rows: ScheduledTransactionWithExt[]) {
  mockPlans = rows
}
export function setFailSubscriptionUpdate(value: boolean) {
  failSubscriptionUpdate = value
}
export function setMockMerchants(rows: Merchant[]) {
  mockMerchantsState = rows
}

export function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchantsState)
    if (cmd === 'subscription_spend_overview') return Promise.resolve(mockSpendOverview)
    if (cmd === 'list_scheduled_transactions') return Promise.resolve(mockPlans)
    if (cmd === 'get_scheduled_transaction_detail') {
      const detail = mockDetails.get(String(args?.id))
      return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
    }
    if (cmd === 'create_scheduled_transaction') {
      const input = args?.input as { kind: string; note: string | null; merchant_id: string | null }
      const id = `new-${input.kind}-${input.note ?? ''}`
      const plan = makePlan(
        { id, note: input.note ?? null },
        input.merchant_id,
      )
      mockPlans = [...mockPlans, plan]
      mockDetails.set(id, makeDetail(plan, []))
      return Promise.resolve(id)
    }
    if (cmd === 'create_merchant') {
      const input = args?.input as { name: string }
      const id = `mer-new-${input.name}`
      return Promise.resolve(id)
    }
    if (cmd === 'update_scheduled_transaction_status') {
      const { id, new_status } = args as { id: string; new_status: string }
      mockPlans = mockPlans.map((p) =>
        p.core.id === id ? { ...p, core: { ...p.core, status: new_status } } : p,
      )
      const detail = mockDetails.get(id)
      if (detail) {
        mockDetails.set(id, { ...detail, core: { ...detail.core, status: new_status } })
      }
      return Promise.resolve()
    }
    if (cmd === 'update_scheduled_subscription') {
      if (failSubscriptionUpdate) {
        return Promise.reject(new Error('订阅金额不可编辑：改价 = 取消旧计划 + 新建'))
      }
      const input = args?.input as {
        id: string
        account_id: string
        category_id: string | null
        merchant_id: string | null
        note: string | null
      }
      mockPlans = mockPlans.map((p) =>
        p.core.id === input.id
          ? {
              ...p,
              core: {
                ...p.core,
                account_id: input.account_id,
                category_id: input.category_id,
                note: input.note,
              },
              merchant_id: input.merchant_id,
            }
          : p,
      )
      const detail = mockDetails.get(input.id)
      if (detail) {
        mockDetails.set(input.id, {
          ...detail,
          core: {
            ...detail.core,
            account_id: input.account_id,
            category_id: input.category_id,
            note: input.note,
          },
          extension: { ...detail.extension, merchant_id: input.merchant_id },
        })
      }
      return Promise.resolve()
    }
    if (cmd === 'execute_scheduled_occurrence') {
      // 重试语义：failed 期次 → completed（issue #205 期次详情弹窗）
      const { occurrence_id } = (args?.input ?? {}) as { occurrence_id: string }
      for (const [id, d] of mockDetails) {
        if (!d.occurrences.some((o) => o.id === occurrence_id && o.status === 'failed')) continue
        mockDetails.set(id, {
          ...d,
          occurrences: d.occurrences.map((o) =>
            o.id === occurrence_id ? { ...o, status: 'completed' as const } : o,
          ),
        })
      }
      return Promise.resolve('txn-new')
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

/** 定位弹窗表单内输入框：NModal teleport 到 body，需经 findComponent 锚定。 */
export function findInput(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.findComponent(`[data-testid="${testid}"]`).find('input')
}

export async function mountView() {
  const wrapper = mount(SubscriptionsPane)
  await flushPromises()
  return wrapper
}

/** 原 SubscriptionsPane.test.ts 顶层 beforeEach（280–292 行）收口：各主题文件显式调用。 */
export async function setup() {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockPlans = []
  mockDetails.clear()
  mockSpendOverview = emptySpendOverview
  failSubscriptionUpdate = false
  mockMerchantsState = mockMerchants
  baseInvoke()
  const store = useReferenceStore()
  await store.refresh()
}
