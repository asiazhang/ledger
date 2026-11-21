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

interface ProgressRow {
  goal: { id: string; name: string; status: string; account_id: string };
  saved_cents: number;
  remaining_cents: number;
  achieved: boolean;
  currency_code: string;
  pace_monthly_cents: number | null;
  pace_source: string | null;
  eta_months: number | null;
  eta_month: string | null;
  required_monthly_cents: number | null;
  pace_delta_cents: number | null;
}

async function progressOf(goalId: string): Promise<ProgressRow> {
  const rows = await invoke<ProgressRow[]>("savings_goal_progress");
  const row = rows.find((r) => r.goal.id === goalId);
  if (!row) throw new Error(`测试辅助：进度行缺席 ${goalId}`);
  return row;
}

describe("蓄水进度读：夹具投影与双向推算", () => {
  it("已存 = 专属账户余额直和；无截止正推 ETA（独立人算字面量）", async () => {
    const row = await progressOf("mock-goal-emergency");
    expect(row).toMatchObject({
      saved_cents: 300_000,
      remaining_cents: 700_000,
      achieved: false,
      currency_code: "CNY",
      pace_monthly_cents: 100_000,
      pace_source: "manual",
      eta_months: 7,
      eta_month: "2026-08",
      required_monthly_cents: null,
      pace_delta_cents: null,
    });
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("savings_goal_progress");
    expect(changed()).toBe(0);
  });

  it("ETA 上取整：节奏 300_000 → ceil(700000/300000) = 3 个月、2026-04", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "应急基金",
        target_amount_cents: 1_000_000,
        deadline: null,
        planned_monthly_cents: 300_000,
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.eta_months).toBe(3);
    expect(row.eta_month).toBe("2026-04");
  });

  it("有截止反推：所需月存上取整 + 落后差值；有截止不给 ETA", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "应急基金",
        target_amount_cents: 1_000_000,
        deadline: "2026-04-10",
        planned_monthly_cents: 100_000,
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.eta_months).toBeNull();
    expect(row.eta_month).toBeNull();
    expect(row.required_monthly_cents).toBe(233_334);
    expect(row.pace_delta_cents).toBe(-133_334);
  });

  it("截止落在当月：按 1 个整月计", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "应急基金",
        target_amount_cents: 1_000_000,
        deadline: "2026-01-31",
        planned_monthly_cents: 100_000,
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.required_monthly_cents).toBe(700_000);
    expect(row.pace_delta_cents).toBe(-600_000);
  });

  it("截止日已过：不虚构月存，反推字段缺席", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "应急基金",
        target_amount_cents: 1_000_000,
        deadline: "2025-12-31",
        planned_monthly_cents: 100_000,
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.required_monthly_cents).toBeNull();
    expect(row.pace_delta_cents).toBeNull();
  });

  it("节奏为零（未手填）：推算字段整体缺席，不虚构时点", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "应急基金",
        target_amount_cents: 1_000_000,
        deadline: null,
        planned_monthly_cents: null,
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.pace_monthly_cents).toBeNull();
    expect(row.pace_source).toBeNull();
    expect(row.eta_months).toBeNull();
    expect(row.eta_month).toBeNull();
  });

  it("达成态：还差 ≤ 0、达成为真；节奏仍随行携带、时点推算缺席", async () => {
    await invoke<string>("create_transaction", {
      input: {
        kind: "transfer",
        amount_cents: 700_000,
        currency_code: "CNY",
        account_id: "mock-acc-bank",
        to_account_id: "mock-acc-goal-emergency",
        date: "2026-02-01",
      },
    });
    const row = await progressOf("mock-goal-emergency");
    expect(row.saved_cents).toBe(1_000_000);
    expect(row.remaining_cents).toBe(0);
    expect(row.achieved).toBe(true);
    expect(row.pace_monthly_cents).toBe(100_000);
    expect(row.pace_source).toBe("manual");
    expect(row.eta_months).toBeNull();
    expect(row.required_monthly_cents).toBeNull();
  });
});

describe("目标写闭环：建目标 → 专属账户 → 蓄水 → 进度变化", () => {
  it("create 同事务自动建专属账户（other 类型、1:1 绑定、账户名随动），恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_savings_goal", {
      input: { name: "换车基金", target_amount_cents: 600_000, deadline: "2026-12-31" },
    });
    expect(id).toBeTruthy();
    expect(changed()).toBe(1);

    const accounts =
      await invoke<Array<{ id: string; name: string; type: string; currency_code: string }>>(
        "list_accounts",
      );
    const dedicated = accounts.find((a) => a.name === "换车基金");
    expect(dedicated).toMatchObject({ type: "other", currency_code: "CNY" });

    const row = await progressOf(id);
    expect(row.goal.account_id).toBe(dedicated?.id);
    expect(row.saved_cents).toBe(0);
    expect(row.remaining_cents).toBe(600_000);
    expect(row.required_monthly_cents).toBe(54_546);
  });

  it("蓄水（往专属账户转账）→ 进度变化；目标额改小即时达成", async () => {
    const id = await invoke<string>("create_savings_goal", {
      input: { name: "旅行基金", target_amount_cents: 500_000 },
    });
    const before = await progressOf(id);
    expect(before.saved_cents).toBe(0);

    await invoke<string>("create_transaction", {
      input: {
        kind: "transfer",
        amount_cents: 250_000,
        currency_code: "CNY",
        account_id: "mock-acc-bank",
        to_account_id: before.goal.account_id,
        date: "2026-02-02",
      },
    });
    const after = await progressOf(id);
    expect(after.saved_cents).toBe(250_000);
    expect(after.remaining_cents).toBe(250_000);

    await invoke<void>("update_savings_goal", {
      id,
      input: {
        name: "旅行基金",
        target_amount_cents: 250_000,
        deadline: null,
        planned_monthly_cents: null,
      },
    });
    const achievedRow = await progressOf(id);
    expect(achievedRow.achieved).toBe(true);
    expect(achievedRow.remaining_cents).toBe(0);
  });

  it("update 改名联动专属账户名；其余字段全量替换", async () => {
    await invoke<void>("update_savings_goal", {
      id: "mock-goal-emergency",
      input: {
        name: "安全垫",
        target_amount_cents: 800_000,
        deadline: "2026-06-30",
        planned_monthly_cents: 150_000,
      },
    });
    const accounts = await invoke<Array<{ id: string; name: string }>>("list_accounts");
    expect(accounts.find((a) => a.id === "mock-acc-goal-emergency")?.name).toBe("安全垫");

    const row = await progressOf("mock-goal-emergency");
    expect(row.goal).toMatchObject({ name: "安全垫", target_amount_cents: 800_000 });
    expect(row).toMatchObject({
      saved_cents: 300_000,
      remaining_cents: 500_000,
      pace_monthly_cents: 150_000,
    });
  });

  it("archive / unarchive 状态二值单点翻转，数据原样保留，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("archive_savings_goal", { id: "mock-goal-emergency" });
    expect(changed()).toBe(1);

    let row = await progressOf("mock-goal-emergency");
    expect(row.goal.status).toBe("archived");
    expect(row.goal).toMatchObject({
      name: "应急基金",
      target_amount_cents: 1_000_000,
      planned_monthly_cents: 100_000,
    });

    await invoke<void>("unarchive_savings_goal", { id: "mock-goal-emergency" });
    row = await progressOf("mock-goal-emergency");
    expect(row.goal.status).toBe("active");
  });

  it("delete 余额非零拒绝：目标与账户原样保留", async () => {
    const changed = captureLedgerChanged();
    await expect(invoke("delete_savings_goal", { id: "mock-goal-emergency" })).rejects.toThrow(
      /余额非零/,
    );
    expect(changed()).toBe(0);

    const row = await progressOf("mock-goal-emergency");
    expect(row.saved_cents).toBe(300_000);
    const accounts = await invoke<Array<{ id: string }>>("list_accounts");
    expect(accounts.find((a) => a.id === "mock-acc-goal-emergency")).toBeTruthy();
  });

  it("delete 余额为零：目标软删 + 级联软删专属账户，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_savings_goal", {
      input: { name: "零余额目标", target_amount_cents: 100_000 },
    });
    const row = await progressOf(id);
    const accountId = row.goal.account_id;

    await invoke<void>("delete_savings_goal", { id });
    expect(changed()).toBe(2);

    const rows = await invoke<ProgressRow[]>("savings_goal_progress");
    expect(rows.find((r) => r.goal.id === id)).toBeUndefined();
    const accounts = await invoke<Array<{ id: string }>>("list_accounts");
    expect(accounts.find((a) => a.id === accountId)).toBeUndefined();
  });

  it("create 缺目标额报错、不落行不发信号", async () => {
    const changed = captureLedgerChanged();
    await expect(invoke("create_savings_goal", { input: { name: "无额目标" } })).rejects.toThrow(
      /target_amount_cents/,
    );
    expect(changed()).toBe(0);
  });
});
