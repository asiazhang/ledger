import { wireInvokeSeam } from '../../helpers/invoke-mock'
import { mountFlushed } from '../../helpers/mount'
import SubscriptionsPane from '@/components/scheduled/SubscriptionsPane.vue'
import type {
  Account,
  Category,
  Merchant,
  ScheduledStatus,
  ScheduledTransaction,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
} from '@/types'

/**
 * SubscriptionsPane 测试目录薄壳（issue #748，ADR-0085 决策 7）：只承载本目录
 * 特有夹具与编排组合——计划三件套夹具族、可变数据源（重载读最新值）、面板挂载
 * 编排。通用布线（wireInvokeSeam）、清理四件套（全局壳层每测自动执行）、挂载与
 * DOM 查找（helpers/mount、helpers/dom）一律上收测试辅助层，禁止薄壳再生长
 * 通用能力。
 */

// 查找助手直用测试辅助层出口（本目录惯用名 findInput 保持主题文件导入不变）。
export { findInputByTestId as findInput } from '../../helpers/dom'

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
    sort_order: 0,
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
    policy_id: null,
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
    // 本目录计划恒为订阅形态（makePlan 固定 kind: 'subscription'），
    // extension 按 SubscriptionPlan 全字段装配（policy_id 随 WithExt 透传）
    extension: {
      scheduled_transaction_id: plan.core.id,
      merchant_id: plan.merchant_id,
      policy_id: plan.policy_id,
    },
    pending_occurrences,
    completed_occurrences: 0,
    completed_amount_cents: 0,
    occurrences: [...pending_occurrences, ...failed_occurrences],
  }
}

// —— 可变数据源，状态操作后重载读得到最新值 ——
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

export async function mountView() {
  return mountFlushed(SubscriptionsPane)
}

/** 各主题文件 beforeEach 显式调用：目录态重置 + 唯一接缝布线 + 参考数据预热。 */
export async function setup() {
  mockPlans = []
  mockDetails.clear()
  mockSpendOverview = emptySpendOverview
  failSubscriptionUpdate = false
  mockMerchantsState = mockMerchants
  // 唯一接缝布线（ADR-0085）：账户与分类以本套夹具覆写（值与规范夹具不同，
  // 属场景契约而非重复枚举），进 defaults 表；可变库与行为编排（订阅 CRUD、
  // 状态机、重试、花费总览）为函数型 overrides。其余参考命令由规范夹具兑底；
  // store 层预热 opt-in 开启：主题用例依赖参考数据就绪后的即时渲染。
  const seam = wireInvokeSeam({
    defaults: {
      list_accounts: mockAccounts,
      list_categories: mockCategories,
    },
    overrides: {
      list_merchants: () => mockMerchantsState,
      subscription_spend_overview: () => mockSpendOverview,
      list_scheduled_transactions: () => mockPlans,
      get_scheduled_transaction_detail: (args?: Record<string, unknown>) => {
        const detail = mockDetails.get(String(args?.id))
        return detail ? Promise.resolve(detail) : Promise.reject(new Error('无此计划详情'))
      },
      create_scheduled_transaction: (args?: Record<string, unknown>) => {
        const input = args?.input as { kind: string; note: string | null; merchant_id: string | null }
        const id = `new-${input.kind}-${input.note ?? ''}`
        const plan = makePlan(
          { id, note: input.note ?? null },
          input.merchant_id,
        )
        mockPlans = [...mockPlans, plan]
        mockDetails.set(id, makeDetail(plan, []))
        return Promise.resolve(id)
      },
      create_merchant: (args?: Record<string, unknown>) => {
        const input = args?.input as { name: string }
        const id = `mer-new-${input.name}`
        return Promise.resolve(id)
      },
      update_scheduled_transaction_status: (args?: Record<string, unknown>) => {
        const { id, new_status } = args as { id: string; new_status: ScheduledStatus }
        mockPlans = mockPlans.map((p) =>
          p.core.id === id ? { ...p, core: { ...p.core, status: new_status } } : p,
        )
        const detail = mockDetails.get(id)
        if (detail) {
          mockDetails.set(id, { ...detail, core: { ...detail.core, status: new_status } })
        }
        return Promise.resolve()
      },
      update_scheduled_subscription: (args?: Record<string, unknown>) => {
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
      },
      execute_scheduled_occurrence: (args?: Record<string, unknown>) => {
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
      },
    },
    refreshReferenceStores: true,
  })
  await seam.ready
}
