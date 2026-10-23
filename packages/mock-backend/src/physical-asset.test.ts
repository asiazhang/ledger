import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 实物资产域替身行为测试（issue #1975 / spec #1964 / ADR-0143）：接缝 = 应用
 * 所见的 tauri 命令面（`installMockBackend` + `invoke`），断言布线与行为一体——
 * 删除任一登记行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 期望值为独立人算字面量：夹具黄金首饰（mock-pa-gold，在持）两条估值历史
 * （2025-12-31 → 130_000、2026-01-10 → 132_000，当前 = 最新一条 132_000）、
 * 笔记本电脑（mock-pa-laptop，2025-11-20 已处置，最新估值 2025-11-01 → 42_000）。
 * 在持合计恒指在持行：132_000（已处置行不计、与筛选无关）。替身从简口径
 * （physical-asset.ts 头注）：不折算（夹具单币种 CNY 直记，native = 原币金额），
 * 缺省估值日期锚定替身「今日」2026-01-15（budget.ts 同款锚）。
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

describe("实物资产读：夹具投影", () => {
  it("list_physical_assets 缺省在持：当前估值 = 最新一条估值历史行", async () => {
    const list = await invoke<{
      assets: Array<{
        id: string;
        current_valuation_cents: number;
        current_valuation_date: string;
      }>;
      holding_total_native_cents: number;
      native_currency: string;
    }>("list_physical_assets", { status: null });

    expect(list.assets.map((a) => a.id)).toEqual(["mock-pa-gold"]);
    expect(list.assets[0]).toMatchObject({
      current_valuation_cents: 132_000,
      current_valuation_currency_code: "CNY",
      current_valuation_date: "2026-01-10",
      current_valuation_native_cents: 132_000,
    });
    expect(list.holding_total_native_cents).toBe(132_000);
    expect(list.native_currency).toBe("CNY");
  });

  it("status 过滤已处置：在持合计恒指在持（回看已处置合计不变）", async () => {
    const list = await invoke<{
      assets: Array<{
        id: string;
        status: string;
        disposal_date: string | null;
        current_valuation_native_cents: number | null;
      }>;
      holding_total_native_cents: number;
    }>("list_physical_assets", { status: "disposed" });

    expect(list.assets.map((a) => a.id)).toEqual(["mock-pa-laptop"]);
    expect(list.assets[0]).toMatchObject({
      status: "disposed",
      disposal_date: "2025-11-20",
      current_valuation_native_cents: null, // 已处置行 native 估值缺席
    });
    expect(list.holding_total_native_cents).toBe(132_000);
  });

  it("get_physical_asset 返回全字段实体；不存在或已删报错", async () => {
    const asset = await invoke<Record<string, unknown>>("get_physical_asset", {
      id: "mock-pa-laptop",
    });
    expect(asset).toMatchObject({
      id: "mock-pa-laptop",
      name: "笔记本电脑",
      purchase_price_cents: 150_000,
      status: "disposed",
      disposal_price_cents: 40_000,
      current_valuation_cents: 42_000,
      current_valuation_date: "2025-11-01",
    });

    await expect(invoke("get_physical_asset", { id: "mock-pa-none" })).rejects.toThrow(/实物资产/);
  });

  it("读命令不发 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke("list_physical_assets", { status: null });
    await invoke("get_physical_asset", { id: "mock-pa-gold" });
    expect(changed()).toBe(0);
  });
});

describe("实物资产写闭环", () => {
  it("create 建档即首条估值历史行：当前估值 = 初始估值，缺省日期锚今日；恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    const id = await invoke<string>("create_physical_asset", {
      input: {
        name: "浪琴手表",
        purchase_date: "2025-10-01",
        purchase_price_cents: 88_000,
        purchase_currency_code: "CNY",
        initial_valuation_cents: 90_000,
        initial_valuation_currency_code: "CNY",
        initial_valuation_date: null, // 缺省 = 替身今日 2026-01-15
      },
    });
    expect(id).toBeTruthy();
    expect(changed()).toBe(1);

    const asset = await invoke<Record<string, unknown>>("get_physical_asset", { id });
    expect(asset).toMatchObject({
      name: "浪琴手表",
      status: "holding",
      current_valuation_cents: 90_000,
      current_valuation_date: "2026-01-15",
      current_valuation_native_cents: 90_000,
    });

    // 在持合计随新资产前进：132_000 + 90_000 = 222_000（独立人算）
    const list = await invoke<{ holding_total_native_cents: number }>("list_physical_assets", {
      status: null,
    });
    expect(list.holding_total_native_cents).toBe(222_000);
  });

  it("create 缺初始估值金额报错、不落行", async () => {
    const changed = captureLedgerChanged();
    await expect(
      invoke("create_physical_asset", {
        input: { name: "无估值资产", initial_valuation_currency_code: "CNY" },
      }),
    ).rejects.toThrow(/initial_valuation_cents/);
    expect(changed()).toBe(0);
    const list = await invoke<{ assets: unknown[] }>("list_physical_assets", { status: null });
    expect(list.assets).toHaveLength(1); // 仍只有夹具在持行
  });

  it("update 仅改名称/购买信息：当前估值不动，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("update_physical_asset", {
      id: "mock-pa-gold",
      input: {
        name: "黄金首饰（结嫣）",
        purchase_date: "2025-06-20",
        purchase_price_cents: 125_000,
        purchase_currency_code: "CNY",
      },
    });
    expect(changed()).toBe(1);

    const asset = await invoke<Record<string, unknown>>("get_physical_asset", {
      id: "mock-pa-gold",
    });
    expect(asset).toMatchObject({
      name: "黄金首饰（结嫣）",
      purchase_date: "2025-06-20",
      purchase_price_cents: 125_000,
      current_valuation_cents: 132_000, // 估值不经编辑变更
      current_valuation_date: "2026-01-10",
    });
  });

  it("update_valuation 追加历史行：当前估值变最新一条，合计随动，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("update_physical_asset_valuation", {
      id: "mock-pa-gold",
      input: {
        amount_cents: 140_000,
        currency_code: "CNY",
        valuation_date: "2026-02-01",
      },
    });
    expect(changed()).toBe(1);

    const asset = await invoke<Record<string, unknown>>("get_physical_asset", {
      id: "mock-pa-gold",
    });
    expect(asset).toMatchObject({
      current_valuation_cents: 140_000,
      current_valuation_date: "2026-02-01",
      current_valuation_native_cents: 140_000,
    });
    const list = await invoke<{ holding_total_native_cents: number }>("list_physical_assets", {
      status: null,
    });
    expect(list.holding_total_native_cents).toBe(140_000);
  });

  it("估值同日追加：后插者胜（插入序 tie-break）", async () => {
    // 与夹具最新行同日 2026-01-10（132_000）追加 133_000：当前估值 = 后插一条
    await invoke<void>("update_physical_asset_valuation", {
      id: "mock-pa-gold",
      input: { amount_cents: 133_000, currency_code: "CNY", valuation_date: "2026-01-10" },
    });
    const asset = await invoke<Record<string, unknown>>("get_physical_asset", {
      id: "mock-pa-gold",
    });
    expect(asset).toMatchObject({
      current_valuation_cents: 133_000,
      current_valuation_date: "2026-01-10",
    });
  });

  it("dispose 转已处置：native 估值缺席、退出默认列表与在持合计，恰发一次信号", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("dispose_physical_asset", {
      id: "mock-pa-gold",
      input: {
        disposal_date: "2026-01-20",
        disposal_price_cents: 135_000,
        disposal_currency_code: "CNY",
      },
    });
    expect(changed()).toBe(1);

    const asset = await invoke<Record<string, unknown>>("get_physical_asset", {
      id: "mock-pa-gold",
    });
    expect(asset).toMatchObject({
      status: "disposed",
      disposal_date: "2026-01-20",
      disposal_price_cents: 135_000,
      current_valuation_cents: 132_000, // 估值历史原样保留
      current_valuation_native_cents: null, // 已处置行 native 投影缺席
    });

    const holding = await invoke<{
      assets: Array<{ id: string }>;
      holding_total_native_cents: number;
    }>("list_physical_assets", { status: null });
    expect(holding.assets).toHaveLength(0);
    expect(holding.holding_total_native_cents).toBe(0);

    const disposed = await invoke<{ assets: Array<{ id: string }> }>("list_physical_assets", {
      status: "disposed",
    });
    expect(disposed.assets.map((a) => a.id)).toEqual(["mock-pa-laptop", "mock-pa-gold"]);
  });

  it("dispose 缺处置日期报错", async () => {
    await expect(
      invoke("dispose_physical_asset", { id: "mock-pa-gold", input: {} }),
    ).rejects.toThrow(/disposal_date/);
  });

  it("delete 软删：退出全部列表与合计，估值历史保留（详情不可达）", async () => {
    const changed = captureLedgerChanged();
    await invoke<void>("delete_physical_asset", { id: "mock-pa-gold" });
    expect(changed()).toBe(1);

    const holding = await invoke<{ assets: unknown[]; holding_total_native_cents: number }>(
      "list_physical_assets",
      { status: null },
    );
    expect(holding.assets).toHaveLength(0);
    expect(holding.holding_total_native_cents).toBe(0);
    await expect(invoke("get_physical_asset", { id: "mock-pa-gold" })).rejects.toThrow(/实物资产/);
  });
});
