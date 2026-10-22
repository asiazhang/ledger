import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 保单域替身行为测试（issue #1974 / spec #1964 / ADR-0143）：接缝 = 应用所见
 * 的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 统计期望值为独立人算字面量（口径见 policy.ts 头注）：夹具保单三行——
 * 百万医疗险（mock-policy-active，止日 2027-06-30 在保）、综合意外险
 * （mock-policy-expired，止日 2025-12-31 已到期，对照替身今日 2026-01-15）、
 * 软删一行不进列表与统计。保费流水挂单（policy_id 归属）：
 * - active：mock-txn-306（expense 600_000，2025-06-01，#1973 定时保费并入后
 *   统一指向本保单）+ mock-txn-308（expense 360_000，2025-07-05，原号 201
 *   撞号改号）+ mock-txn-204（expense 360_000，2026-01-05）→ 已缴 1_320_000；
 *   mock-txn-203（income 500_000，理赔）→ 流入 500_000；挂靠缴费协议 pending
 *   期次 2026-02-01 / 03-01 / 04-01 → 下期扣款 2026-02-01（最早 pending）。
 * - expired：mock-txn-309（expense 120_000，2025-03-01，原号 202 撞号改号）→
 *   已缴 120_000、无挂靠 → 下期扣款 null；止日早于替身今日 → 已到期。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数（断言失效信号必发，ADR-0143 决策 3）。 */
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
      insurer_id: "mock-ins-picc", // 咬合 fixtureInsurers 字典行
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
      total_paid_native_cents: 1_320_000, // 600_000 + 360_000 × 2（独立人算）
      total_inflow_native_cents: 500_000,
      next_charge_date: "2026-02-01", // pending {02-01, 03-01, 04-01} 取最早
      is_expired: false,
    });
  });

  it("已到期保单：止日早于替身今日即已到期；无挂靠协议下期扣款 null", async () => {
    const row = await statsOf("mock-policy-expired");
    expect(row).toMatchObject({
      total_paid_native_cents: 120_000,
      total_inflow_native_cents: 0,
      next_charge_date: null,
      is_expired: true, // 2025-12-31 < 2026-01-15
    });
  });

  it("软删保单不产生统计行", async () => {
    const rows = await invoke<StatsRow[]>("list_policy_stats");
    expect(rows.find((r) => r.policy_id === "mock-policy-del")).toBeUndefined();
  });

  it("到期态随保障期间止实时推导：止日空 = 长期恒未到期，跨替身今日翻转", async () => {
    // 止日改到替身今日前一天 → 已到期
    await invoke<void>("update_policy", {
      id: "mock-policy-active",
      input: {
        insurer_id: "mock-ins-picc",
        policy_number: "PICC-2025-0066",
        product_name: "百万医疗险",
        start_date: "2025-07-01",
        end_date: "2026-01-14", // < 2026-01-15
      },
    });
    expect((await statsOf("mock-policy-active")).is_expired).toBe(true);

    // 止日改到替身今日当天 → 未到期（严格早于才到期）
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

    // 止日置空 → 长期/终身恒未到期
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
    expect((await statsOf("mock-policy-active")).total_paid_native_cents).toBe(960_000); // 1_320_000 − 360_000
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
      next_charge_date: null, // 新建无挂靠协议
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
    expect(after.total_paid_native_cents).toBe(1_680_000); // 1_320_000 + 360_000
    expect(after.total_inflow_native_cents).toBe(500_000); // 流入不受保费影响
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
    expect(row.total_paid_native_cents).toBe(120_000); // 已缴不变
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
        insurer_id: "mock-ins-picc", // 维持既有保司
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

    // 换成不存在的保司 → 拒绝
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
    // 历史流水引用原样保留（软删不级联，ADR-0051 决策 5）
    const txns = await invoke<{ total: number }>("list_transactions", {
      filter: { from: "2025-07-01", to: "2025-07-10" },
    });
    expect(txns.total).toBe(1); // mock-txn-308 仍在
  });
});
