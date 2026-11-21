import { afterEach, describe, expect, it } from "vitest";
import { errorMessage } from "../errors";
import { applyLocale } from "@ledger/i18n";

describe("errors.json 闭集未知值码表（#1071）", () => {
  afterEach(async () => {
    await applyLocale("zh-CN");
  });

  it("transaction.kind-unknown：未知值 + 合法值清单插值", async () => {
    const legal = "income/expense/transfer/refund/buy/sell/dividend/split/convert";
    const message = `未知交易类型: bonus（合法值: ${legal}）`;
    const wire = {
      kind: "Invalid",
      message,
      code: "transaction.kind-unknown",
      params: ["bonus", legal],
    };
    expect(errorMessage(wire), "zh 模板与后端 message 逐字一致").toBe(message);
    await applyLocale("en-US");
    expect(errorMessage(wire)).toBe(`unknown transaction kind: bonus (valid values: ${legal})`);
  });

  it("instrument.type-unknown：未知值 + 合法值清单插值", async () => {
    const legal = "stock/fund/bond/etf/other";
    const message = `未知金融工具类型: etfs（合法值: ${legal}）`;
    const wire = {
      kind: "Invalid",
      message,
      code: "instrument.type-unknown",
      params: ["etfs", legal],
    };
    expect(errorMessage(wire), "zh 模板与后端 message 逐字一致").toBe(message);
    await applyLocale("en-US");
    expect(errorMessage(wire)).toBe(`unknown instrument type: etfs (valid values: ${legal})`);
  });
});
