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

/**
 * 汇率与行情价格域替身行为测试（issue #1972 / spec #1964 / ADR-0143）：接缝 =
 * 应用所见的 tauri 命令面（`installMockBackend` + `invoke`）与事件面（`listen`），
 * 断言读写闭环（写改内存、读投影内存）与失效信号（汇率写入发 `ledger:changed`、
 * 价格类写入发 `ledger:prices-changed`），不探内表。替身刻意不追行为等价
 * （ADR-0143 决策 1）：同步汇率答确定性常量报告，不真取数、不发进度事件。
 *
 * 行情现价种子序（instrument_id 字典序，与真后端 ORDER BY instrument_id,
 * priced_at DESC 的首键一致）：mock-inst-custom(GOLD) / mock-inst-etf(159915) /
 * mock-inst-fund(000001) / mock-inst-hk(00700) / mock-inst-sh(600519)——
 * 019666（mock-inst-bond）无现价行，是 create 落新行的验收标的。
 *
 * GOLD 种子历史水位 2026-01-10（周六，ISO 周 = 2026-01-05 当周）：回填旧价
 * （早于 01-10）只沉淀历史、不动现价；报新价（不早于 01-10）现价随之更新
 * ——手动报价两落点的判定锚（最新点映像规则，ADR-0036）。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数（汇率写入的失效信号，票面判据）。 */
function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

/** 订阅 ledger:prices-changed 并返回触发计数（价格类写入的失效信号，ADR-0031）。 */
function capturePricesChanged(): () => number {
  let count = 0;
  void listen("ledger:prices-changed", () => {
    count++;
  });
  return () => count;
}

/** 订阅汇率同步进度事件并返回触发计数（替身不模拟进度，恒零断言用）。 */
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

    expect(changed()).toBe(1); // 汇率写入发 ledger:changed（票面判据）
    expect(prices()).toBe(0); // 不发价格信号
  });

  it("create 既有货币对 upsert：复用行 id、行数不增、语义列更新", async () => {
    const id = await invoke<string>("create_exchange_rate", {
      input: { base_code: "CNY", quote_code: "USD", rate: 0.15, priced_at: "2026-02-02" },
    });
    expect(id).toBe("mock-fx-cny-usd"); // 复用既有行 id（真后端按货币对 upsert）

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
    // manual_protected = 手动录入货币对数（种子 1 对 CNY/USD），与内存态一致
    expect(report.persist.manual_protected).toBe(1);

    expect(changed()).toBe(0);
    expect(prices()).toBe(0);
    expect(progress()).toBe(0); // 不发进度事件（票面取向）
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

    // 标的字典现价列同步（标的列表价格直投影，两表一致性）
    const instruments = await invoke<InstrumentListResult>("list_instruments", { filter: null });
    const bondInst = instruments.items.find((i) => i.id === "mock-inst-bond");
    expect(bondInst?.price_cents).toBe(100_050);

    expect(prices()).toBe(1); // 价格类写入发 prices-changed（票面判据）
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
    expect(id).toBe("mock-mp-sh"); // 复用既有现价行 id（per-instrument upsert）

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

    expect(prices()).toBe(1); // 实际写入即发（ADR-0031；历史落点必写 → 恒发）
    expect(changed()).toBe(0);
  });

  it("回填旧价：current_price_written=false，现价行不动，仍发一次 prices-changed", async () => {
    const prices = capturePricesChanged();

    const result = await quote("2026-01-08", 54_000); // 早于种子水位 2026-01-10
    expect(result).toEqual({ history_written: true, current_price_written: false });

    const gold = goldRow(await priceRows());
    expect(gold).toMatchObject({ price_cents: 55_000, priced_at: "2026-01-10" }); // 现价不动

    expect(prices()).toBe(1); // 历史落点实际写入即发（任一落点写入即广播）
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
    expect(prices()).toBe(0); // 失败路径零写入不广播（ADR-0031）
  });
});
