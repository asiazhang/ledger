import type { Account, Category, Currency, Insurer, Merchant } from '@ledger/types'

/**
 * 参考数据测试桩的单一来源（issue #725；收尾票 #750 起本文件只承载夹具与登记处）。
 *
 * 根因回顾：每个测试文件手搓全量 `list_*` invoke 桩，参考数据每加一张表就要
 * 散弹式改全部文件；两分支并行各改一轮，合并时产生同回调重复桩（if 链先命中
 * 短路，后一条永远不生效），带数据桩被兜底空桩短路、数据静默变空。
 *
 * 深模块收口：本文件集中持有规范参考数据夹具与命令登记处；接线能力在唯一接缝
 * `wireInvokeSeam`（helpers/invoke-mock.ts）——参考字典五命令由接缝经
 * `REFERENCE_DEFAULTS` 内建兜底应答，测试只覆写自己实际行使的命令；未覆写的
 * 非参考命令保持
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
