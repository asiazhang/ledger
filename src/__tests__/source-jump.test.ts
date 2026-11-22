import { describe, expect, it, vi } from "vitest";
import { resolveSourceJumpTarget } from "@/components/source-jump";
import type { TransactionSourceKind } from "@/components/source-jump";

const allMain = () => false;
const allContained = () => true;
const allOpen = () => false;
const allClosed = () => true;

const SIX_KINDS: readonly TransactionSourceKind[] = [
  "installmentPlan",
  "subscription",
  "scheduledTransfer",
  "policy",
  "item",
  "instrument",
];

describe("resolveSourceJumpTarget 主项态（独立路由直达）", () => {
  it("分期计划：落定时视图分期页签 + focus", () => {
    expect(resolveSourceJumpTarget("installmentPlan", "p1", allMain, allOpen)).toEqual({
      name: "scheduled",
      query: { tab: "installments", focus: "p1" },
    });
  });

  it("订阅计划：落定时视图订阅页签 + focus", () => {
    expect(resolveSourceJumpTarget("subscription", "p2", allMain, allOpen)).toEqual({
      name: "scheduled",
      query: { tab: "subscriptions", focus: "p2" },
    });
  });

  it("定时转账计划：落定时视图定时转账页签 + focus", () => {
    expect(resolveSourceJumpTarget("scheduledTransfer", "p3", allMain, allOpen)).toEqual({
      name: "scheduled",
      query: { tab: "transfers", focus: "p3" },
    });
  });

  it("保单：落保单独立路由 + focus（收纳分流由路由守卫既有重定向兜底）", () => {
    expect(resolveSourceJumpTarget("policy", "pol1", allMain, allOpen)).toEqual({
      name: "policies",
      query: { focus: "pol1" },
    });
  });

  it("物品：落物品主项路由 + focus", () => {
    expect(resolveSourceJumpTarget("item", "it1", allMain, allOpen)).toEqual({
      name: "items",
      query: { focus: "it1" },
    });
  });

  it("标的：落投资主项路由 + focus（走势页签选中归落点侧 focus 消费）", () => {
    expect(resolveSourceJumpTarget("instrument", "ins1", allMain, allOpen)).toEqual({
      name: "investments",
      query: { focus: "ins1" },
    });
  });
});

describe("resolveSourceJumpTarget 收纳态（组「更多」页签落点）", () => {
  it("计划三形态：落记账「更多」定时页签，形态页签以 scheduledTab 叠加 + focus", () => {
    expect(resolveSourceJumpTarget("installmentPlan", "p1", allContained, allOpen)).toEqual({
      name: "bookkeeping-more",
      query: { tab: "scheduled", scheduledTab: "installments", focus: "p1" },
    });
    expect(resolveSourceJumpTarget("subscription", "p2", allContained, allOpen)).toEqual({
      name: "bookkeeping-more",
      query: { tab: "scheduled", scheduledTab: "subscriptions", focus: "p2" },
    });
    expect(resolveSourceJumpTarget("scheduledTransfer", "p3", allContained, allOpen)).toEqual({
      name: "bookkeeping-more",
      query: { tab: "scheduled", scheduledTab: "transfers", focus: "p3" },
    });
  });

  it("保单收纳态：直达资产「更多」保单页签 + focus（语义先行；路由守卫透传由消费票接线）", () => {
    expect(resolveSourceJumpTarget("policy", "pol1", allContained, allOpen)).toEqual({
      name: "assets-more",
      query: { tab: "policies", focus: "pol1" },
    });
  });

  it("物品收纳态：落资产「更多」物品页签 + focus（#474 移入后的用户布局）", () => {
    expect(resolveSourceJumpTarget("item", "it1", allContained, allOpen)).toEqual({
      name: "assets-more",
      query: { tab: "items", focus: "it1" },
    });
  });

  it("标的收纳态：落资产「更多」投资页签 + focus", () => {
    expect(resolveSourceJumpTarget("instrument", "ins1", allContained, allOpen)).toEqual({
      name: "assets-more",
      query: { tab: "investments", focus: "ins1" },
    });
  });
});

describe("resolveSourceJumpTarget 收纳谓词按目标视图逐一定问", () => {
  it("计划形态只问 scheduled、保单只问 policies（互不误伤）", () => {
    const asked = vi.fn(() => false);
    resolveSourceJumpTarget("installmentPlan", "p1", asked, allOpen);
    expect(asked).toHaveBeenCalledTimes(1);
    expect(asked).toHaveBeenCalledWith("scheduled");

    resolveSourceJumpTarget("policy", "pol1", asked, allOpen);
    expect(asked).toHaveBeenLastCalledWith("policies");
  });

  it("混合收纳态：定时收纳而保单主项态，各自落对落点", () => {
    const contained = (view: string) => view === "scheduled";
    expect(resolveSourceJumpTarget("subscription", "p2", contained, allOpen)).toEqual({
      name: "bookkeeping-more",
      query: { tab: "scheduled", scheduledTab: "subscriptions", focus: "p2" },
    });
    expect(resolveSourceJumpTarget("policy", "pol1", contained, allOpen)).toEqual({
      name: "policies",
      query: { focus: "pol1" },
    });
  });
});

describe("resolveSourceJumpTarget focus 参数统一装配", () => {
  it("六类来源 × 两态共 12 个目标全部携带 focus=<实体 id>", () => {
    for (const kind of SIX_KINDS) {
      for (const contained of [allMain, allContained]) {
        const target = resolveSourceJumpTarget(kind, "entity-42", contained, allOpen);
        expect(target.query.focus).toBe("entity-42");
      }
    }
  });
});

describe("resolveSourceJumpTarget 关闭态（引用不转收纳落点）", () => {
  it("计划三形态：收纳态也落定时独立路由，形态页签归位到定时视图 query.tab", () => {
    expect(resolveSourceJumpTarget("installmentPlan", "p1", allContained, allClosed)).toEqual({
      name: "scheduled",
      query: { tab: "installments", focus: "p1" },
    });
    expect(resolveSourceJumpTarget("subscription", "p2", allContained, allClosed)).toEqual({
      name: "scheduled",
      query: { tab: "subscriptions", focus: "p2" },
    });
    expect(resolveSourceJumpTarget("scheduledTransfer", "p3", allContained, allClosed)).toEqual({
      name: "scheduled",
      query: { tab: "transfers", focus: "p3" },
    });
  });

  it("保单/物品/标的：收纳态也落各自独立路由 + focus", () => {
    expect(resolveSourceJumpTarget("policy", "pol1", allContained, allClosed)).toEqual({
      name: "policies",
      query: { focus: "pol1" },
    });
    expect(resolveSourceJumpTarget("item", "it1", allContained, allClosed)).toEqual({
      name: "items",
      query: { focus: "it1" },
    });
    expect(resolveSourceJumpTarget("instrument", "ins1", allContained, allClosed)).toEqual({
      name: "investments",
      query: { focus: "ins1" },
    });
  });

  it("关闭谓词按目标视图逐一定问（只问本次跳转的目标视图）", () => {
    const closed = vi.fn(() => false);
    resolveSourceJumpTarget("policy", "pol1", allContained, closed);
    expect(closed).toHaveBeenCalledTimes(1);
    expect(closed).toHaveBeenCalledWith("policies");
  });

  it("关闭不改写收纳清单：同一收纳态下重新打开即回原落点", () => {
    expect(resolveSourceJumpTarget("policy", "pol1", allContained, allClosed)).toEqual({
      name: "policies",
      query: { focus: "pol1" },
    });
    expect(resolveSourceJumpTarget("policy", "pol1", allContained, allOpen)).toEqual({
      name: "assets-more",
      query: { tab: "policies", focus: "pol1" },
    });
  });
});
