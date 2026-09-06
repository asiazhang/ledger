import { vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import type { Account, Category, Currency, Insurer, Merchant } from '@/types'

/**
 * 参考数据测试桩的单一来源（issue #725）。
 *
 * 根因回顾：每个测试文件手搓全量 `list_*` invoke 桩，参考数据每加一张表就要
 * 散弹式改全部文件；两分支并行各改一轮，合并时产生同回调重复桩（if 链先命中
 * 短路，后一条永远不生效），带数据桩被兜底空桩短路、数据静默变空。
 *
 * 深模块收口：本文件集中持有规范参考数据夹具与命令登记处，测试经
 * `stubReferenceInvoke(overrides?)` 一行接入——默认桩住参考 store 重拉的全部
 * `list_*` 命令，测试只覆写自己实际行使的命令；未覆写的非参考命令保持
 * `unexpected invoke` 拒绝（既有严格性是有价值的，予以保留）。
 *
 * 新增参考表 = 只改本文件（夹具 + `REFERENCE_DEFAULTS` 一行），全仓测试文件
 * 零改动；守门脚本 scripts/check-test-stubs.ts 从本文件的登记处提取命令清单，
 * 防止手搓桩回归。
 */

// —— 规范参考数据夹具：每张表一套，软删表含恰一行软删行 ——

/** 币种无软删概念（非 Syncable），规范集即单一 CNY 行。 */
export const refCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

export const refAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '现金',
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
  {
    id: 'acc-del',
    name: '已删账户',
    type: 'bank',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: true,
    is_hidden: false,
  },
]

export const refCategories: Category[] = [
  {
    id: 'cat-1',
    name: '餐饮',
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
  {
    id: 'cat-del',
    name: '已删分类',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: true,
  },
]

export const refMerchants: Merchant[] = [
  {
    id: 'mch-1',
    name: '京东',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'mch-del',
    name: '已删商户',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: true,
  },
]

export const refInsurers: Insurer[] = [
  {
    id: 'ins-1',
    name: '平安人寿',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'ins-del',
    name: '已删保司',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: true,
  },
]

/**
 * 参考 store 重拉的全部 `list_*` 命令 → 规范夹具（唯一登记处）。
 * 新增参考表：在上方补夹具、在此登记一行；守门脚本据此提取命令清单。
 */
export const REFERENCE_DEFAULTS: Record<string, unknown> = {
  list_currencies: refCurrencies,
  list_accounts: refAccounts,
  list_categories: refCategories,
  list_merchants: refMerchants,
  list_insurers: refInsurers,
}

/** 覆写值：固定返回值（JSON 可表达形态），或 `(args) => 返回值 | Promise`（可变库、
 *  在途、拒绝场景）。函数成员的 args 收窄为对象形态（tauri `InvokeArgs` 的 `number[]`
 *  缓冲区形态本应用不产生，收拢理由与边界见 helpers/invoke-mock.ts），覆写不标参数
 *  类型时经上下文推断获得；字段访问返回 `unknown`，具体形状在覆写体内断言。 */
export type ReferenceStubOverride =
  | ((args?: Record<string, unknown>) => unknown)
  | string
  | number
  | boolean
  | null
  | undefined
  | unknown[]
  | Record<string, unknown>

function isThenable(value: unknown): value is Promise<unknown> {
  return typeof (value as Promise<unknown> | undefined)?.then === 'function'
}

/**
 * 桩住参考 store 重拉的全部 `list_*` 命令（默认规范夹具），`overrides` 按命令
 * 覆写（参考命令与领域数据命令均可覆写；函数型覆写在派发时以 args 调用）。
 * 未覆写的非参考命令保持 `unexpected invoke` 拒绝。
 * 返回派发函数本身：`mockImplementationOnce` 等一次性桩处理完自己的领域命令后，
 * 可把其余命令委托回派发函数，避免手拷参考命令兑底（参考命令集合随助手演进）。
 *
 * 典型用法（beforeEach 内，mockReset 之后）：
 * ```ts
 * const base = stubReferenceInvoke({ list_transactions: () => Promise.resolve(txnDb) })
 * // 一次性桩：领域命令自己接，其余（含参考命令）委托回基础桩
 * mockInvoke.mockImplementationOnce((cmd, args) =>
 *   cmd === 'create_transaction' ? Promise.resolve('new-id') : base(cmd, args))
 * ```
 */
export function stubReferenceInvoke(
  overrides: Record<string, ReferenceStubOverride> = {},
): (cmd: string, args?: Record<string, unknown>) => Promise<unknown> {
  const dispatch = (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (cmd in overrides) {
      const handler = overrides[cmd]
      if (typeof handler === 'function') {
        const out = (handler as (a?: Record<string, unknown>) => unknown)(args)
        return isThenable(out) ? out : Promise.resolve(out)
      }
      return Promise.resolve(handler)
    }
    if (cmd in REFERENCE_DEFAULTS) return Promise.resolve(REFERENCE_DEFAULTS[cmd])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }
  vi.mocked(invoke).mockImplementation(dispatch as typeof invoke)
  return dispatch
}
