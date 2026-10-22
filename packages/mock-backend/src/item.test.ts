import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 物品域替身行为测试（issue #1974 / spec #1964 / ADR-0143）：接缝 = 应用所见
 * 的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 日均成本三元组期望值为独立人算字面量（口径见 item.ts 头注：分母 = 购买日 →
 * 目标日的日历天数**含起止两端**；分子 = 总成本 − 残值下限 0；每天成本 =
 * 分子 ÷ 天数）。替身「今日」锚定 2026-01-15（MOCK_TODAY，确定性纪律）。
 *
 * 夹具两行的人算过程：
 * - 人体工学椅（mock-item-chair，已处置）：购买 2025-09-24 → 处置 2026-01-01，
 *   9 月余 6 天 + 10 月 31 + 11 月 30 + 12 月 31 + 1 月 1 = 99 天差，含两端
 *   = 100 天；分子 = 200_000 − 100_000 = 100_000；每天 = 100_000 ÷ 100 = 1000。
 * - 智能手机（mock-item-phone，在用）：购买 2026-01-06 → 替身今日 2026-01-15
 *   = 9 天差，含两端 = 10 天；无残值分子 = 599_900；每天 = 59_990。
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

/** 物品行的观察面：仅取断言关心的字段。 */
interface ItemRow {
  id: string;
  name: string;
  status: string;
  purchase_date: string;
  disposal_date: string | null;
  residual_value_cents: number | null;
  used_days: number;
  numerator_cents: number;
  per_day_cents: number;
}

async function itemOf(id: string): Promise<ItemRow> {
  const rows = await invoke<ItemRow[]>("list_items");
  const row = rows.find((r) => r.id === id);
  if (!row) throw new Error(`测试辅助：物品行缺席 ${id}`);
  return row;
}

describe("物品列表读：夹具投影与日均成本三元组", () => {
  it("已处置物品摊到处置日（含起止两端）：100 天、分子扣残值、每天 1000 分", async () => {
    const row = await itemOf("mock-item-chair");
    expect(row).toMatchObject({
      name: "人体工学椅",
      status: "disposed",
      purchase_date: "2025-09-24",
      disposal_date: "2026-01-01",
      residual_value_cents: 100_000,
      used_days: 100, // 2025-09-24 → 2026-01-01 含两端（独立人算）
      numerator_cents: 100_000, // 200_000 − 100_000
      per_day_cents: 1000.0, // 100_000 ÷ 100
    });
  });

  it("在用物品摊到替身今日（2026-01-15）：10 天、无残值分子即总成本", async () => {
    const row = await itemOf("mock-item-phone");
    expect(row).toMatchObject({
      name: "智能手机",
      status: "in_use",
      purchase_date: "2026-01-06",
      used_days: 10, // 2026-01-06 → 2026-01-15 含两端
      numerator_cents: 599_900,
      per_day_cents: 59_990.0, // 599_900 ÷ 10
    });
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("list_items");
    await invoke("calculate_item_cost", { id: "mock-item-phone" });
    expect(changed()).toBe(0);
  });
});

describe("calculate_item_cost：自选参考日重算", () => {
  it("缺省沿用列表口径：在用 → 替身今日，与列表行同值", async () => {
    const cost = await invoke<ItemRow>("calculate_item_cost", { id: "mock-item-phone" });
    expect(cost).toEqual({ used_days: 10, numerator_cents: 599_900, per_day_cents: 59_990.0 });
  });

  it("缺省沿用列表口径：已处置 → 处置日（不随今日漂移）", async () => {
    const cost = await invoke<ItemRow>("calculate_item_cost", { id: "mock-item-chair" });
    expect(cost).toEqual({ used_days: 100, numerator_cents: 100_000, per_day_cents: 1000.0 });
  });

  it("自选参考日覆盖目标日（含未来日预览）：2026-01-25 → 20 天、每天 29_995", async () => {
    const cost = await invoke<ItemRow>("calculate_item_cost", {
      id: "mock-item-phone",
      referenceDate: "2026-01-25",
    });
    // 2026-01-06 → 2026-01-25 = 19 天差，含两端 20 天；599_900 ÷ 20 = 29_995
    expect(cost).toEqual({ used_days: 20, numerator_cents: 599_900, per_day_cents: 29_995.0 });
  });

  it("参考日早于购买日报错，不静默回绕", async () => {
    await expect(
      invoke("calculate_item_cost", { id: "mock-item-phone", referenceDate: "2026-01-05" }),
    ).rejects.toThrow(/早于购买日期/);
  });
});

describe("物品写闭环：建档 → 列表出现 → 处置 → 三元组重锚", () => {
  it("create_item 落内存（关联购买交易校验在用），列表出现，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_item", {
      input: {
        name: "电吹风",
        purchase_date: "2025-12-01",
        total_cost_cents: 25_000,
        currency_code: "CNY",
        purchase_transaction_id: "mock-txn-001",
      },
    });
    expect(id).toBeTruthy();
    expect(changed()).toBe(1);

    const row = await itemOf(id);
    expect(row).toMatchObject({
      name: "电吹风",
      status: "in_use",
      purchase_date: "2025-12-01",
      // 2025-12-01 → 2026-01-15 = 45 天差，含两端 46 天；25_000 ÷ 46 ≈ 543.48
      used_days: 46,
      numerator_cents: 25_000,
    });
    expect(row.per_day_cents).toBeCloseTo(25_000 / 46, 9);
  });

  it("未来购买日的在用物品列表不翻车：目标日取购买日当天起算 1 天（#1977 过页发现的错位）", async () => {
    // 场景重放：mock 模式下新记交易日期 = 浏览器真实今天，可晚于替身时钟锚
    // 2026-01-15；修复前 list_items 对在用物品传 MOCK_TODAY 整表抛错，
    // store 重拉静默失败、新物品永不出现。
    const id = await invoke<string>("create_item", {
      input: {
        name: "未来买入",
        purchase_date: "2026-10-23",
        total_cost_cents: 10_000,
        currency_code: "CNY",
        purchase_transaction_id: null,
      },
    });
    const rows = await invoke<ItemRow[]>("list_items");
    const row = rows.find((r) => r.id === id);
    expect(row).toMatchObject({
      status: "in_use",
      purchase_date: "2026-10-23",
      // 购买当天起算：同日 = 1 天，成本全额
      used_days: 1,
      numerator_cents: 10_000,
    });
  });

  it("create_item 关联不存在交易拒绝：不落行、不发信号", async () => {
    const changed = captureLedgerChanged();
    await expect(
      invoke("create_item", {
        input: {
          name: "无源物品",
          purchase_date: "2026-01-01",
          total_cost_cents: 1_000,
          currency_code: "CNY",
          purchase_transaction_id: "mock-txn-404",
        },
      }),
    ).rejects.toThrow(/关联交易不存在/);
    expect(changed()).toBe(0);
    const rows = await invoke<ItemRow[]>("list_items");
    expect(rows).toHaveLength(2);
  });

  it("update_item 全量替换编辑面字段，缺省关联保留既有指针，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("update_item", {
      id: "mock-item-phone",
      input: {
        name: "智能手机 Pro",
        purchase_date: "2026-01-01",
        total_cost_cents: 500_000,
        currency_code: "CNY",
        note: "改过",
      },
    });
    expect(changed()).toBe(1);

    const row = await itemOf("mock-item-phone");
    expect(row).toMatchObject({
      name: "智能手机 Pro",
      purchase_date: "2026-01-01",
      // 2026-01-01 → 2026-01-15 = 14 天差，含两端 15 天；500_000 ÷ 15
      used_days: 15,
      numerator_cents: 500_000,
    });
    expect(row.per_day_cents).toBeCloseTo(500_000 / 15, 9);
  });

  it("dispose_item 置 disposed + 处置日 + 残值，三元组重锚到处置日，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("dispose_item", {
      id: "mock-item-phone",
      input: { disposal_date: "2026-01-10", residual_value_cents: 99_900 },
    });
    expect(changed()).toBe(1);

    const row = await itemOf("mock-item-phone");
    expect(row).toMatchObject({
      status: "disposed",
      disposal_date: "2026-01-10",
      residual_value_cents: 99_900,
      // 2026-01-06 → 2026-01-10 = 4 天差，含两端 5 天；分子 599_900 − 99_900 = 500_000
      used_days: 5,
      numerator_cents: 500_000,
    });
    expect(row.per_day_cents).toBe(100_000.0); // 500_000 ÷ 5
  });

  it("残值 ≥ 总成本：分子下限 0，每天成本 0（不输出负成本）", async () => {
    await invoke<void>("dispose_item", {
      id: "mock-item-phone",
      input: { disposal_date: "2026-01-10", residual_value_cents: 599_900 },
    });
    const row = await itemOf("mock-item-phone");
    expect(row).toMatchObject({ numerator_cents: 0, used_days: 5, per_day_cents: 0.0 });
  });

  it("再次处置 = 修正处置信息（改日期与残值），版本递增仍一次信号", async () => {
    await invoke<void>("dispose_item", {
      id: "mock-item-phone",
      input: { disposal_date: "2026-01-10", residual_value_cents: 99_900 },
    });
    const changed = captureLedgerChanged();
    await invoke<void>("dispose_item", {
      id: "mock-item-phone",
      input: { disposal_date: "2026-01-12", residual_value_cents: 199_900 },
    });
    expect(changed()).toBe(1);

    const row = await itemOf("mock-item-phone");
    expect(row).toMatchObject({ disposal_date: "2026-01-12", residual_value_cents: 199_900 });
    // 2026-01-06 → 2026-01-12 含两端 7 天；599_900 − 199_900 = 400_000 ÷ 7
    expect(row.used_days).toBe(7);
    expect(row.per_day_cents).toBeCloseTo(400_000 / 7, 9);
  });

  it("处置日期早于购买日 / 晚于替身今日均拒绝，不落状态不发信号", async () => {
    const changed = captureLedgerChanged();
    await expect(
      invoke("dispose_item", {
        id: "mock-item-phone",
        input: { disposal_date: "2026-01-05" }, // 早于购买 2026-01-06
      }),
    ).rejects.toThrow(/早于购买日期/);
    await expect(
      invoke("dispose_item", {
        id: "mock-item-phone",
        input: { disposal_date: "2026-01-16" }, // 晚于替身今日 2026-01-15
      }),
    ).rejects.toThrow(/不能晚于今天/);
    expect(changed()).toBe(0);
    expect((await itemOf("mock-item-phone")).status).toBe("in_use");
  });

  it("delete_item 软删后列表不再返回，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("delete_item", { id: "mock-item-phone" });
    expect(changed()).toBe(1);

    const rows = await invoke<ItemRow[]>("list_items");
    expect(rows.map((r) => r.id)).toEqual(["mock-item-chair"]);
    await expect(
      invoke("list_items").then((rs: ItemRow[]) => rs.find((r) => r.id === "mock-item-phone")),
    ).resolves.toBeUndefined();
  });
});
