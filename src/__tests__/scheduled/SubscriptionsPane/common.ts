import { vi } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mountFlushed } from "@ledger/test-support/mount";
import { makeSubscriptionPlan } from "../../factories";
import SubscriptionsPane from "@/scheduled/SubscriptionsPane.vue";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}));

vi.mock("vue-chartjs", async () => {
  const { BarChartStub } = await import("../../line-chart-stub");
  return { Bar: BarChartStub };
});
import type {
  Account,
  Category,
  Merchant,
  ScheduledStatus,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
} from "@ledger/types";

export { findInputByTestId as findInput } from "@ledger/test-support/dom";

export const mockAccounts: Account[] = [
  {
    id: "acc-1",
    name: "招商银行",
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
];

const mockCategories: Category[] = [
  {
    id: "cat-1",
    name: "订阅服务",
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
];

export const mockMerchants: Merchant[] = [
  {
    id: "mer-1",
    name: "视频平台",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

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
      policy_id: plan.policy_id,
    },
    pending_occurrences,
    completed_occurrences: 0,
    completed_amount_cents: 0,
    occurrences: [...pending_occurrences, ...failed_occurrences],
  };
}

let mockPlans: ScheduledTransactionWithExt[] = [];
export const mockDetails = new Map<string, ScheduledTransactionDetail>();
let failSubscriptionUpdate = false;
let mockMerchantsState: Merchant[] = mockMerchants;

const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: "CNY",
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
};
let mockSpendOverview: SubscriptionSpendOverview = emptySpendOverview;

export function setMockPlans(rows: ScheduledTransactionWithExt[]) {
  mockPlans = rows;
}
export function setFailSubscriptionUpdate(value: boolean) {
  failSubscriptionUpdate = value;
}
export function setMockMerchants(rows: Merchant[]) {
  mockMerchantsState = rows;
}
let accountsDb: Account[] = mockAccounts;
export function setMockAccounts(rows: Account[]) {
  accountsDb = rows;
}

export async function mountView() {
  return mountFlushed(SubscriptionsPane);
}

export async function setup() {
  mockPlans = [];
  mockDetails.clear();
  mockSpendOverview = emptySpendOverview;
  failSubscriptionUpdate = false;
  mockMerchantsState = mockMerchants;
  accountsDb = mockAccounts;
  const seam = wireInvokeSeam({
    defaults: {
      list_categories: mockCategories,
    },
    overrides: {
      list_accounts: () => accountsDb,
      list_merchants: () => mockMerchantsState,
      subscription_spend_overview: () => mockSpendOverview,
      list_scheduled_transactions: () => mockPlans,
      get_scheduled_transaction_detail: (args?: Record<string, unknown>) => {
        const detail = mockDetails.get(String(args?.id));
        return detail ? Promise.resolve(detail) : Promise.reject(new Error("无此计划详情"));
      },
      create_scheduled_transaction: (args?: Record<string, unknown>) => {
        const input = args?.input as {
          kind: string;
          note: string | null;
          merchant_id: string | null;
        };
        const id = `new-${input.kind}-${input.note ?? ""}`;
        const plan = makeSubscriptionPlan({ id, note: input.note ?? null }, input.merchant_id);
        mockPlans = [...mockPlans, plan];
        mockDetails.set(id, makeDetail(plan, []));
        return Promise.resolve(id);
      },
      create_merchant: (args?: Record<string, unknown>) => {
        const input = args?.input as { name: string };
        const id = `mer-new-${input.name}`;
        return Promise.resolve(id);
      },
      update_scheduled_transaction_status: (args?: Record<string, unknown>) => {
        const { id, new_status } = args as { id: string; new_status: ScheduledStatus };
        mockPlans = mockPlans.map((p) =>
          p.core.id === id ? { ...p, core: { ...p.core, status: new_status } } : p,
        );
        const detail = mockDetails.get(id);
        if (detail) {
          mockDetails.set(id, { ...detail, core: { ...detail.core, status: new_status } });
        }
        return Promise.resolve();
      },
      update_scheduled_subscription: (args?: Record<string, unknown>) => {
        if (failSubscriptionUpdate) {
          return Promise.reject(new Error("订阅金额不可编辑：改价 = 取消旧计划 + 新建"));
        }
        const input = args?.input as {
          id: string;
          account_id: string;
          category_id: string | null;
          merchant_id: string | null;
          note: string | null;
        };
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
        );
        const detail = mockDetails.get(input.id);
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
          });
        }
        return Promise.resolve();
      },
      execute_scheduled_occurrence: (args?: Record<string, unknown>) => {
        const { occurrence_id } = (args?.input ?? {}) as { occurrence_id: string };
        for (const [id, d] of mockDetails) {
          if (!d.occurrences.some((o) => o.id === occurrence_id && o.status === "failed")) continue;
          mockDetails.set(id, {
            ...d,
            occurrences: d.occurrences.map((o) =>
              o.id === occurrence_id ? { ...o, status: "completed" as const } : o,
            ),
          });
        }
        return Promise.resolve("txn-new");
      },
    },
    refreshReferenceStores: true,
  });
  await seam.ready;
}
