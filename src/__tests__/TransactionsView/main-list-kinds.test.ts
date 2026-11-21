import { routeMock, makeTxn, setTxnDb, mountView, lastListFilter } from "./common";
import { describe, it, expect } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import {
  MAIN_LIST_KINDS,
  normalizeKindSelection,
  resolveRequestKinds,
} from "@/transaction/useTransactionFilter";

describe("主列表请求显式携带四通用 kind（ADR-0135 / issue #1783，调用事实）", () => {
  it("默认态请求显式携带四通用 kind（含默认态；删除该携带即红）", async () => {
    await mountView();
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
  });

  it("主列表不呈现投资行：投资行在库但请求默认排除，「共 N 条」随之收窄", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { kind: "expense", date: "2026-01-05" }),
      makeTxn(2, "acc-1", { kind: "income", date: "2026-01-06" }),
      makeTxn(3, "acc-1", { kind: "buy", date: "2026-01-07" }),
      makeTxn(4, "acc-1", { kind: "dividend", date: "2026-01-08" }),
      makeTxn(5, "acc-1", { kind: "split", date: "2026-01-09" }),
    ]);
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("共 2 条");
    expect(wrapper.text()).not.toContain("买入");
    expect(wrapper.text()).not.toContain("分红");
    expect(wrapper.text()).not.toContain("份额调整");
  });

  it("账户下钻与手动账户过滤同口径：两条入口的请求都显式携带四通用 kind（维度正交，不开例外）", async () => {
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-1" });
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
    wrapper.findAllComponents(NSelect)[0].vm.$emit("update:value", "acc-2");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-2" });
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
  });
});

describe("主列表 kind 口径纯函数（resolveRequestKinds / normalizeKindSelection）", () => {
  it("resolveRequestKinds：默认态携带四通用 kind，显式集合在场按其携带（浅拷贝脱只读）", () => {
    expect(resolveRequestKinds(null)).toEqual([...MAIN_LIST_KINDS]);
    const explicit: Array<(typeof MAIN_LIST_KINDS)[number]> = ["expense"];
    const resolved = resolveRequestKinds(explicit);
    expect(resolved).toEqual(["expense"]);
    expect(resolved).not.toBe(explicit);
  });

  it("normalizeKindSelection：空集合与满选四通用 kind 都归一为 null（满选 ≡ 默认，ADR-0135）", () => {
    expect(normalizeKindSelection(null)).toBeNull();
    expect(normalizeKindSelection([])).toBeNull();
    expect(normalizeKindSelection([...MAIN_LIST_KINDS])).toBeNull();
    expect(normalizeKindSelection(["expense"])).toEqual(["expense"]);
    expect(normalizeKindSelection(["income", "refund"])).toEqual(["income", "refund"]);
  });
});
