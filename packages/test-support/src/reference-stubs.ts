import type { Account, Category, Currency, Insurer, Merchant } from "@ledger/types";

export const refCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
];

export const refAccounts: Account[] = [
  {
    id: "acc-1",
    name: "现金",
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: false,
  },
  {
    id: "acc-del",
    name: "已删账户",
    type: "bank",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
    is_hidden: false,
    is_archived: false,
  },
];

export const refCategories: Category[] = [
  {
    id: "cat-1",
    name: "餐饮",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "cat-del",
    name: "已删分类",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
  },
];

export const refMerchants: Merchant[] = [
  {
    id: "mch-1",
    name: "京东",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "mch-del",
    name: "已删商户",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
  },
];

export const refInsurers: Insurer[] = [
  {
    id: "ins-1",
    name: "平安人寿",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "ins-del",
    name: "已删保司",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
  },
];

export const REFERENCE_DEFAULTS: Record<string, unknown> = {
  list_currencies: refCurrencies,
  list_accounts: refAccounts,
  list_categories: refCategories,
  list_merchants: refMerchants,
  list_insurers: refInsurers,
};
