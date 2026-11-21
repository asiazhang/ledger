import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

beforeEach(() => {
  installMockBackend();
});

function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

interface PolicyRow {
  id: string;
  insurer_id: string;
  policy_number: string;
  product_name: string;
  end_date: string | null;
  is_deleted: boolean;
}

interface StatsRow {
  policy_id: string;
  native_currency: string;
  total_paid_native_cents: number;
  total_inflow_native_cents: number;
  next_charge_date: string | null;
  is_expired: boolean;
}

async function statsOf(policyId: string): Promise<StatsRow> {
  const rows = await invoke<StatsRow[]>("list_policy_stats");
  const row = rows.find((r) => r.policy_id === policyId);
  if (!row) throw new Error(`测试辅助：统计行缺席 ${policyId}`);
  return row;
}

describe("保单列表读：夹具投影与保司咬合", () => {
  it("在保 / 已到期两行进列表，软删行缺席；保司引用咬合保司字典", async () => {
    const rows = await invoke<PolicyRow[]>("list_policies");
    expect(rows.map((r) => r.id).sort()).toEqual(["mock-policy-active", "mock-policy-expired"]);
    expect(rows.find((r) => r.id === "mock-policy-active")).toMatchObject({
      insurer_id: "mock-ins-picc",
      policy_number: "PICC-2025-0066",
      product_name: "百万医疗险",
    });
    expect(rows.find((r) => r.id === "mock-policy-expired")?.insurer_id).toBe("mock-ins-pingan");
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("list_policies");
    await invoke("list_policy_stats");
    expect(changed()).toBe(0);
  });
});

describe("保单统计读：挂单直和 + 下期扣款推导 + 到期态实时推导", () => {
  it("在保保单：保费三笔直和 1_320_000、理赔流入 500_000、下期扣款最早 pending 日", async () => {
    const row = await statsOf("mock-policy-active");
    expect(row).toMatchObject({
      native_currency: "CNY",
      total_paid_native_cents: 1_320_000,
      total_inflow_native_cents: 500_000,
      next_charge_date: "2026-02-01",
      is_expired: false,
    });
  });

  it("已到期保单：止日早于替身今日即已到期；无挂靠协议下期扣款 null", async () => {
    const row = await statsOf("mock-policy-expired");
    expect(row).toMatchObject({
      total_paid_native_cents: 120_000,
      total_inflow_native_cents: 0,
      next_charge_date: null,
      is_expired: true,
    });
  });

  it("软删保单不产生统计行", async () => {
    const rows = await invoke<StatsRow[]>("list_policy_stats");
    expect(rows.find((r) => r.policy_id === "mock-policy-del")).toBeUndefined();
  });

  it("到期态随保障期间止实时推导：止日空 = 长期恒未到期，跨替身今日翻转", async () => {
    await invoke<void>("update_policy", {
      id: "mock-policy-active",
      input: {
        insurer_id: "mock-ins-picc",
        policy_number: "PICC-2025-0066",
        product_name: "百万医疗险",
        start_date: "2025-07-01",
        end_date: "2026-01-14",
      },
    });
    expect((await statsOf("mock-policy-active")).is_expired).toBe(true);

    await invoke<void>("update_policy", {
      id: "mock-policy-active",
      input: {
        insurer_id: "mock-ins-picc",
        policy_number: "PICC-2025-0066",
        product_name: "百万医疗险",
        start_date: "2025-07-01",
        end_date: "2026-01-15",
      },
    });
    expect((await statsOf("mock-policy-active")).is_expired).toBe(false);

    await invoke<void>("update_policy", {
      id: "mock-policy-active",
      input: {
        insurer_id: "mock-ins-picc",
        policy_number: "PICC-2025-0066",
        product_name: "百万医疗险",
        start_date: "2025-07-01",
        end_date: null,
      },
    });
    const row = await statsOf("mock-policy-active");
    expect(row.is_expired).toBe(false);
  });

  it("保费挂单可对账：软删一笔保费流水，已缴直和同步收窄", async () => {
    await invoke<void>("delete_transaction", { id: "mock-txn-308" });
    expect((await statsOf("mock-policy-active")).total_paid_native_cents).toBe(960_000);
  });
});

describe("保单写闭环：建保单 → 列表出现 → 记保费 → 统计变化", () => {
  it("create_policy 落内存，列表与统计行出现（零合计），恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_policy", {
      input: {
        insurer_id: "mock-ins-pingan",
        policy_number: "PA-2026-0102",
        product_name: "重疾险",
        start_date: "2026-01-10",
        end_date: "2036-01-09",
        coverage_amount_cents: 30_000_000,
        coverage_currency_code: "CNY",
      },
    });
    expect(id).toBeTruthy();
    expect(changed()).toBe(1);

    const rows = await invoke<PolicyRow[]>("list_policies");
    expect(rows.find((r) => r.id === id)).toMatchObject({
      insurer_id: "mock-ins-pingan",
      product_name: "重疾险",
    });
    expect(await statsOf(id)).toMatchObject({
      total_paid_native_cents: 0,
      total_inflow_native_cents: 0,
      next_charge_date: null,
      is_expired: false,
    });
  });

  it("记一笔保费（create_transaction 挂 policy_id）→ 统计已缴变化", async () => {
    const before = await statsOf("mock-policy-active");
    expect(before.total_paid_native_cents).toBe(1_320_000);

    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 360_000,
        currency_code: "CNY",
        account_id: "mock-acc-ewallet",
        policy_id: "mock-policy-active",
        date: "2026-02-05",
        note: "续期保费",
      },
    });
    const after = await statsOf("mock-policy-active");
    expect(after.total_paid_native_cents).toBe(1_680_000);
    expect(after.total_inflow_native_cents).toBe(500_000);
  });

  it("理赔流入（income 挂 policy_id）只进流入合计", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "income",
        amount_cents: 20_000,
        currency_code: "CNY",
        account_id: "mock-acc-bank",
        policy_id: "mock-policy-expired",
        date: "2026-01-08",
        note: "意外医疗理赔",
      },
    });
    const row = await statsOf("mock-policy-expired");
    expect(row.total_paid_native_cents).toBe(120_000);
    expect(row.total_inflow_native_cents).toBe(20_000);
  });

  it("create_policy 保司不存在拒绝：不落行不发信号", async () => {
    const changed = captureLedgerChanged();
    await expect(
      invoke("create_policy", {
        input: {
          insurer_id: "mock-ins-404",
          policy_number: "X-1",
          product_name: "无源保单",
          start_date: "2026-01-01",
        },
      }),
    ).rejects.toThrow(/保司不存在/);
    expect(changed()).toBe(0);
  });

  it("update_policy 全量替换静态要素；维持既有保司可编辑（换保司才校验在用）", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("update_policy", {
      id: "mock-policy-active",
      input: {
        insurer_id: "mock-ins-picc",
        policy_number: "PICC-2025-0066-R",
        product_name: "百万医疗险（改）",
        start_date: "2025-07-01",
        end_date: "2027-06-30",
        note: "批注更新",
      },
    });
    expect(changed()).toBe(1);

    const rows = await invoke<PolicyRow[]>("list_policies");
    expect(rows.find((r) => r.id === "mock-policy-active")).toMatchObject({
      policy_number: "PICC-2025-0066-R",
      product_name: "百万医疗险（改）",
    });

    await expect(
      invoke("update_policy", {
        id: "mock-policy-active",
        input: {
          insurer_id: "mock-ins-404",
          policy_number: "X-2",
          product_name: "换关失败",
          start_date: "2025-07-01",
        },
      }),
    ).rejects.toThrow(/保司不存在/);
  });

  it("delete_policy 软删：列表与统计行同步缺席，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("delete_policy", { id: "mock-policy-active" });
    expect(changed()).toBe(1);

    const rows = await invoke<PolicyRow[]>("list_policies");
    expect(rows.map((r) => r.id)).toEqual(["mock-policy-expired"]);
    const stats = await invoke<StatsRow[]>("list_policy_stats");
    expect(stats.find((r) => r.policy_id === "mock-policy-active")).toBeUndefined();
    const txns = await invoke<{ total: number }>("list_transactions", {
      filter: { from: "2025-07-01", to: "2025-07-10" },
    });
    expect(txns.total).toBe(1);
  });
});
