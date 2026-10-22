import { beforeEach, describe, expect, it } from "vitest";
import type { AddFundResult, AddStockInstrumentResult, InstrumentListResult } from "@ledger/types";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 投资标的域替身行为测试（issue #1969 / spec #1964 / ADR-0143）：接缝 = 应用
 * 所见的 tauri 命令面（`installMockBackend` + `invoke`）与事件面（`listen`），
 * 断言筛选/分页/anchor 定位的契约形状与写闭环（写改内存、读投影内存、写后
 * 必发 ledger:changed），不探内表。替身刻意不追行为等价（ADR-0143 决策 1）：
 * 按代码添加的「行情源回显」为按代码确定性推导，不联网。
 *
 * 夹具种子序（symbol 排序，与真后端 ORDER BY symbol 一致）：
 * 000001 基金 unknown / 00700 港股 hk / 019666 债券 sh / 159915 ETF sz /
 * 600519 沪股 sh / GOLD 自建 other unknown——下列筛选与分页断言按此序人算。
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

async function symbolsOf(filter: unknown): Promise<string[]> {
  const result = await invoke<InstrumentListResult>("list_instruments", { filter });
  return result.items.map((i) => i.symbol);
}

describe("list_instruments：夹具投影与四维筛选", () => {
  it("无过滤返回全部种子行，按 symbol 排序", async () => {
    const result = await invoke<InstrumentListResult>("list_instruments", { filter: null });
    expect(result.items.map((i) => i.symbol)).toEqual([
      "000001",
      "00700",
      "019666",
      "159915",
      "600519",
      "GOLD",
    ]);
    expect(result.total).toBe(6);
    expect(result.anchor_page).toBeUndefined();
  });

  it("search 对 symbol / name 子串命中（大小写不敏感）", async () => {
    await expect(symbolsOf({ search: "茅台" })).resolves.toEqual(["600519"]);
    await expect(symbolsOf({ search: "etf" })).resolves.toEqual(["159915"]); // 名称「创业板ETF」
    await expect(symbolsOf({ search: "007" })).resolves.toEqual(["00700"]);
    await expect(symbolsOf({ search: "不存在" })).resolves.toEqual([]);
  });

  it("market 精确 / type / only_invested 三维筛选", async () => {
    await expect(symbolsOf({ market: "sh" })).resolves.toEqual(["019666", "600519"]);
    await expect(symbolsOf({ type: "stock" })).resolves.toEqual(["00700", "600519"]);
    // invested 口径自 issue #1971 起与 fixtureHoldings 行集一致：茅台在库未持有，
    // 其余五标的有持仓行（期初存量 / 夹具交易建仓）
    await expect(symbolsOf({ only_invested: true })).resolves.toEqual([
      "000001",
      "00700",
      "019666",
      "159915",
      "GOLD",
    ]);
    // 四维叠加：sh + stock → 仅贵州茅台
    await expect(symbolsOf({ market: "sh", type: "stock" })).resolves.toEqual(["600519"]);
  });
});

describe("list_instruments：服务端分页与 anchor 定位", () => {
  it("page / page_size 服务端切片，total 恒为过滤全集", async () => {
    await expect(symbolsOf({ page: 1, page_size: 2 })).resolves.toEqual(["000001", "00700"]);
    await expect(symbolsOf({ page: 2, page_size: 2 })).resolves.toEqual(["019666", "159915"]);
    await expect(symbolsOf({ page: 3, page_size: 2 })).resolves.toEqual(["600519", "GOLD"]);
    await expect(symbolsOf({ page: 4, page_size: 2 })).resolves.toEqual([]);

    const result = await invoke<InstrumentListResult>("list_instruments", {
      filter: { page: 2, page_size: 2 },
    });
    expect(result.total).toBe(6);
  });

  it("anchor_id 命中时返回 anchor_page 且行必在返回页（issue #1907 定位接缝）", async () => {
    const result = await invoke<InstrumentListResult>("list_instruments", {
      filter: { page: 1, page_size: 2, anchor_id: "mock-inst-etf" },
    });
    // 159915 排序序号为 3（0 起），page_size=2 → 第 2 页
    expect(result.anchor_page).toBe(2);
    expect(result.items.map((i) => i.symbol)).toEqual(["019666", "159915"]);
  });

  it("anchor_id 不在结果集（被筛掉 / 不存在）时不带 anchor_page，按普通页码查询", async () => {
    const filtered = await invoke<InstrumentListResult>("list_instruments", {
      filter: { market: "sz", anchor_id: "mock-inst-sh" }, // 被市场筛藏
    });
    expect(filtered.anchor_page).toBeUndefined();
    expect(filtered.items.map((i) => i.symbol)).toEqual(["159915"]);

    const missing = await invoke<InstrumentListResult>("list_instruments", {
      filter: { anchor_id: "no-such-id" },
    });
    expect(missing.anchor_page).toBeUndefined();
    expect(missing.total).toBe(6);
  });
});

describe("create_instrument：自建标的写闭环", () => {
  it("自建标的落内存，列表出现，发恰一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();

    const id = await invoke<string>("create_instrument", {
      input: {
        symbol: "SILVER",
        type: "other",
        name: "家庭白银存折",
        currency_code: "CNY",
      },
    });
    expect(id).toMatch(/^mock-inst-/);
    expect(changed()).toBe(1);

    const symbols = await symbolsOf({ search: "SILVER" });
    expect(symbols).toEqual(["SILVER"]);
    const result = await invoke<InstrumentListResult>("list_instruments", {
      filter: { search: "SILVER" },
    });
    expect(result.items[0]).toMatchObject({
      id,
      type: "other",
      name: "家庭白银存折",
      source: "manual", // 非同步即手动，来源随行终身不变（ADR-0036 决策 2）
      market: "unknown", // 缺省归一 unknown
      price_cents: null, // 手动建档无现价
      invested: false,
    });
  });

  it("同（symbol, type）自然键命中时复用既有行并只更新名称", async () => {
    const id = await invoke<string>("create_instrument", {
      input: { symbol: "GOLD", type: "other", name: "家庭黄金存折·改名", currency_code: "CNY" },
    });
    expect(id).toBe("mock-inst-custom"); // 复用夹具行，不新建

    const result = await invoke<InstrumentListResult>("list_instruments", {
      filter: { search: "GOLD" },
    });
    expect(result.total).toBe(1);
    expect(result.items[0].name).toBe("家庭黄金存折·改名");
  });

  it("类型白名单与必填守卫：股票/基金手动创建拒，名称/代码空拒（ADR-0036 决策 3）", async () => {
    await expect(
      invoke("create_instrument", {
        input: { symbol: "600000", type: "stock", name: "浦发银行", currency_code: "CNY" },
      }),
    ).rejects.toThrow(/股票/);
    await expect(
      invoke("create_instrument", {
        input: { symbol: "110022", type: "fund", name: "易方达消费", currency_code: "CNY" },
      }),
    ).rejects.toThrow(/基金/);
    await expect(
      invoke("create_instrument", {
        input: { symbol: "BOND2", type: "bond", name: "  ", currency_code: "CNY" },
      }),
    ).rejects.toThrow(/名称/);
    await expect(
      invoke("create_instrument", {
        input: { symbol: "  ", type: "bond", name: "新国债", currency_code: "CNY" },
      }),
    ).rejects.toThrow(/代码/);
  });
});

describe("delete_instrument：仅手动来源可删", () => {
  it("手动行删除后列表消失，发恰一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();

    await invoke<void>("delete_instrument", { id: "mock-inst-custom" });
    expect(changed()).toBe(1);

    await expect(symbolsOf({ search: "GOLD" })).resolves.toEqual([]);
    const result = await invoke<InstrumentListResult>("list_instruments", { filter: null });
    expect(result.total).toBe(5);
  });

  it("同步来源行拒删（字典修正由按代码查询/创建承担，ADR-0081）", async () => {
    await expect(invoke<void>("delete_instrument", { id: "mock-inst-sh" })).rejects.toThrow(
      /同步来源/,
    );
    await expect(symbolsOf({ search: "600519" })).resolves.toEqual(["600519"]);
  });

  it("不存在的 id 报错", async () => {
    await expect(invoke<void>("delete_instrument", { id: "no-such-id" })).rejects.toThrow(
      /不存在或已删除/,
    );
  });
});

describe("add_fund_by_code：按代码即拉场外基金（确定性回显，不联网）", () => {
  it("回显按代码推导的权威名称与净值占位，落库后列表可查，发恰一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();

    // 确定性推导：name = `替身基金 ${code}`；nav = code % 9000 + 1000（万分之一元）
    // 110022 % 9000 = 2022 → 3022 → 0.3022 元；nav_date 恒字面量
    const result = await invoke<AddFundResult>("add_fund_by_code", { code: "110022" });
    expect(result).toEqual({
      instrument_id: result.instrument_id,
      symbol: "110022",
      name: "替身基金 110022",
      fund_class: "",
      nav_cents: 3022,
      nav_date: "2026-01-31",
      price_written: true,
    });
    expect(result.instrument_id).toMatch(/^mock-inst-/);
    expect(changed()).toBe(1);

    const list = await invoke<InstrumentListResult>("list_instruments", {
      filter: { search: "110022" },
    });
    expect(list.items[0]).toMatchObject({
      id: result.instrument_id,
      type: "fund",
      market: "unknown", // 场外无交易所市场，恒 unknown（ADR-0038）
      currency_code: "CNY",
      price_cents: 3022, // 净值即现价占位
      price_channel: "fund_nav",
      source: "manual", // 建档来源标记（adopt 语义，非同步即手动）
    });
  });

  it("同代码重复拉取幂等：复用同一标的行 id", async () => {
    const first = await invoke<AddFundResult>("add_fund_by_code", { code: "110022" });
    const second = await invoke<AddFundResult>("add_fund_by_code", { code: "110022" });
    expect(second.instrument_id).toBe(first.instrument_id);

    await expect(symbolsOf({ type: "fund" })).resolves.toEqual(["000001", "110022"]);
  });

  it("非 6 位纯数字代码拒绝（格式校验即刻拒，不发网络请求）", async () => {
    await expect(invoke("add_fund_by_code", { code: "12345" })).rejects.toThrow(/6 位数字/);
    await expect(invoke("add_fund_by_code", { code: "abc123" })).rejects.toThrow(/6 位数字/);
  });
});

describe("add_instrument_by_code：场内通道按代码建档 + 类型自动识别回显", () => {
  it("sh 通道：普通 6 位代码识别为股票，回显名称/现价（确定性推导）", async () => {
    // 确定性推导：price = (ΣcharCode % 900 + 100) × 1000；
    // "600036": 54+48+48+48+51+54 = 303 → 403000 → 40.3 元
    const result = await invoke<AddStockInstrumentResult>("add_instrument_by_code", {
      market: "sh",
      code: "600036",
    });
    expect(result).toMatchObject({
      symbol: "600036",
      name: "替身标的 600036",
      type: "stock",
      market: "sh",
      currency_code: "CNY",
      price_cents: 403_000,
      price_date: "2026-01-30",
      price_written: true,
    });

    await expect(symbolsOf({ search: "600036" })).resolves.toEqual(["600036"]);
  });

  it("sh/sz 通道场内基金代码段（51/56/58、15 开头）自动识别为 etf", async () => {
    await expect(
      invoke<AddStockInstrumentResult>("add_instrument_by_code", { market: "sh", code: "510300" }),
    ).resolves.toMatchObject({ type: "etf", market: "sh", price_cents: 397_000 });
    await expect(
      invoke<AddStockInstrumentResult>("add_instrument_by_code", { market: "sz", code: "159949" }),
    ).resolves.toMatchObject({ type: "etf", market: "sz" });
  });

  it("hk 通道代码左补零至 5 位，命中自然键时复用既有行（幂等建档）", async () => {
    const result = await invoke<AddStockInstrumentResult>("add_instrument_by_code", {
      market: "hk",
      code: "700",
    });
    expect(result.symbol).toBe("00700");
    expect(result.instrument_id).toBe("mock-inst-hk"); // 复用夹具行，不新建
    await expect(symbolsOf({ market: "hk" })).resolves.toEqual(["00700"]);
  });

  it("us 通道代码归一大写，交易所归属确定性推导，币种 USD", async () => {
    const result = await invoke<AddStockInstrumentResult>("add_instrument_by_code", {
      market: "us",
      code: "aapl",
    });
    expect(result).toMatchObject({
      symbol: "AAPL",
      type: "stock",
      market: "nasdaq", // 首字母 < N → nasdaq（替身确定性规则，非真实归属）
      currency_code: "USD",
    });
  });

  it("非场内通道（fund/custom）与形态不符的代码拒绝", async () => {
    await expect(
      invoke("add_instrument_by_code", { market: "fund", code: "110022" }),
    ).rejects.toThrow(/通道/);
    await expect(invoke("add_instrument_by_code", { market: "custom", code: "X" })).rejects.toThrow(
      /通道/,
    );
    await expect(invoke("add_instrument_by_code", { market: "sh", code: "700" })).rejects.toThrow(
      /6 位数字/,
    );
    await expect(invoke("add_instrument_by_code", { market: "hk", code: "AAPL" })).rejects.toThrow(
      /数字/,
    );
  });

  it("建档后发恰一次 ledger:changed", async () => {
    const changed = captureLedgerChanged();
    await invoke<AddStockInstrumentResult>("add_instrument_by_code", {
      market: "sh",
      code: "600036",
    });
    expect(changed()).toBe(1);
  });
});
