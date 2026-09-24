// 路由替身经 common.ts 的 vi.mock 注册，必须先于任何直连组件导入（导入顺序即 mock 生效面）
import { routeMock, makeTxn, setTxnDb, mountView, lastListFilter } from "./common";
import { describe, it, expect } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import {
  MAIN_LIST_KINDS,
  normalizeKindSelection,
  resolveRequestKinds,
} from "@/transaction/useTransactionFilter";

/**
 * 主列表收窄为四通用 kind（issue #1783 / ADR-0135）：主交易列表只呈现四通用 kind，
 * 排除机制走前端显式 kind 集合——每次列表请求显式携带 kind 集合（含默认态），调用
 * 事实断言打 list_transactions invoke 接缝（删除该携带即红：默认请求回退为无 kinds
 * 参数时本文件首条用例失败，ADR-0087 接线型负向条目）。类型下拉可选集收窄为四项
 * （满选与默认态同集）；账户过滤维度正交——账户下钻与手动过滤同口径，不开例外。
 * `list_transactions` 契约本身零改动（无 kinds 参数仍返回全部 kind），由 e2e BDD
 * 哨兵钉住（transactions_query.feature「契约哨兵」场景）。
 */

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
    // 主列表只呈现四通用 kind 行（买入/分红/份额调整行不出现）
    expect(wrapper.text()).toContain("共 2 条");
    expect(wrapper.text()).not.toContain("买入");
    expect(wrapper.text()).not.toContain("分红");
    expect(wrapper.text()).not.toContain("份额调整");
  });

  it("账户下钻与手动账户过滤同口径：两条入口的请求都显式携带四通用 kind（维度正交，不开例外）", async () => {
    // 入口 1：账户名下钻（URL ?account=）
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-1" });
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
    // 入口 2：手动账户过滤（同一行集口径）
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
    // 浅拷贝：返回值与入参非同一实例
    expect(resolved).not.toBe(explicit);
  });

  it("normalizeKindSelection：空集合与满选四通用 kind 都归一为 null（满选 ≡ 默认，ADR-0135）", () => {
    expect(normalizeKindSelection(null)).toBeNull();
    expect(normalizeKindSelection([])).toBeNull();
    expect(normalizeKindSelection([...MAIN_LIST_KINDS])).toBeNull();
    // 子集保持显式集合（浅拷贝脱只读）
    expect(normalizeKindSelection(["expense"])).toEqual(["expense"]);
    expect(normalizeKindSelection(["income", "refund"])).toEqual(["income", "refund"]);
  });
});
