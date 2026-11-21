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

describe("报表日期极值：report_date_range", () => {
  it("返回未删交易日期极值对（人算：2025-03-01 保费最小、02-25 分红最大）", async () => {
    await expect(invoke("report_date_range")).resolves.toEqual({
      min_date: "2025-03-01",
      max_date: "2026-02-25",
    });
  });

  it("极值随删除收窄：软删 02-25 的分红后 max 落到 02-20", async () => {
    await invoke("delete_transaction", { id: "mock-txn-105" });
    await expect(invoke("report_date_range")).resolves.toEqual({
      min_date: "2025-03-01",
      max_date: "2026-02-20",
    });
  });
});

describe("月度汇总：monthly_summary（毛值三列，人算字面量）", () => {
  it("期间口径：当月一行 income 1_200_000 / expense 毛值 376_000 / refund 1_500", async () => {
    await expect(
      invoke("monthly_summary", { year: 2026, from: "2026-01-01", to: "2026-01-31" }),
    ).resolves.toEqual([
      { month: "2026-01", income_cents: 1_200_000, expense_cents: 376_000, refund_cents: 1_500 },
    ]);
  });

  it("遗留 year 形态：2026 年按月两行（2 月仅分红 8_500）；2025 年定时 + 保单流水七行；无交易年空数组", async () => {
    await expect(invoke("monthly_summary", { year: 2026, from: null, to: null })).resolves.toEqual([
      { month: "2026-01", income_cents: 1_200_000, expense_cents: 376_000, refund_cents: 1_500 },
      { month: "2026-02", income_cents: 8_500, expense_cents: 0, refund_cents: 0 },
    ]);
    await expect(invoke("monthly_summary", { year: 2025, from: null, to: null })).resolves.toEqual([
      { month: "2025-03", income_cents: 0, expense_cents: 120_000, refund_cents: 0 },
      { month: "2025-06", income_cents: 0, expense_cents: 600_000, refund_cents: 0 },
      { month: "2025-07", income_cents: 0, expense_cents: 360_000, refund_cents: 0 },
      { month: "2025-09", income_cents: 0, expense_cents: 2_500, refund_cents: 0 },
      { month: "2025-10", income_cents: 0, expense_cents: 2_500, refund_cents: 0 },
      { month: "2025-11", income_cents: 500_000, expense_cents: 2_500, refund_cents: 0 },
      { month: "2025-12", income_cents: 0, expense_cents: 3_500, refund_cents: 0 },
    ]);
    await expect(invoke("monthly_summary", { year: 2024, from: null, to: null })).resolves.toEqual(
      [],
    );
  });

  it("期间边界含两端：from=to=2026-01-08 只聚 005/006（expense 10_000、无收支退款）", async () => {
    await expect(
      invoke("monthly_summary", { year: 2026, from: "2026-01-08", to: "2026-01-08" }),
    ).resolves.toEqual([
      { month: "2026-01", income_cents: 0, expense_cents: 10_000, refund_cents: 0 },
    ]);
  });

  it("跨月期间按月分布成多行：2 月补一笔后两行各记各的账", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 6_600,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-transport",
        date: "2026-02-03",
      },
    });
    await expect(
      invoke("monthly_summary", { year: 2026, from: "2026-01-01", to: "2026-02-28" }),
    ).resolves.toEqual([
      { month: "2026-01", income_cents: 1_200_000, expense_cents: 376_000, refund_cents: 1_500 },
      { month: "2026-02", income_cents: 8_500, expense_cents: 6_600, refund_cents: 0 },
    ]);
  });
});

describe("分类份额：category_shares（净值口径，人算字面量）", () => {
  it("expense 全月：未分类 373_500（005/006 + 定时历史两笔 + 204 保费）居首、餐饮 1_000（2500 − 退款 1500）次之", async () => {
    await expect(
      invoke("category_shares", {
        kind: "expense",
        month: null,
        year: null,
        from: "2026-01-01",
        to: "2026-01-31",
      }),
    ).resolves.toEqual([
      { category_id: "", category_name: "未分类", amount_cents: 373_500 },
      { category_id: "mock-cat-food", category_name: "餐饮", amount_cents: 1_000 },
    ]);
  });

  it("income 全月：工资 1_200_000 单行", async () => {
    await expect(
      invoke("category_shares", {
        kind: "income",
        month: null,
        year: null,
        from: "2026-01-01",
        to: "2026-01-31",
      }),
    ).resolves.toEqual([
      { category_id: "mock-cat-salary", category_name: "工资", amount_cents: 1_200_000 },
    ]);
  });

  it("期间边界决定退款冲减是否参与：到 01-05 截止时餐饮无退款可冲（净值 2_500）", async () => {
    await expect(
      invoke("category_shares", {
        kind: "expense",
        month: null,
        year: null,
        from: "2026-01-01",
        to: "2026-01-05",
      }),
    ).resolves.toEqual([
      { category_id: "", category_name: "未分类", amount_cents: 360_000 },
      { category_id: "mock-cat-food", category_name: "餐饮", amount_cents: 2_500 },
    ]);
  });

  it("子类直归本类不模拟上卷：早餐子类支出单独成行、不并入餐饮", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 4_400,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food-breakfast",
        date: "2026-01-12",
      },
    });
    const rows = await invoke<
      Array<{ category_id: string; category_name: string; amount_cents: number }>
    >("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: "2026-01-01",
      to: "2026-01-31",
    });
    expect(rows).toEqual([
      { category_id: "", category_name: "未分类", amount_cents: 373_500 },
      { category_id: "mock-cat-food-breakfast", category_name: "早餐", amount_cents: 4_400 },
      { category_id: "mock-cat-food", category_name: "餐饮", amount_cents: 1_000 },
    ]);
  });

  it("遗留 month+year 形态：2025-01 无交易空数组", async () => {
    await expect(
      invoke("category_shares", {
        kind: "expense",
        month: "2025-01",
        year: 2025,
        from: null,
        to: null,
      }),
    ).resolves.toEqual([]);
  });
});

describe("商户消费排行：merchant_shares（expense_net + 同口径笔数 + total，人算字面量）", () => {
  it("全月：京东 13_500/4 笔居首（含定时历史两笔）、美团 1_000（2500−1500）/2 笔，total 14_500", async () => {
    await expect(
      invoke("merchant_shares", { year: 2026, from: "2026-01-01", to: "2026-01-31", topN: null }),
    ).resolves.toEqual({
      rows: [
        {
          merchant_id: "mock-mch-jd",
          merchant_name: "京东",
          amount_cents: 13_500,
          transaction_count: 4,
        },
        {
          merchant_id: "mock-mch-meituan",
          merchant_name: "美团",
          amount_cents: 1_000,
          transaction_count: 2,
        },
      ],
      total_cents: 14_500,
    });
  });

  it("topN 键名契约（camelCase）：topN=1 截断到首行，total_cents 仍为全量合计 14_500", async () => {
    await expect(
      invoke("merchant_shares", { year: 2026, from: "2026-01-01", to: "2026-01-31", topN: 1 }),
    ).resolves.toEqual({
      rows: [
        {
          merchant_id: "mock-mch-jd",
          merchant_name: "京东",
          amount_cents: 13_500,
          transaction_count: 4,
        },
      ],
      total_cents: 14_500,
    });
  });

  it("期间边界含两端：from=to=2026-01-08 只有京东两笔（美团 01-05/01-06 出界）", async () => {
    await expect(
      invoke("merchant_shares", { year: 2026, from: "2026-01-08", to: "2026-01-08", topN: null }),
    ).resolves.toEqual({
      rows: [
        {
          merchant_id: "mock-mch-jd",
          merchant_name: "京东",
          amount_cents: 10_000,
          transaction_count: 2,
        },
      ],
      total_cents: 10_000,
    });
  });

  it("软删商户历史引用照常统计（名字典含软删行）", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "expense",
        amount_cents: 3_300,
        currency_code: "CNY",
        account_id: "mock-acc-cash",
        merchant_id: "mock-mch-del",
        date: "2026-01-20",
      },
    });
    const report = await invoke<{
      rows: Array<{ merchant_id: string; amount_cents: number; transaction_count: number }>;
      total_cents: number;
    }>("merchant_shares", { year: 2026, from: "2026-01-01", to: "2026-01-31", topN: null });
    expect(report.rows.find((r) => r.merchant_id === "mock-mch-del")).toEqual({
      merchant_id: "mock-mch-del",
      merchant_name: "已删商户",
      amount_cents: 3_300,
      transaction_count: 1,
    });
    expect(report.total_cents).toBe(17_800);
  });

  it("遗留 year 形态：2025 年仅定时历史京东五行", async () => {
    await expect(
      invoke("merchant_shares", { year: 2025, from: null, to: null, topN: null }),
    ).resolves.toEqual({
      rows: [
        {
          merchant_id: "mock-mch-jd",
          merchant_name: "京东",
          amount_cents: 11_000,
          transaction_count: 5,
        },
      ],
      total_cents: 11_000,
    });
  });
});

describe("报表域读命令不发 ledger:changed", () => {
  it("四条聚合命令全部零信号", async () => {
    const changed = captureLedgerChanged();
    await invoke("report_date_range");
    await invoke("monthly_summary", { year: 2026, from: null, to: null });
    await invoke("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: null,
      to: null,
    });
    await invoke("merchant_shares", { year: 2026, from: null, to: null, topN: null });
    expect(changed()).toBe(0);
  });
});
