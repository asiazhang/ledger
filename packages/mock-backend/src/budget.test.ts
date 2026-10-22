import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 预算域替身行为测试（issue #1967 / spec #1964 / ADR-0143）：接缝 = 应用所见
 * 的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 聚合期望值为独立人算字面量：夹具交易里 expense 仅 mock-txn-001（餐饮 2500，
 * 2026-01-05），替身「今日」锚定 2026-01-15（当月 = 2026-01、本年 = 2026）。
 * 替身从简口径（budget.ts 头注记录与真后端差异）：spent = 当期该分类
 * kind=expense 交易直和，不冲减退款（expense_net）、不展开父分类预算覆盖。
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

describe("预算清单读：夹具投影", () => {
  it("list_budgets 返回月度/年度两类种子行", async () => {
    const budgets =
      await invoke<
        Array<{ id: string; category_id: string; period: string; amount_cents: number }>
      >("list_budgets");
    expect(
      budgets.map(({ id, category_id, period, amount_cents }) => ({
        id,
        category_id,
        period,
        amount_cents,
      })),
    ).toEqual([
      {
        id: "mock-bud-food-monthly",
        category_id: "mock-cat-food",
        period: "monthly",
        amount_cents: 200_000,
      },
      {
        id: "mock-bud-transport-yearly",
        category_id: "mock-cat-transport",
        period: "yearly",
        amount_cents: 1_200_000,
      },
    ]);
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("list_budgets");
    expect(changed()).toBe(0);
  });
});

describe("预算写闭环", () => {
  it("create_budget 落内存并返回 id，list_budgets 出现新行，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_budget", {
      input: {
        category_id: "mock-cat-food-breakfast",
        period: "monthly",
        amount_cents: 100_000,
        start_date: "2026-01-15",
      },
    });
    expect(id).toBeTruthy();

    const budgets =
      await invoke<Array<{ id: string; category_id: string; period: string }>>("list_budgets");
    const created = budgets.find((b) => b.id === id);
    expect(created).toMatchObject({
      category_id: "mock-cat-food-breakfast",
      period: "monthly",
      amount_cents: 100_000,
    });
    expect(changed()).toBe(1);
  });

  it("create_budget 引用不存在的分类以存在性校验拒绝（spec #1964 取向）", async () => {
    await expect(
      invoke<string>("create_budget", {
        input: {
          category_id: "mock-cat-none",
          period: "monthly",
          amount_cents: 100,
          start_date: "2026-01-15",
        },
      }),
    ).rejects.toThrow(/mock-cat-none/);
  });

  it("update_budget 改金额与周期（period 缺省不改），list 反映，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("update_budget", {
      id: "mock-bud-food-monthly",
      input: { amount_cents: 3_000 },
    });
    await invoke<void>("update_budget", {
      id: "mock-bud-transport-yearly",
      input: { amount_cents: 600_000, period: "monthly" },
    });
    expect(changed()).toBe(2);

    const budgets =
      await invoke<Array<{ id: string; period: string; amount_cents: number; version: number }>>(
        "list_budgets",
      );
    const food = budgets.find((b) => b.id === "mock-bud-food-monthly");
    expect(food).toMatchObject({ period: "monthly", amount_cents: 3_000 });
    expect(food?.version).toBe(2); // 每次触碰 version 自增（种子 1 → 编辑后 2）
    expect(budgets.find((b) => b.id === "mock-bud-transport-yearly")).toMatchObject({
      period: "monthly",
      amount_cents: 600_000,
    });
  });

  it("update_budget 不存在的预算行报错", async () => {
    await expect(
      invoke<void>("update_budget", { id: "mock-bud-none", input: { amount_cents: 1 } }),
    ).rejects.toThrow(/mock-bud-none/);
  });

  it("delete_budget 软删：list 不再出现，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("delete_budget", { id: "mock-bud-food-monthly" });
    expect(changed()).toBe(1);

    const budgets = await invoke<Array<{ id: string }>>("list_budgets");
    expect(budgets.map((b) => b.id)).toEqual(["mock-bud-transport-yearly"]);
  });

  it("delete_budget 不存在的预算行报错", async () => {
    await expect(invoke<void>("delete_budget", { id: "mock-bud-none" })).rejects.toThrow(
      /mock-bud-none/,
    );
  });
});

describe("预算进度：按交易夹具直聚合（独立人算字面量）", () => {
  /** 人算基线（当月 = 2026-01、本年 = 2026，见文件头注）：
   * - food monthly：当月餐饮 expense 仅 mock-txn-001（2500，2026-01-05）；
   *   mock-txn-004（refund 1500）不冲减 → spent 2500，200_000 未超标。
   * - transport yearly：本年交通 expense 无交易 → spent 0，未超标。 */
  it("budget_progress 种子态两行：月度当月直和 / 年度本年直和，独立人算字面量", async () => {
    const rows = await invoke<
      Array<{
        budget: { id: string; period: string; amount_cents: number };
        category_name: string;
        spent_cents: number;
        over_budget: boolean;
      }>
    >("budget_progress");
    expect(
      rows.map((r) => ({
        id: r.budget.id,
        period: r.budget.period,
        amount: r.budget.amount_cents,
        name: r.category_name,
        spent: r.spent_cents,
        over: r.over_budget,
      })),
    ).toEqual([
      {
        id: "mock-bud-food-monthly",
        period: "monthly",
        amount: 200_000,
        name: "餐饮",
        spent: 2_500,
        over: false,
      },
      {
        id: "mock-bud-transport-yearly",
        period: "yearly",
        amount: 1_200_000,
        name: "交通",
        spent: 0,
        over: false,
      },
    ]);
  });

  it("月/年窗口各记各的账：3 月支出不进当月窗口、进本年窗口（周期可改后窗口随迁）", async () => {
    // 补一笔 3 月餐饮支出（不在当月 2026-01、在本年 2026）
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 50_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food",
        date: "2026-03-08",
      },
    });

    // 月度窗口：仅 1 月那笔 2500；3 月笔不在当月
    const monthlyRows =
      await invoke<Array<{ budget: { id: string }; spent_cents: number }>>("budget_progress");
    expect(monthlyRows.find((r) => r.budget.id === "mock-bud-food-monthly")?.spent_cents).toBe(
      2_500,
    );

    // 周期改年度后窗口 = 本年：2500 + 50000 = 52500（人算）
    await invoke<void>("update_budget", {
      id: "mock-bud-food-monthly",
      input: { amount_cents: 200_000, period: "yearly" },
    });
    const yearlyRows =
      await invoke<Array<{ budget: { id: string }; spent_cents: number }>>("budget_progress");
    expect(yearlyRows.find((r) => r.budget.id === "mock-bud-food-monthly")?.spent_cents).toBe(
      52_500,
    );
  });

  it("超标判定：金额改到当月支出之下后 over_budget 翻真", async () => {
    await invoke<void>("update_budget", {
      id: "mock-bud-food-monthly",
      input: { amount_cents: 2_000 }, // 当月 spent 2500（人算）> 2000
    });
    const rows =
      await invoke<Array<{ budget: { id: string }; spent_cents: number; over_budget: boolean }>>(
        "budget_progress",
      );
    expect(rows.find((r) => r.budget.id === "mock-bud-food-monthly")).toMatchObject({
      spent_cents: 2_500,
      over_budget: true,
    });
  });

  it("建预算 → 列表出现 → 进度变化：新分类预算按既有交易出进度行（票面闭环）", async () => {
    // 早餐（无预算分类）先记一笔当月支出
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 3_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food-breakfast",
        date: "2026-01-20",
      },
    });

    const id = await invoke<string>("create_budget", {
      input: {
        category_id: "mock-cat-food-breakfast",
        period: "monthly",
        amount_cents: 10_000,
        start_date: "2026-01-15",
      },
    });

    const budgets = await invoke<Array<{ id: string }>>("list_budgets");
    expect(budgets.map((b) => b.id)).toContain(id);

    const rows =
      await invoke<Array<{ budget: { id: string }; category_name: string; spent_cents: number }>>(
        "budget_progress",
      );
    expect(rows.find((r) => r.budget.id === id)).toMatchObject({
      category_name: "早餐",
      spent_cents: 3_000,
    });
  });

  it("软删预算不进进度（删除后进度行消失）", async () => {
    await invoke<void>("delete_budget", { id: "mock-bud-food-monthly" });
    const rows = await invoke<Array<{ budget: { id: string } }>>("budget_progress");
    expect(rows.map((r) => r.budget.id)).toEqual(["mock-bud-transport-yearly"]);
  });
});

describe("预算年度汇总：月预算 ×12 年化 + 本年已支（独立人算字面量）", () => {
  /** 人算基线：年度预算总额 = 200_000 × 12 + 1_200_000 = 3_600_000（分）；
   * 本年已支 = 预算覆盖分类 {餐饮, 交通} 的本年 expense 直和 = 2_500（仅
   * mock-txn-001）；未超标。 */
  it("budget_annual_summary 种子态：年化总额与覆盖分类本年已支的人算字面量", async () => {
    await expect(invoke("budget_annual_summary")).resolves.toEqual({
      annual_budget_total_cents: 3_600_000,
      spent_cents: 2_500,
      over_budget: false,
    });
  });

  it("未设预算分类的支出不进本年已支（覆盖分类边界，与真后端同边界）", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 100_000,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food-breakfast", // 早餐无预算行
        date: "2026-01-20",
      },
    });
    const summary = await invoke<{ spent_cents: number }>("budget_annual_summary");
    expect(summary.spent_cents).toBe(2_500); // 早餐支出不计入，已支不变
  });

  it("软删预算不进年化总额（月 ×12 部分保留）", async () => {
    await invoke<void>("delete_budget", { id: "mock-bud-transport-yearly" });
    const summary = await invoke<{ annual_budget_total_cents: number }>("budget_annual_summary");
    expect(summary.annual_budget_total_cents).toBe(2_400_000); // 200_000 × 12（人算）
  });

  it("超标判定：已支超出年化总额后 over_budget 翻真（人算 2_500 > 2_400）", async () => {
    await invoke<void>("delete_budget", { id: "mock-bud-transport-yearly" });
    await invoke<void>("update_budget", {
      id: "mock-bud-food-monthly",
      input: { amount_cents: 200 }, // 200 × 12 = 2_400 < 本年已支 2_500
    });
    await expect(invoke("budget_annual_summary")).resolves.toMatchObject({
      annual_budget_total_cents: 2_400,
      spent_cents: 2_500,
      over_budget: true,
    });
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("budget_progress");
    await invoke("budget_annual_summary");
    expect(changed()).toBe(0);
  });
});
