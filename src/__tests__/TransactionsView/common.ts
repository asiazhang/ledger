import { vi, beforeEach, expect } from "vitest";
import {
  mockInvoke,
  wireInvokeSeam,
  type InvokeSeamOverride,
  type InvokeSeamStaticValue,
} from "@ledger/test-support/invoke-mock";
import { fireProp } from "@ledger/test-support/component-vm";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { reactive } from "vue";
import { NDropdown, NModal, NPagination } from "naive-ui";
import { mountWithDialog } from "@ledger/test-support/mount";
import { makeTransaction } from "../factories";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import TransactionsView from "@/views/TransactionsView.vue";
import type { Account, Merchant, ReportDateRange, Transaction } from "@ledger/types";

export function setMerchantDb(rows: Merchant[]) {
  merchantDb = rows;
}
export function setTxnDb(rows: Transaction[]) {
  txnDb = rows;
}
export function setAccountDb(rows: Account[]) {
  mockAccounts = rows;
}

let reportDateRangeOverride: Promise<ReportDateRange> | null = null;
export function setReportDateRange(value: Promise<ReportDateRange> | null) {
  reportDateRangeOverride = value;
}

export let merchantDb: Merchant[] = [
  {
    id: "mch-1",
    name: "京东",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

export const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({
  query: {},
});
export const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: pushMock }),
}));

let mockAccounts: Account[] = [
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
    id: "acc-2",
    name: "银行",
    type: "bank",
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
];

export function makeTxn(
  i: number,
  accountId: string | null = "acc-1",
  overrides: Partial<Transaction> = {},
): Transaction {
  return makeTransaction({
    id: `txn-${String(i).padStart(3, "0")}`,
    amount_cents: i * 100,
    amount_native_cents: i * 100,
    note: `备注 ${i}`,
    account_id: accountId,
    ...overrides,
  });
}

let txnDb: Transaction[] = [];

function applyListFilter(filter: Record<string, unknown>) {
  return txnDb.filter((t) => {
    if (filter.involving_account_id) {
      const id = filter.involving_account_id as string;
      if (t.account_id !== id && t.to_account_id !== id && t.funding_account_id !== id)
        return false;
    }
    if (filter.merchant_id && t.merchant_id !== filter.merchant_id) return false;
    if (filter.from && t.date < (filter.from as string)) return false;
    if (filter.to && t.date > (filter.to as string)) return false;
    if (filter.category_id && t.category_id !== (filter.category_id as string)) return false;
    if (filter.uncategorized_only === true && t.category_id !== null) return false;
    if (Array.isArray(filter.kinds) && !filter.kinds.includes(t.kind)) return false;
    if (filter.hide_investment_related === true) {
      const investmentIds = new Set(
        mockAccounts.filter((a) => a.type === "investment").map((a) => a.id),
      );
      if (
        (t.account_id !== null && investmentIds.has(t.account_id)) ||
        (t.to_account_id !== null && investmentIds.has(t.to_account_id)) ||
        (t.funding_account_id !== null && investmentIds.has(t.funding_account_id))
      )
        return false;
    }
    return true;
  });
}

export const SHELL_DEFAULTS: Record<string, InvokeSeamStaticValue> = {
  list_policies: [],
  list_items: [],
};

export const SHELL_OVERRIDES: Record<string, InvokeSeamOverride> = {
  list_accounts: () => mockAccounts,
  list_merchants: () => merchantDb,
  report_date_range: () => {
    if (reportDateRangeOverride) return reportDateRangeOverride;
    const dates = txnDb.map((t) => t.date).sort();
    return Promise.resolve({
      min_date: dates[0] ?? null,
      max_date: dates[dates.length - 1] ?? null,
    });
  },
  list_transactions: (args?: { filter?: Record<string, unknown> }) => {
    const filter = args?.filter ?? {};
    const scoped = applyListFilter(filter);
    const pageSize = (filter.page_size as number) ?? scoped.length;
    const page = (filter.page as number) ?? 1;
    const start = (page - 1) * pageSize;
    return Promise.resolve({
      items: scoped.slice(start, start + pageSize),
      total: scoped.length,
    });
  },
  delete_transaction: (args?: { id?: string }) => {
    txnDb = txnDb.filter((t) => t.id !== args?.id);
    return Promise.resolve();
  },
};

beforeEach(async () => {
  pushMock.mockReset();
  routeMock.query = {};
  txnDb = Array.from({ length: 45 }, (_, i) => makeTxn(i + 1, i % 2 === 0 ? "acc-2" : "acc-1"));
  merchantDb = [
    {
      id: "mch-1",
      name: "京东",
      updated_at: "2026-01-01T00:00:00Z",
      version: 1,
      device_id: "test",
      is_deleted: false,
    },
  ];
  reportDateRangeOverride = null;
  const seam = wireInvokeSeam({
    defaults: SHELL_DEFAULTS,
    overrides: SHELL_OVERRIDES,
    refreshReferenceStores: true,
  });
  await seam.ready;
});

export async function mountView() {
  const wrapper = mountViewSync();
  await flushPromises();
  return wrapper;
}

export function mountViewSync() {
  return mountWithDialog(TransactionsView);
}

export function mountMobile() {
  setFakeMedia({ width: 839 });
  return mountView();
}

export function mountPhone() {
  setFakeMedia({ width: 839, hover: "none", pointer: "coarse" });
  return mountView();
}

export async function openCreateDropdown(wrapper: VueWrapper): Promise<string[]> {
  const arrow = wrapper.find('button[aria-label="更多记账类型"]');
  expect(arrow.exists()).toBe(true);
  await arrow.trigger("click");
  await flushPromises();
  return [...document.body.querySelectorAll(".n-dropdown-option-body__label")].map(
    (el) => el.textContent ?? "",
  );
}

export async function openCreateFab(wrapper: VueWrapper): Promise<string[]> {
  await wrapper.find(".create-fab").trigger("click");
  await flushPromises();
  return [...document.body.querySelectorAll(".create-fab-option")].map((el) =>
    (el.textContent ?? "").trim(),
  );
}

export function cards(wrapper: VueWrapper) {
  return wrapper.findAll(".transaction-card");
}

export function shownModal(wrapper: VueWrapper) {
  return wrapper.findAllComponents(NModal).find((m) => m.props("show") === true);
}

export async function closeShownModal(wrapper: VueWrapper) {
  const modal = shownModal(wrapper);
  expect(modal, "期望有展示中的弹窗").toBeTruthy();
  const handler = modal!.props("onUpdate:show") as unknown as
    | ((v: boolean) => void)
    | Array<(v: boolean) => void>;
  for (const fn of Array.isArray(handler) ? handler : [handler]) fn?.(false);
  await flushPromises();
}

export function listCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_transactions");
}

export function lastListFilter() {
  const calls = listCalls();
  const [, args] = calls[calls.length - 1] as [string, { filter: Record<string, unknown> }];
  return args.filter;
}

export function tablePagination(wrapper: VueWrapper) {
  const pagination = wrapper.findComponent(NPagination);
  return {
    get page() {
      return pagination.props("page") as number;
    },
    get itemCount() {
      return pagination.props("itemCount") as number;
    },
    onChange: (page: number) => fireProp(pagination, "onUpdate:page", page),
    onUpdatePageSize: (pageSize: number) => fireProp(pagination, "onUpdate:pageSize", pageSize),
  };
}

export function bodyRows(wrapper: VueWrapper) {
  return wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
}

export function deleteCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "delete_transaction");
}

export function createCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
}

export async function openMenuOnRow(wrapper: VueWrapper, index = 0) {
  await bodyRows(wrapper)[index].trigger("contextmenu");
  await flushPromises();
}

export function rowMenu(wrapper: VueWrapper) {
  return wrapper
    .findAllComponents(NDropdown)
    .find((d) =>
      (d.props("options") as Array<{ key?: string }>).some(
        (o) => o.key === "delete" || o.key === "detail",
      ),
    )!;
}

export function rowMenuKeys(wrapper: VueWrapper) {
  return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
}

export async function selectRowMenu(wrapper: VueWrapper, key: string) {
  fireProp(rowMenu(wrapper), "onSelect", key);
  await flushPromises();
}
