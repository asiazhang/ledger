import { beforeEach, describe, expect, it } from "vitest";
import type {
  InvestmentTransactionListResult,
  TransactionConvert,
  TransactionSplit,
  TransactionTrade,
} from "@ledger/types";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 投资明细与交易扩展投影域替身行为测试（issue #1970 / spec #1964 / ADR-0143）：
 * 接缝 = 应用所见的 tauri 命令面（`installMockBackend` + `invoke`），断言四维
 * 筛选 + offset 分页 + 编辑回填三投影的契约形状——删除任一 index.ts 登记行
 * 即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 夹具种子（fixtureSecurityTxns，人算期望值的独立事实源）：
 * - mock-txn-101 buy   2026-02-03  ETF 159915   数量 1000 / 单价 2.05 / 费 2 元，出资工资卡
 * - mock-txn-102 sell  2026-02-10  基金 000001  数量 2400 / 确认净额 30 元，到账工资卡
 * - mock-txn-103 convert 2026-02-15 基金→ETF  转出 1600 份 / 转入 97 份，行金额锚点 198 元
 * - mock-txn-104 split 2026-02-20  GOLD      Δ = +5，无现金腿金额恒 0
 * - mock-txn-105 dividend 2026-02-25 腾讯 00700  85 元，到账账户 = 工资卡（账户端）
 *
 * 期望值全部为独立人算字面量，不复算实现公式。纯读命令不发失效信号
 * （ledger:changed 恒 0）。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数（纯读命令断言恒 0，ADR-0143 决策 3 反向）。 */
function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

async function idsOf(filter: unknown): Promise<string[]> {
  const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
    filter,
  });
  return result.items.map((r) => r.id);
}

describe("list_investment_transactions：夹具投影与四维筛选", () => {
  it("无过滤返回五种投资 kind 行，通用 kind 不入集，date 倒序", async () => {
    const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: null,
    });
    expect(result.items.map((r) => r.id)).toEqual([
      "mock-txn-105",
      "mock-txn-104",
      "mock-txn-103",
      "mock-txn-102",
      "mock-txn-101",
    ]);
    expect(result.items.map((r) => r.kind)).toEqual([
      "dividend",
      "split",
      "convert",
      "sell",
      "buy",
    ]);
    expect(result.total).toBe(5);
  });

  it("行投影逐字段对齐契约：buy 行 JOIN 展示字段与 trade 载荷", async () => {
    const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: { kinds: ["buy"] },
    });
    expect(result.items[0]).toEqual({
      id: "mock-txn-101",
      date: "2026-02-03",
      kind: "buy",
      amount_cents: 205_200,
      account_id: "mock-acc-invest",
      funding_account_id: "mock-acc-bank",
      instrument_id: "mock-inst-etf",
      symbol: "159915",
      instrument_name: "创业板ETF",
      instrument_type: "etf",
      trade: { quantity: 1000, price_cents: 20_500, fee_cents: 200 },
      convert: null,
      split: null,
      note: "ETF 建仓",
      currency_code: "CNY",
      amount_native_cents: 205_200,
      source_order_no: "ORD-20260203-001",
    });
  });

  it("kind 专属载荷按形态携带：convert 两腿载荷 / split 带符号 Δ / dividend 无载荷", async () => {
    const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: null,
    });
    const convertRow = result.items.find((r) => r.id === "mock-txn-103");
    expect(convertRow?.instrument_id).toBe("mock-inst-fund"); // 公共标的地 = 转出腿
    expect(convertRow?.convert).toEqual({
      quantity: 1600,
      to_instrument_id: "mock-inst-etf",
      to_symbol: "159915",
      to_quantity: 97,
      out_amount_cents: 20_000,
      in_amount_cents: 19_900,
    });
    expect(convertRow?.amount_cents).toBe(19_800); // 行金额锚点 = 结转成本

    const splitRow = result.items.find((r) => r.id === "mock-txn-104");
    expect(splitRow?.split).toEqual({ delta_quantity: 5 });
    expect(splitRow?.amount_cents).toBe(0);

    const dividendRow = result.items.find((r) => r.id === "mock-txn-105");
    expect(dividendRow?.trade).toBeNull();
    expect(dividendRow?.convert).toBeNull();
    expect(dividendRow?.split).toBeNull();
    expect(dividendRow?.account_id).toBe("mock-acc-bank"); // 到账账户即账户端
  });

  it("账户筛选：涉及账户 ∪ 出资账户", async () => {
    // 工资卡：buy 出资、sell 到账、dividend 到账
    await expect(idsOf({ account_id: "mock-acc-bank" })).resolves.toEqual([
      "mock-txn-105",
      "mock-txn-102",
      "mock-txn-101",
    ]);
    // 券商账户端：buy/sell/convert/split 四行（dividend 到账在工资卡）
    await expect(idsOf({ account_id: "mock-acc-invest" })).resolves.toEqual([
      "mock-txn-104",
      "mock-txn-103",
      "mock-txn-102",
      "mock-txn-101",
    ]);
    await expect(idsOf({ account_id: "mock-acc-cash" })).resolves.toEqual([]);
  });

  it("标的筛选：convert 两腿任一命中（转出腿与转入腿都算）", async () => {
    // ETF：buy 直接持有 + convert 转入腿
    await expect(idsOf({ instrument_id: "mock-inst-etf" })).resolves.toEqual([
      "mock-txn-103",
      "mock-txn-101",
    ]);
    // 基金：sell 直接持有 + convert 转出腿
    await expect(idsOf({ instrument_id: "mock-inst-fund" })).resolves.toEqual([
      "mock-txn-103",
      "mock-txn-102",
    ]);
    // 腾讯：仅 dividend
    await expect(idsOf({ instrument_id: "mock-inst-hk" })).resolves.toEqual(["mock-txn-105"]);
  });

  it("kinds 子集维度内取或；通用 kind 恒空集不报错", async () => {
    await expect(idsOf({ kinds: ["buy"] })).resolves.toEqual(["mock-txn-101"]);
    await expect(idsOf({ kinds: ["buy", "dividend"] })).resolves.toEqual([
      "mock-txn-105",
      "mock-txn-101",
    ]);
    await expect(idsOf({ kinds: ["expense", "income"] })).resolves.toEqual([]);
    // 空集合视为未携带
    await expect(idsOf({ kinds: [] })).resolves.toHaveLength(5);
  });

  it("日期筛选：from / to 含端点，可叠加", async () => {
    await expect(idsOf({ from: "2026-02-11" })).resolves.toEqual([
      "mock-txn-105",
      "mock-txn-104",
      "mock-txn-103",
    ]);
    await expect(idsOf({ to: "2026-02-14" })).resolves.toEqual(["mock-txn-102", "mock-txn-101"]);
    await expect(idsOf({ from: "2026-02-05", to: "2026-02-18" })).resolves.toEqual([
      "mock-txn-103",
      "mock-txn-102",
    ]);
  });

  it("多维 AND 组合：账户 ∩ 标的 ∩ kinds ∩ 日期", async () => {
    await expect(idsOf({ account_id: "mock-acc-bank", kinds: ["buy", "sell"] })).resolves.toEqual([
      "mock-txn-102",
      "mock-txn-101",
    ]);
    await expect(idsOf({ instrument_id: "mock-inst-etf", from: "2026-02-05" })).resolves.toEqual([
      "mock-txn-103",
    ]);
  });
});

describe("list_investment_transactions：服务端 offset 分页", () => {
  it("page / page_size 服务端切片，total 恒为过滤全集", async () => {
    await expect(idsOf({ page: 1, page_size: 2 })).resolves.toEqual([
      "mock-txn-105",
      "mock-txn-104",
    ]);
    await expect(idsOf({ page: 2, page_size: 2 })).resolves.toEqual([
      "mock-txn-103",
      "mock-txn-102",
    ]);
    await expect(idsOf({ page: 3, page_size: 2 })).resolves.toEqual(["mock-txn-101"]);
    await expect(idsOf({ page: 4, page_size: 2 })).resolves.toEqual([]);

    const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: { page: 2, page_size: 2 },
    });
    expect(result.total).toBe(5);
  });

  it("page_size 缺省返回全部（total 恒返回）", async () => {
    const result = await invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: { page: 2 },
    });
    expect(result.items).toHaveLength(5);
    expect(result.total).toBe(5);
  });
});

describe("get_transaction_trade / convert / split：编辑回填三投影", () => {
  it("buy / sell 行回填标的展示字段与数量单价费用", async () => {
    const buy = await invoke<TransactionTrade>("get_transaction_trade", { id: "mock-txn-101" });
    expect(buy).toEqual({
      instrument_id: "mock-inst-etf",
      symbol: "159915",
      instrument_name: "创业板ETF",
      instrument_type: "etf",
      quantity: 1000,
      price_cents: 20_500,
      fee_cents: 200,
    });

    const sell = await invoke<TransactionTrade>("get_transaction_trade", { id: "mock-txn-102" });
    expect(sell?.symbol).toBe("000001");
    expect(sell?.instrument_type).toBe("fund");
    expect(sell?.quantity).toBe(2400);
    expect(sell?.price_cents).toBe(12_625); // 金额权威反算单价（毛收入 30.3 元 ÷ 2400 份）
    expect(sell?.fee_cents).toBe(30);
  });

  it("convert 行回填「A → B」全量信息，结转成本与币种随行", async () => {
    const convert = await invoke<TransactionConvert>("get_transaction_convert", {
      id: "mock-txn-103",
    });
    expect(convert).toEqual({
      out_instrument_id: "mock-inst-fund",
      out_symbol: "000001",
      out_instrument_name: "华夏成长混合",
      out_quantity: 1600,
      out_amount_cents: 20_000,
      in_instrument_id: "mock-inst-etf",
      in_symbol: "159915",
      in_instrument_name: "创业板ETF",
      in_quantity: 97,
      in_amount_cents: 19_900,
      fee_cents: 100,
      carried_cost_cents: 19_800,
      currency_code: "CNY",
    });
  });

  it("split 行回填标的与带符号份额增量 Δ", async () => {
    const split = await invoke<TransactionSplit>("get_transaction_split", {
      id: "mock-txn-104",
    });
    expect(split).toEqual({
      instrument_id: "mock-inst-custom",
      symbol: "GOLD",
      instrument_name: "家庭黄金存折",
      quantity: 5,
    });
  });

  it("非对应 kind 查投影被拒绝（存在性校验形态）", async () => {
    // trade 投影：convert/split/dividend/通用 kind/不存在
    for (const id of [
      "mock-txn-103",
      "mock-txn-104",
      "mock-txn-105",
      "mock-txn-001",
      "mock-txn-none",
    ]) {
      await expect(invoke("get_transaction_trade", { id })).rejects.toThrow(/明细|不存在/);
    }
    // convert 投影：buy/sell/不存在
    for (const id of ["mock-txn-101", "mock-txn-102", "mock-txn-none"]) {
      await expect(invoke("get_transaction_convert", { id })).rejects.toThrow(/明细|不存在/);
    }
    // split 投影：convert/dividend/不存在
    for (const id of ["mock-txn-103", "mock-txn-105", "mock-txn-none"]) {
      await expect(invoke("get_transaction_split", { id })).rejects.toThrow(/明细|不存在/);
    }
  });

  it("纯读命令不发失效信号（ledger:changed 恒 0）", async () => {
    const changed = captureLedgerChanged();
    await invoke("list_investment_transactions", { filter: null });
    await invoke("get_transaction_trade", { id: "mock-txn-101" });
    await invoke("get_transaction_convert", { id: "mock-txn-103" });
    await invoke("get_transaction_split", { id: "mock-txn-104" });
    expect(changed()).toBe(0);
  });
});
