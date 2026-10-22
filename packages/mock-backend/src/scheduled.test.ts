import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 定时/订阅域替身行为测试（issue #1973 / spec #1964 / ADR-0143）：接缝 = 应用
 * 所见的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除 index.ts 任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087）。
 *
 * 期望值为独立人算字面量（输入全字面量化，见 scheduled.ts 头注的从简口径）。
 * 夹具（fixtures.ts）：分期 12 期（1_000/期、2 completed）、订阅·商户月付
 * 2_500（5 completed + 1 pending）、订阅·保单年缴 600_000（1 completed）、
 * 定时转账月度 500_000（1 completed + 1 pending）。替身「今日」锚 2026-01-15
 * （MOCK_TODAY）——订阅花费窗口 = 2025-02..2026-01（12 个日历月，旧→新）：
 *
 * - 本月（2026-01）实际 = 视频会员 2_500；本年（2026）实际 = 2_500（年缴
 *   600_000 落 2025-06，不进本年）。
 * - 逐月序列：2025-06 = 600_000、2025-09..2026-01 各 2_500、其余月 0。
 * - 折算月成本（只算 active）= 2_500 × 1 + round(600_000 ÷ 12) = 52_500；
 *   折算年成本 = 52_500 × 12 = 630_000。
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

interface WithExtRow {
  core: { id: string; kind: string; status: string; start_date: string };
  merchant_id: string | null;
  policy_id: string | null;
  total_amount_cents: number | null;
  total_occurrences: number | null;
  to_account_id: string | null;
}

async function listPlans(): Promise<WithExtRow[]> {
  return invoke<WithExtRow[]>("list_scheduled_transactions");
}

describe("定时计划读：列表与详情投影", () => {
  it("list：四条夹具计划 created_at 倒序 + 形态扩展投影（独立人算字面量）", async () => {
    const rows = await listPlans();
    expect(rows.map((r) => r.core.id)).toEqual([
      "mock-sch-installment", // created_at 2025-12-01，倒序最先
      "mock-sch-transfer", // 2025-11-25
      "mock-sch-subscription", // 2025-08-01
      "mock-sch-policy-sub", // 2025-05-20
    ]);
    const inst = rows.find((r) => r.core.id === "mock-sch-installment");
    expect(inst).toMatchObject({
      merchant_id: "mock-mch-jd",
      policy_id: null,
      total_amount_cents: 12_000,
      total_occurrences: 12,
      to_account_id: null,
    });
    const sub = rows.find((r) => r.core.id === "mock-sch-subscription");
    expect(sub).toMatchObject({
      merchant_id: "mock-mch-jd",
      policy_id: null,
      total_amount_cents: null,
      total_occurrences: null,
      to_account_id: null,
    });
    const policySub = rows.find((r) => r.core.id === "mock-sch-policy-sub");
    expect(policySub).toMatchObject({
      merchant_id: null, // 保单协议不挂商户（ADR-0082 决策 2）
      policy_id: "mock-pol-health",
    });
    const transfer = rows.find((r) => r.core.id === "mock-sch-transfer");
    expect(transfer).toMatchObject({
      merchant_id: null,
      to_account_id: "mock-acc-invest",
      total_occurrences: null,
    });
  });

  it("detail：核心 + 扩展 + 全量期次升序 + 已完成计数与金额合计", async () => {
    const detail = await invoke<{
      core: { id: string; amount_cents: number };
      extension: {
        merchant_id: string | null;
        total_amount_cents: number;
        total_occurrences: number;
      };
      pending_occurrences: { scheduled_date: string }[];
      completed_occurrences: number;
      completed_amount_cents: number;
      occurrences: {
        id: string;
        scheduled_date: string;
        status: string;
        transaction_id: string | null;
      }[];
    }>("get_scheduled_transaction_detail", { id: "mock-sch-installment" });

    expect(detail.core.amount_cents).toBe(1_000);
    expect(detail.extension).toEqual({
      scheduled_transaction_id: "mock-sch-installment",
      merchant_id: "mock-mch-jd",
      total_amount_cents: 12_000,
      total_occurrences: 12,
    });
    expect(detail.completed_occurrences).toBe(2); // 夹具前 2 期已完成
    expect(detail.completed_amount_cents).toBe(2_000); // 1_000 × 2
    expect(detail.occurrences).toHaveLength(12);
    expect(detail.occurrences.map((o) => o.scheduled_date)).toEqual([
      "2025-12-15",
      "2026-01-15",
      "2026-02-15",
      "2026-03-15",
      "2026-04-15",
      "2026-05-15",
      "2026-06-15",
      "2026-07-15",
      "2026-08-15",
      "2026-09-15",
      "2026-10-15",
      "2026-11-15",
    ]);
    expect(detail.pending_occurrences).toHaveLength(10);
    expect(detail.occurrences[0]).toMatchObject({
      status: "completed",
      transaction_id: "mock-txn-201",
    });
  });

  it("detail：订阅扩展行投影商户与保单引用", async () => {
    const detail = await invoke<{ extension: Record<string, unknown> }>(
      "get_scheduled_transaction_detail",
      { id: "mock-sch-policy-sub" },
    );
    expect(detail.extension).toEqual({
      scheduled_transaction_id: "mock-sch-policy-sub",
      merchant_id: null,
      policy_id: "mock-pol-health",
    });
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await listPlans();
    await invoke("get_scheduled_transaction_detail", { id: "mock-sch-transfer" });
    await invoke("subscription_spend_overview");
    expect(changed()).toBe(0);
  });
});

describe("订阅花费总览：实际花费聚合与折算推算（独立人算字面量）", () => {
  it("本月/本年/12 个月序列/逐订阅行/折算月年成本", async () => {
    const overview = await invoke<{
      native_currency: string;
      this_month_native_cents: number;
      this_year_native_cents: number;
      months: { month: string; native_cents: number }[];
      rows: {
        plan_id: string;
        note: string | null;
        merchant_name: string | null;
        status: string;
        amount_cents: number;
        currency_code: string;
        this_month_native_cents: number;
        this_year_native_cents: number;
      }[];
      projected_month_native_cents: number;
      projected_year_native_cents: number;
    }>("subscription_spend_overview");

    expect(overview.native_currency).toBe("CNY");
    expect(overview.this_month_native_cents).toBe(2_500); // 视频会员 2026-01 一笔
    expect(overview.this_year_native_cents).toBe(2_500); // 年缴 600_000 落 2025-06
    // 12 个月窗口 2025-02..2026-01（旧→新，无扣款月补 0）
    expect(overview.months).toEqual([
      { month: "2025-02", native_cents: 0 },
      { month: "2025-03", native_cents: 0 },
      { month: "2025-04", native_cents: 0 },
      { month: "2025-05", native_cents: 0 },
      { month: "2025-06", native_cents: 600_000 }, // 年缴保费
      { month: "2025-07", native_cents: 0 },
      { month: "2025-08", native_cents: 0 },
      { month: "2025-09", native_cents: 2_500 },
      { month: "2025-10", native_cents: 2_500 },
      { month: "2025-11", native_cents: 2_500 },
      { month: "2025-12", native_cents: 2_500 },
      { month: "2026-01", native_cents: 2_500 },
    ]);
    // 逐订阅行（created_at 倒序）只含订阅形态：商户版在前
    expect(overview.rows.map((r) => r.plan_id)).toEqual([
      "mock-sch-subscription",
      "mock-sch-policy-sub",
    ]);
    expect(overview.rows[0]).toMatchObject({
      note: "视频会员",
      merchant_name: "京东",
      status: "active",
      amount_cents: 2_500,
      currency_code: "CNY",
      this_month_native_cents: 2_500,
      this_year_native_cents: 2_500,
    });
    expect(overview.rows[1]).toMatchObject({
      note: "重疾险年缴",
      merchant_name: null,
      this_month_native_cents: 0, // 年缴落 2025-06
      this_year_native_cents: 0,
    });
    // 折算推算（只算 active）：2_500 × 1 + 600_000 ÷ 12 = 52_500；年 = × 12
    expect(overview.projected_month_native_cents).toBe(52_500);
    expect(overview.projected_year_native_cents).toBe(630_000);
  });
});

describe("建计划 → 期次出现 → 手动执行 → 流水出现（主闭环）", () => {
  it("create 展开首段期次、execute 真落交易并回填（恰发一次信号）", async () => {
    const changed = captureLedgerChanged();

    const planId = await invoke<string>("create_scheduled_transaction", {
      input: {
        kind: "subscription",
        account_id: "mock-acc-ewallet",
        amount_cents: 3_000,
        currency_code: "CNY",
        recurrence_type: "monthly",
        recurrence_interval: 1,
        recurrence_day: 10,
        start_date: "2026-02-10",
        note: "云存储",
        merchant_id: "mock-mch-jd",
      },
    });
    expect(changed()).toBe(1); // 计划写入恰发一次

    // 月度窗口展开：12 期 2026-02-10..2027-01-10，全 pending
    const created = await invoke<{
      occurrences: { id: string; scheduled_date: string; status: string }[];
    }>("get_scheduled_transaction_detail", { id: planId });
    expect(created.occurrences.map((o) => o.scheduled_date)).toEqual([
      "2026-02-10",
      "2026-03-10",
      "2026-04-10",
      "2026-05-10",
      "2026-06-10",
      "2026-07-10",
      "2026-08-10",
      "2026-09-10",
      "2026-10-10",
      "2026-11-10",
      "2026-12-10",
      "2027-01-10",
    ]);
    expect(created.occurrences.every((o) => o.status === "pending")).toBe(true);

    // 手动执行第一期：真落一笔 expense 流水（复制计划商户/备注）
    const occId = created.occurrences[0]?.id;
    if (!occId) throw new Error("测试辅助：首期缺席");
    const txnId = await invoke<string>("execute_scheduled_occurrence", {
      input: { occurrence_id: occId },
    });
    expect(changed()).toBe(2); // 执行恰发一次

    const txns = await invoke<{ items: { id: string }[] }>("list_transactions");
    const landed = txns.items.find((t) => t.id === txnId);
    expect(landed).toMatchObject({
      kind: "expense",
      amount_cents: 3_000,
      account_id: "mock-acc-ewallet",
      merchant_id: "mock-mch-jd",
      note: "云存储",
      date: "2026-02-10",
    });

    // 期次置 completed 并回填 transaction_id
    const after = await invoke<{
      occurrences: { id: string; status: string; transaction_id: string | null }[];
      completed_occurrences: number;
      completed_amount_cents: number;
    }>("get_scheduled_transaction_detail", { id: planId });
    const executed = after.occurrences.find((o) => o.id === occId);
    expect(executed).toMatchObject({ status: "completed", transaction_id: txnId });
    expect(after.completed_occurrences).toBe(1);
    expect(after.completed_amount_cents).toBe(3_000);
  });

  it("定时转账期次执行落 transfer 流水（两账户、无商户）", async () => {
    const txnId = await invoke<string>("execute_scheduled_occurrence", {
      input: { occurrence_id: "mock-sch-occ-trf-2" }, // 2026-01-05 pending
    });
    const txns = await invoke<{ items: { id: string }[] }>("list_transactions");
    expect(txns.items.find((t) => t.id === txnId)).toMatchObject({
      kind: "transfer",
      amount_cents: 500_000,
      account_id: "mock-acc-bank",
      to_account_id: "mock-acc-invest",
      merchant_id: null,
      date: "2026-01-05",
    });
  });

  it("分期建计划金额分配：整除基准 + 末期尾差（独立人算字面量）", async () => {
    const planId = await invoke<string>("create_scheduled_transaction", {
      input: {
        kind: "installment",
        account_id: "mock-acc-credit",
        amount_cents: 1_041,
        currency_code: "CNY",
        recurrence_type: "monthly",
        recurrence_interval: 1,
        recurrence_day: 20,
        start_date: "2026-02-20",
        note: "相机分期",
        merchant_id: "mock-mch-jd",
        total_amount_cents: 12_500,
        total_occurrences: 12,
      },
    });
    const detail = await invoke<{ occurrences: { amount_cents: number }[] }>(
      "get_scheduled_transaction_detail",
      { id: planId },
    );
    // 12_500 ÷ 12 = 1_041 余 8：前 11 期 1_041、末期 1_041 + 8 = 1_049
    expect(detail.occurrences.map((o) => o.amount_cents)).toEqual([
      1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_041, 1_049,
    ]);
  });

  it("月度 recurrence_day 锚定与月末钳制（31 号在 2 月钳到 28）", async () => {
    const planId = await invoke<string>("create_scheduled_transaction", {
      input: {
        kind: "subscription",
        account_id: "mock-acc-cash",
        amount_cents: 1_000,
        currency_code: "CNY",
        recurrence_type: "monthly",
        recurrence_interval: 1,
        recurrence_day: 31,
        start_date: "2026-01-31",
      },
    });
    const detail = await invoke<{ occurrences: { scheduled_date: string }[] }>(
      "get_scheduled_transaction_detail",
      { id: planId },
    );
    // 2026 半年：1月31、2月28（平年钳制）、3月31、4月30、5月31、6月30
    expect(detail.occurrences.slice(0, 6).map((o) => o.scheduled_date)).toEqual([
      "2026-01-31",
      "2026-02-28",
      "2026-03-31",
      "2026-04-30",
      "2026-05-31",
      "2026-06-30",
    ]);
  });

  it("分期全部期次执行完 → 计划自动置 completed", async () => {
    const before = await invoke<{ occurrences: { id: string; status: string }[] }>(
      "get_scheduled_transaction_detail",
      { id: "mock-sch-installment" },
    );
    for (const occ of before.occurrences.filter((o) => o.status === "pending")) {
      await invoke("execute_scheduled_occurrence", { input: { occurrence_id: occ.id } });
    }
    const rows = await listPlans();
    expect(rows.find((r) => r.core.id === "mock-sch-installment")?.core.status).toBe("completed");
  });

  it("非 pending 期次与暂停计划的期次执行被拒（不发信号）", async () => {
    const changed = captureLedgerChanged();
    // 已完成期次再执行被拒
    await expect(
      invoke("execute_scheduled_occurrence", {
        input: { occurrence_id: "mock-sch-occ-sub-1" }, // completed
      }),
    ).rejects.toThrow(/期次状态/);

    // 暂停计划后执行被拒
    await invoke("update_scheduled_transaction_status", {
      input: { id: "mock-sch-subscription", new_status: "paused" },
    });
    const before = changed(); // 暂停发过一次
    await expect(
      invoke("execute_scheduled_occurrence", {
        input: { occurrence_id: "mock-sch-occ-sub-6" }, // pending 但计划已暂停
      }),
    ).rejects.toThrow(/活跃/);
    expect(changed()).toBe(before); // 拒绝路径零信号
  });
});

describe("计划状态与订阅编辑", () => {
  it("取消：pending 期次级联 cancelled、恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke("update_scheduled_transaction_status", {
      input: { id: "mock-sch-subscription", new_status: "cancelled" },
    });
    expect(changed()).toBe(1);

    const detail = await invoke<{
      core: { status: string };
      occurrences: { status: string }[];
      pending_occurrences: unknown[];
    }>("get_scheduled_transaction_detail", { id: "mock-sch-subscription" });
    expect(detail.core.status).toBe("cancelled");
    // 5 期 completed 保留、pending 1 期级联 cancelled
    expect(detail.occurrences.filter((o) => o.status === "cancelled")).toHaveLength(1);
    expect(detail.occurrences.filter((o) => o.status === "completed")).toHaveLength(5);
    expect(detail.pending_occurrences).toHaveLength(0);
  });

  it("闭集外状态值被拒", async () => {
    await expect(
      invoke("update_scheduled_transaction_status", {
        input: { id: "mock-sch-transfer", new_status: "bogus" },
      }),
    ).rejects.toThrow(/状态/);
  });

  it("订阅编辑非金额字段全量替换，恰发一次信号；金额不变", async () => {
    const changed = captureLedgerChanged();
    await invoke("update_scheduled_subscription", {
      input: {
        id: "mock-sch-subscription",
        account_id: "mock-acc-cash",
        category_id: "mock-cat-food",
        note: "云存储改道",
        merchant_id: "mock-mch-meituan",
      },
    });
    expect(changed()).toBe(1);

    const rows = await listPlans();
    const row = rows.find((r) => r.core.id === "mock-sch-subscription");
    expect(row?.core.account_id).toBe("mock-acc-cash");
    expect(row?.core.category_id).toBe("mock-cat-food");
    expect(row?.core.note).toBe("云存储改道");
    expect(row?.merchant_id).toBe("mock-mch-meituan");
    expect(row?.core.amount_cents).toBe(2_500); // 金额不在编辑面
  });

  it("携带 amount_cents / total_amount_cents 哨兵被拒（含显式 null，不发信号）", async () => {
    const changed = captureLedgerChanged();
    // ADR-0023 决策三：请求体 key 一旦出现即拒绝（含 null）
    await expect(
      invoke("update_scheduled_subscription", {
        input: {
          id: "mock-sch-subscription",
          account_id: "mock-acc-ewallet",
          amount_cents: 9_999,
        },
      }),
    ).rejects.toThrow(/订阅金额不可编辑/);

    await expect(
      invoke("update_scheduled_subscription", {
        input: {
          id: "mock-sch-subscription",
          account_id: "mock-acc-ewallet",
          total_amount_cents: null, // 显式 null 同样拒绝（哨兵看 key 不看值）
        },
      }),
    ).rejects.toThrow(/订阅金额不可编辑/);

    expect(changed()).toBe(0); // 拒绝路径零信号、零写入
    const rows = await listPlans();
    expect(rows.find((r) => r.core.id === "mock-sch-subscription")?.core.note).toBe("视频会员");
  });
});

describe("期次展开：按计划推进后续序列", () => {
  it("订阅从最后一期下一期起补 12 期（月度 day 10 锚定）", async () => {
    const changed = captureLedgerChanged();
    const ids = await invoke<string[]>("expand_scheduled_occurrences", {
      id: "mock-sch-subscription",
    });
    expect(ids).toHaveLength(12);
    expect(changed()).toBe(1);

    const detail = await invoke<{ occurrences: { scheduled_date: string; status: string }[] }>(
      "get_scheduled_transaction_detail",
      { id: "mock-sch-subscription" },
    );
    // 夹具最后一期 2026-02-10 → 从 2026-03-10 起补 12 期
    expect(detail.occurrences.slice(-12).map((o) => o.scheduled_date)).toEqual([
      "2026-03-10",
      "2026-04-10",
      "2026-05-10",
      "2026-06-10",
      "2026-07-10",
      "2026-08-10",
      "2026-09-10",
      "2026-10-10",
      "2026-11-10",
      "2026-12-10",
      "2027-01-10",
      "2027-02-10",
    ]);
    expect(detail.occurrences.slice(-12).every((o) => o.status === "pending")).toBe(true);
  });

  it("分期已展开满额 → 空结果零信号；非 active 计划 → 空", async () => {
    const changed = captureLedgerChanged();
    const none = await invoke<string[]>("expand_scheduled_occurrences", {
      id: "mock-sch-installment", // 12/12 已满
    });
    expect(none).toEqual([]);
    expect(changed()).toBe(0);

    await invoke("update_scheduled_transaction_status", {
      input: { id: "mock-sch-transfer", new_status: "paused" },
    });
    expect(changed()).toBe(1);
    const paused = await invoke<string[]>("expand_scheduled_occurrences", {
      id: "mock-sch-transfer",
    });
    expect(paused).toEqual([]);
    expect(changed()).toBe(1); // 无新行零信号
  });
});
