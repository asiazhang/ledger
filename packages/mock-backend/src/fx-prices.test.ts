import { beforeEach, describe, expect, it } from "vitest";
import type {
  ExchangeRate,
  ExchangeRateSyncReport,
  InstrumentListResult,
  ManualPriceResult,
  MarketPrice,
} from "@ledger/types";
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

function capturePricesChanged(): () => number {
  let count = 0;
  void listen("ledger:prices-changed", () => {
    count++;
  });
  return () => count;
}

function captureFxSyncProgress(): () => number {
  let count = 0;
  void listen("ledger:fx-sync-progress", () => {
    count++;
  });
  return () => count;
}

async function rateRows(): Promise<ExchangeRate[]> {
  return invoke<ExchangeRate[]>("list_exchange_rates");
}

async function priceRows(): Promise<MarketPrice[]> {
  return invoke<MarketPrice[]>("list_market_prices");
}

function goldRow(rows: MarketPrice[]): MarketPrice {
  const row = rows.find((r) => r.instrument_id === "mock-inst-custom");
  if (!row) throw new Error("GOLD 现价行缺失");
  return row;
}

describe("list_exchange_rates / create_exchange_rate：汇率读写闭环", () => {
  it("list 返回种子货币对，按 base_code, quote_code 排序", async () => {
    const rows = await rateRows();
    expect(rows.map((r) => `${r.base_code}/${r.quote_code}`)).toEqual(["CNY/USD", "USD/CNY"]);
    expect(rows[0]).toMatchObject({ rate: 0.1389, source: "manual" });
    expect(rows[1]).toMatchObject({ rate: 7.2, source: "ecb" });
  });

  it("create 新货币对落内存：写后列表读回语义列，恰发一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    const prices = capturePricesChanged();

    const id = await invoke<string>("create_exchange_rate", {
      input: {
        base_code: "EUR",
        quote_code: "CNY",
        rate: 7.85,
        priced_at: "2026-02-01",
        source: "manual",
      },
    });
    expect(typeof id).toBe("string");

    const rows = await rateRows();
    const added = rows.find((r) => r.base_code === "EUR" && r.quote_code === "CNY");
    expect(added).toMatchObject({ rate: 7.85, priced_at: "2026-02-01", source: "manual" });

    expect(changed()).toBe(1);
    expect(prices()).toBe(0);
  });

  it("create 既有货币对 upsert：复用行 id、行数不增、语义列更新", async () => {
    const id = await invoke<string>("create_exchange_rate", {
      input: { base_code: "CNY", quote_code: "USD", rate: 0.15, priced_at: "2026-02-02" },
    });
    expect(id).toBe("mock-fx-cny-usd");

    const rows = await rateRows();
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ rate: 0.15, priced_at: "2026-02-02" });
  });

  it("rate 非正数报错且不发信号", async () => {
    const changed = captureLedgerChanged();
    await expect(
      invoke("create_exchange_rate", {
        input: { base_code: "EUR", quote_code: "CNY", rate: 0, priced_at: "2026-02-01" },
      }),
    ).rejects.toThrow("汇率必须大于 0");
    expect(changed()).toBe(0);
  });
});

describe("sync_exchange_rates：同步报告常量态", () => {
  it("答确定性报告（覆盖区间与条数、manual_protected），不发任何事件", async () => {
    const changed = captureLedgerChanged();
    const prices = capturePricesChanged();
    const progress = captureFxSyncProgress();

    const report = await invoke<ExchangeRateSyncReport>("sync_exchange_rates");
    expect(report.full_backfilled).toBe(false);
    expect(report.persist.pairs).toBeGreaterThan(0);
    expect(report.persist.points).toBeGreaterThan(0);
    expect(report.persist.earliest).not.toBeNull();
    expect(report.persist.latest).not.toBeNull();
    expect(report.persist.manual_protected).toBe(1);

    expect(changed()).toBe(0);
    expect(prices()).toBe(0);
    expect(progress()).toBe(0);
  });
});

describe("list_market_prices / create_market_price：现价缓存读写", () => {
  it("list 返回种子现价缓存行，按 instrument_id, priced_at 排序", async () => {
    const rows = await priceRows();
    expect(rows.map((r) => r.instrument_id)).toEqual([
      "mock-inst-custom",
      "mock-inst-etf",
      "mock-inst-fund",
      "mock-inst-hk",
      "mock-inst-sh",
    ]);
    expect(rows[0]).toMatchObject({
      price_cents: 55_000,
      currency_code: "CNY",
      priced_at: "2026-01-10",
      source: "manual",
    });
  });

  it("create 对无现价标的落新行：列表读回、标的价格列同步、恰发一次 prices-changed", async () => {
    const changed = captureLedgerChanged();
    const prices = capturePricesChanged();

    const id = await invoke<string>("create_market_price", {
      input: {
        instrument_id: "mock-inst-bond",
        price_cents: 100_050,
        currency_code: "CNY",
        priced_at: "2026-02-01",
      },
    });
    expect(typeof id).toBe("string");

    const rows = await priceRows();
    expect(rows).toHaveLength(6);
    const bond = rows.find((r) => r.instrument_id === "mock-inst-bond");
    expect(bond).toMatchObject({ price_cents: 100_050, priced_at: "2026-02-01" });

    const instruments = await invoke<InstrumentListResult>("list_instruments", { filter: null });
    const bondInst = instruments.items.find((i) => i.id === "mock-inst-bond");
    expect(bondInst?.price_cents).toBe(100_050);

    expect(prices()).toBe(1);
    expect(changed()).toBe(0);
  });

  it("create 既有行 upsert：复用行 id、行数不增、价格前进", async () => {
    const id = await invoke<string>("create_market_price", {
      input: {
        instrument_id: "mock-inst-sh",
        price_cents: 1_500_000_000,
        currency_code: "CNY",
        priced_at: "2026-02-03",
      },
    });
    expect(id).toBe("mock-mp-sh");

    const rows = await priceRows();
    expect(rows).toHaveLength(5);
    expect(rows.find((r) => r.instrument_id === "mock-inst-sh")).toMatchObject({
      price_cents: 1_500_000_000,
      priced_at: "2026-02-03",
    });
  });

  it("price 非正数 / 标的不存在报错", async () => {
    await expect(
      invoke("create_market_price", {
        input: {
          instrument_id: "mock-inst-bond",
          price_cents: 0,
          currency_code: "CNY",
          priced_at: "2026-02-01",
        },
      }),
    ).rejects.toThrow("价格必须大于 0");
    await expect(
      invoke("create_market_price", {
        input: {
          instrument_id: "mock-inst-none",
          price_cents: 100,
          currency_code: "CNY",
          priced_at: "2026-02-01",
        },
      }),
    ).rejects.toThrow("标的不存在或已删除");
  });
});

describe("record_manual_price：手动报价两落点写入", () => {
  const quote = (date: string, priceCents: number) =>
    invoke<ManualPriceResult>("record_manual_price", {
      input: { instrument_id: "mock-inst-custom", date, price_cents: priceCents },
    });

  it("报新价：两落点均写、现价行与标的价格列同步、恰发一次 prices-changed", async () => {
    const changed = captureLedgerChanged();
    const prices = capturePricesChanged();

    const result = await quote("2026-01-20", 60_000);
    expect(result).toEqual({ history_written: true, current_price_written: true });

    const gold = goldRow(await priceRows());
    expect(gold).toMatchObject({
      price_cents: 60_000,
      priced_at: "2026-01-20",
      source: "manual",
    });

    const instruments = await invoke<InstrumentListResult>("list_instruments", { filter: null });
    const goldInst = instruments.items.find((i) => i.id === "mock-inst-custom");
    expect(goldInst?.price_cents).toBe(60_000);

    expect(prices()).toBe(1);
    expect(changed()).toBe(0);
  });

  it("回填旧价：current_price_written=false，现价行不动，仍发一次 prices-changed", async () => {
    const prices = capturePricesChanged();

    const result = await quote("2026-01-08", 54_000);
    expect(result).toEqual({ history_written: true, current_price_written: false });

    const gold = goldRow(await priceRows());
    expect(gold).toMatchObject({ price_cents: 55_000, priced_at: "2026-01-10" });

    expect(prices()).toBe(1);
  });

  it("回填后再报更新价：current_price_written 恢复 true（历史水位维护）", async () => {
    await quote("2026-01-08", 54_000);
    const result = await quote("2026-01-20", 61_000);
    expect(result.current_price_written).toBe(true);

    const gold = goldRow(await priceRows());
    expect(gold.price_cents).toBe(61_000);
  });

  it("首价标的（无历史）：current_price_written=true", async () => {
    const result = await invoke<ManualPriceResult>("record_manual_price", {
      input: { instrument_id: "mock-inst-bond", date: "2026-01-05", price_cents: 99_500 },
    });
    expect(result).toEqual({ history_written: true, current_price_written: true });

    const rows = await priceRows();
    expect(rows.find((r) => r.instrument_id === "mock-inst-bond")).toMatchObject({
      price_cents: 99_500,
      priced_at: "2026-01-05",
    });
  });

  it("price 非正数 / 日期非法 / 标的不存在报错且不发信号", async () => {
    const prices = capturePricesChanged();
    await expect(quote("2026-01-20", 0)).rejects.toThrow("价格必须大于 0");
    await expect(quote("2026/01/20", 10_000)).rejects.toThrow("日期格式须为 YYYY-MM-DD");
    await expect(
      invoke("record_manual_price", {
        input: { instrument_id: "mock-inst-none", date: "2026-01-20", price_cents: 10_000 },
      }),
    ).rejects.toThrow("标的不存在或已删除");
    expect(prices()).toBe(0);
  });
});
