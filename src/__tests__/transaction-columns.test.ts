import { describe, expect, it, vi } from "vitest";
import { NPerformantEllipsis, type DataTableColumn } from "naive-ui";
import type { VNode } from "vue";
import {
  buildTransactionColumns,
  displayAmountText,
  expandPurchaseRows,
  precomputeTransactionDisplayRows,
  renderPurchaseLine,
  rowActionsColumn,
  KIND_TAG_TYPE,
  type ReferenceStore,
  type PrecomputedTransactionRow,
} from "@/transaction/transaction-columns";
import SourceLink from "@/transaction/SourceLink.vue";
import NoteCopyButton from "@ledger/ui-kit/NoteCopyButton.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import {
  TRANSACTION_KINDS,
  type Account,
  type Merchant,
  type Transaction,
  type TransactionSource,
} from "@ledger/types";
import { amountPrivacyEnabled, formatAmount } from "@ledger/money";
import { kindLabel } from "@/transaction/transaction-columns";
import { makeAccount, makeFunding, makeMerchant, makePurchase, makeTransaction } from "./factories";

/** 金额列按交易类型语义着色（issue #435）：只测外部行为——
 * 给定交易类型与主题，金额单元格最终呈现语义色模块给出的颜色；
 * 模块自身的色值定案见 semantic-colors.test.ts。
 * 单元格交互归 AmountCell 组件（issue #843），此处只断言列配置的载荷单点。 */

const reference = {
  categoryPath: () => null,
  accountMap: new Map(),
  getCurrency: () => undefined,
} as unknown as ReferenceStore;

/**
 * 按键取渲染列：DataTableColumn 是含分组列的联合（key/render 并非每支都有），
 * 本文件只消费「带键、带 render 的普通列」——经断言守卫单点窄化，
 * 不在用例内散布 as/非空断言。
 */
function renderColumnOf(columns: DataTableColumn<Transaction>[], key: string) {
  const hit = columns.find((c) => (c as { key?: unknown }).key === key);
  expect(hit, `列 ${key} 应存在`).toBeTruthy();
  const render = (hit as { render?: unknown }).render;
  expect(typeof render).toBe("function");
  return render as (row: Transaction, index: number) => unknown;
}

function amountCellOf(row: Transaction): VNode {
  const render = renderColumnOf(buildTransactionColumns(reference), "amount_native_cents");
  return render(row, 0) as VNode;
}

describe("buildTransactionColumns 金额单元格语义着色", () => {
  it("暗色主题（默认）：逐类型呈现语义色暗色变体", () => {
    const app = useAppStore();
    app.setTheme("dark");
    for (const kind of TRANSACTION_KINDS) {
      const vnode = amountCellOf(makeTransaction({ id: `tx-${kind}`, kind }));
      expect(vnode.type).toBe(AmountCell);
      expect((vnode.props as { color: string }).color, kind).toBe(kindSemanticColor(kind, "dark"));
    }
  });

  it("亮色主题：逐类型呈现语义色亮色变体（收入绿/支出红/退款蓝维持既有亮色值）", () => {
    const app = useAppStore();
    app.setTheme("light");
    for (const kind of TRANSACTION_KINDS) {
      const vnode = amountCellOf(makeTransaction({ id: `tx-${kind}`, kind }));
      expect((vnode.props as { color: string }).color, kind).toBe(kindSemanticColor(kind, "light"));
    }
  });

  it("切换主题即时换色：同一列配置下重渲染即取新主题色（无需重建列）", () => {
    const app = useAppStore();
    app.setTheme("dark");
    const row = makeTransaction({ id: "tx-expense", kind: "expense" });
    const colorOf = () => (amountCellOf(row).props as { color: string }).color;
    const darkStyle = colorOf();
    app.setTheme("light");
    expect(colorOf()).not.toBe(darkStyle);
    expect(colorOf()).toBe(kindSemanticColor("expense", "light"));
  });

  it("金额文案单点：单元格载荷携带 formatAmount 产物（含币种形态，issue #843 AmountCell 载荷）", () => {
    const row = makeTransaction({ id: "tx-cny", amount_native_cents: 123456 });
    const vnode = amountCellOf(row);
    expect((vnode.props as { text: string }).text).toBe(
      formatAmount(123456, reference.getCurrency(row.currency_code)),
    );
  });

  it("转换行金额显示转出金额（确认单口径），不读行金额锚点（结转成本，ADR-0099）", () => {
    const app = useAppStore();
    app.setTheme("dark");
    const row = makeTransaction({
      id: "tx-convert",
      kind: "convert",
      // 行金额锚点 = 结转成本（3590.62），展示口径 = 转出金额（3615.61）。
      amount_native_cents: 359062,
      convert: {
        to_instrument_id: "inst-in",
        to_symbol: "519700",
        to_quantity: 10,
        out_amount_cents: 361561,
        in_amount_cents: 361561,
      },
    });
    const vnode = amountCellOf(row);
    expect((vnode.props as { text: string }).text).toBe(
      formatAmount(361561, reference.getCurrency(row.currency_code)),
    );
    expect((vnode.props as { color: string }).color).toBe(kindSemanticColor("convert", "dark"));
  });
});

/** 来源列（spec #704 / issue #706）：列序与渲染产物——
 * 只测外部行为：列位置、单元格产物（SourceLink 组件/占位符），
 * 链接交互与状态标注的渲染矩阵归 SourceLink 组件测试（一缝一测）。 */
describe("buildTransactionColumns 来源列", () => {
  function columnByKey(key: string) {
    return renderColumnOf(buildTransactionColumns(reference), key);
  }

  function sourceCellOf(row: Transaction) {
    return columnByKey("source")(row, 0);
  }

  it("列序：来源列位于账户之后、备注之前", () => {
    const keys = buildTransactionColumns(reference).map((c) => (c as { key?: string }).key);
    expect(keys.indexOf("account_id")).toBeLessThan(keys.indexOf("source"));
    expect(keys.indexOf("source")).toBeLessThan(keys.indexOf("note"));
  });

  it("保单来源渲染 SourceLink，携带行来源对象", () => {
    const source: TransactionSource = {
      kind: "policy",
      entity_id: "pol-1",
      display_name: "重疾险",
      status: null,
    };
    const vnode = sourceCellOf(makeTransaction({ id: "t1", source })) as VNode;
    expect(vnode.type).toBe(SourceLink);
    expect((vnode.props as { source: TransactionSource }).source).toEqual(source);
  });

  it("软删保单来源同样走 SourceLink（禁用点击/标注归组件渲染矩阵）", () => {
    const source: TransactionSource = {
      kind: "policy",
      entity_id: "pol-2",
      display_name: "医疗险",
      status: "deleted",
    };
    const vnode = sourceCellOf(makeTransaction({ id: "t2", source })) as VNode;
    expect(vnode.type).toBe(SourceLink);
  });

  it("无来源留空（占位符，手动/AI 导入口径）", () => {
    expect(sourceCellOf(makeTransaction({ id: "t3" }))).toBe("-");
  });
});

/** 备注列（显式复制通道，见「界面文本不可选」词条）：只测单元格产物——
 * 占位符/容器结构/按钮载荷；复制动作与 toast 归 NoteCopyButton 组件测试（一缝一测）。 */
describe("buildTransactionColumns 备注列", () => {
  function noteCellOf(row: Transaction) {
    const render = renderColumnOf(buildTransactionColumns(reference), "note");
    return render(row, 0);
  }

  it("无备注渲染占位符，不渲染复制按钮（空备注无可复制）", () => {
    expect(noteCellOf(makeTransaction({ id: "t1", note: null }))).toBe("-");
  });

  it("有备注渲染单元格容器：文本 NEllipsis（自省略+悬停全文）与复制按钮并排，按钮携带完整备注", () => {
    const vnode = noteCellOf(makeTransaction({ id: "t2", note: "视频会员月费" })) as VNode;
    expect((vnode.props as { style: string }).style).toContain("display: flex");
    const children = vnode.children as VNode[];
    expect(children).toHaveLength(2);
    expect(children[0].type).toBe(NPerformantEllipsis);
    expect(children[1].type).toBe(NoteCopyButton);
    expect((children[1].props as { note: string }).note).toBe("视频会员月费");
  });
});

/** 交易行「⋯」常显操作列（ADR-0088 决策 6 / issue #843）：只测装配面——
 * 回调在场才追加列、菜单打开回调随按钮携行；菜单项集合与动作分派归
 * TransactionsView 组件测试（行菜单编排接缝，一缝一测）。 */
describe("buildTransactionColumns 操作列（「⋯」常显第二入口）", () => {
  const row = makeTransaction({ id: "t-actions", kind: "expense" });

  it("未声明 onRowMenuOpen 不追加操作列（搜索结果保持只读）", () => {
    const keys = buildTransactionColumns(reference).map((c) => (c as { key?: string }).key);
    expect(keys).not.toContain("actions");
  });

  it("声明 onRowMenuOpen 追加末位操作列：tiny「⋯」按钮，点击以事件与目标行回调", () => {
    const onRowMenuOpen = vi.fn();
    const columns = buildTransactionColumns(reference, { onRowMenuOpen });
    const actions = columns.find((c) => (c as { key?: string }).key === "actions");
    expect(actions).toBeTruthy();
    const keys = columns.map((c) => (c as { key?: string }).key);
    expect(keys[keys.length - 1]).toBe("actions");
    const vnode = (actions as unknown as { render: (row: Transaction) => VNode }).render(row);
    const children = vnode.children as { default: () => string };
    expect(children.default()).toBe("⋯");
    const props = vnode.props as {
      class: string;
      "aria-label": string;
      onClick: (e: MouseEvent) => void;
    };
    expect(props.class).toBe("row-actions-btn touch-hit-area");
    expect(props["aria-label"]).toBe("更多操作");
    const event = new MouseEvent("click", { clientX: 10, clientY: 20 });
    props.onClick(event);
    expect(onRowMenuOpen).toHaveBeenCalledWith(event, row);
  });
});

/** 购买项展开装配（issue #1883 / ADR-0138 决策 13）：列构造器是展开 + 合并规则的
 * 生产单源——订单级列 rowSpan 纵向合并 + 合并格居中类，分类/备注列不合并。
 * 视图层外部行为归 TransactionsView/purchase-read.test.ts，此处只测列装配面。 */
describe("buildTransactionColumns 购买项展开装配", () => {
  const ORDER_LEVEL_KEYS = [
    "date",
    "kind",
    "merchant_id",
    "account_id",
    "source",
    "amount_native_cents",
    "actions",
  ];

  function expandedRows() {
    return expandPurchaseRows([
      makeTransaction({
        id: "t-order",
        purchases: [
          makePurchase({ name: "猫粮", quantity: 1 }),
          makePurchase({ name: "洗衣液", quantity: 2 }),
          makePurchase({ name: "纸巾", quantity: 1 }),
        ],
      }),
      makeTransaction({ id: "t-plain", note: "普通行" }),
    ]);
  }

  function rowSpanOf(columns: DataTableColumn<Transaction>[], key: string) {
    const hit = columns.find((c) => (c as { key?: unknown }).key === key);
    expect(hit, `列 ${key} 应存在`).toBeTruthy();
    return (hit as { rowSpan?: (row: Transaction, index: number) => number }).rowSpan;
  }

  it("缺省（未声明 expandPurchases）任何列都不带 rowSpan，备注列名不变——未展开行集零变化", () => {
    const columns = buildTransactionColumns(reference, { onRowMenuOpen: () => {} });
    for (const key of [...ORDER_LEVEL_KEYS, "category_id", "note"]) {
      expect(rowSpanOf(columns, key)).toBeUndefined();
    }
    const note = columns.find((c) => (c as { key?: unknown }).key === "note");
    expect((note as { title?: string }).title).toBe("备注");
  });

  it("展开装配：订单级列带 rowSpan（首行 = 件数、续行与普通行 = 1）与合并格居中类", () => {
    const columns = buildTransactionColumns(reference, {
      onRowMenuOpen: () => {},
      expandPurchases: true,
    });
    const [orderFirst, orderSecond, , plainRow] = expandedRows();
    for (const key of ORDER_LEVEL_KEYS) {
      const rowSpan = rowSpanOf(columns, key);
      expect(rowSpan, key).toBeTruthy();
      expect(rowSpan!(orderFirst, 0)).toBe(3);
      expect(rowSpan!(orderSecond, 1)).toBe(1);
      expect(rowSpan!(plainRow, 3)).toBe(1);
      const col = columns.find((c) => (c as { key?: unknown }).key === key);
      expect((col as { className?: string }).className, key).toContain("purchase-merged-cell");
    }
  });

  it("展开装配：分类列与备注列不合并（分类逐行、备注两用）", () => {
    const columns = buildTransactionColumns(reference, { expandPurchases: true });
    expect(rowSpanOf(columns, "category_id")).toBeUndefined();
    expect(rowSpanOf(columns, "note")).toBeUndefined();
  });

  it("展开装配：备注列名「商品 / 备注」", () => {
    const columns = buildTransactionColumns(reference, { expandPurchases: true });
    const note = columns.find((c) => (c as { key?: unknown }).key === "note");
    expect((note as { title?: string }).title).toBe("商品 / 备注");
  });

  it("购买项单元格：商品名走 NEllipsis（悬停全文）+ 件数标注，无复制按钮", () => {
    const render = renderColumnOf(
      buildTransactionColumns(reference, { expandPurchases: true }),
      "note",
    );
    const rows = expandedRows();
    const vnode = render(rows[1], 1) as VNode;
    const children = vnode.children as VNode[];
    expect(children).toHaveLength(2);
    expect(children[0].type).toBe(NPerformantEllipsis);
    expect((children[1].children as string).length).toBeGreaterThan(0);
  });

  it("截断形态（列表）与全文形态（卡片 / 详情）经同一渲染单点分形：懒挂载省略 vs 整名渲染", () => {
    const item = makePurchase({
      name: "超长商品名『进口无谷深海鱼油成猫粮专用 10kg 装大袋』",
      quantity: 3,
    });
    // 列表形态：名内懒挂载省略（初始纯 span，悬停才挂提示弹层，#2188）+ 件数标注
    const truncated = renderPurchaseLine(item, { truncate: true });
    const truncChildren = truncated.children as VNode[];
    expect(truncChildren[0].type).toBe(NPerformantEllipsis);
    expect(truncChildren).toHaveLength(2);
    // 全文形态（卡片 / 详情）：整名渲染、自然换行——触控轴无悬停，详情是全文出口
    const full = renderPurchaseLine(item);
    const fullChildren = full.children as VNode[];
    expect(fullChildren[0].type).not.toBe(NPerformantEllipsis);
    expect(fullChildren[0].children as string).toContain("10kg 装大袋』");
    expect(fullChildren).toHaveLength(2);
  });

  it("件数标注两形态同文案单源（transactions.purchase.quantity），全文形态可附分类标注", () => {
    const item = makePurchase({ name: "猫粮", quantity: 2 });
    const qtyText = (vnode: VNode) =>
      ((vnode.children as VNode[]).at(-1) as VNode).children as string;
    expect(qtyText(renderPurchaseLine(item, { truncate: true }))).toBe("共 2 件");
    expect(qtyText(renderPurchaseLine(item))).toBe("共 2 件");
    // 详情形态：分类标注插入在件数标注之前（名称 / 分类 / 件数顺序）
    const withCategory = renderPurchaseLine(item, { suffix: "生活 > 宠物" });
    const children = withCategory.children as VNode[];
    expect(children).toHaveLength(3);
    expect(children[1].children as string).toBe("生活 > 宠物");
  });

  it("展开视图下非购买项行仍走原备注渲染（有备注带复制按钮）", () => {
    const render = renderColumnOf(
      buildTransactionColumns(reference, { expandPurchases: true }),
      "note",
    );
    const rows = expandedRows();
    const vnode = render(rows[3], 3) as VNode;
    const children = vnode.children as VNode[];
    expect(children[0].type).toBe(NPerformantEllipsis);
    expect(children[1].type).toBe(NoteCopyButton);
  });

  it("rowActionsColumn 缺省不带合并装配（投资明细页签零变化）", () => {
    const col = rowActionsColumn<Transaction>(() => {});
    expect((col as { rowSpan?: unknown }).rowSpan).toBeUndefined();
    expect((col as { className?: unknown }).className).toBeUndefined();
  });
});

/** 展示行预计算（issue #2188）：纯函数入口以行集与只读参考上下文为输入，一次性产出
 * 每条显示行的展示文本与属性。验收判据：全部展示形态与原渲染口径逐字段一致
 * （同批派生单源函数的同一时点输出）。 */
describe("precomputeTransactionDisplayRows 展示行预计算", () => {
  // 参考上下文替身：分类路径映射（多级 + 软删同口径）、账户映射（借贷派生）、
  // 商户映射（在用 + 软删）、币种免折算。语义与 Reference Data store 派生映射一致。
  const PATHS: Record<string, string> = {
    "cat-food": "生活 > 餐饮",
    "cat-pet": "生活 > 宠物",
    "cat-del": "已删分类 > 历史",
  };
  const accounts = new Map<string, Account>([
    ["acc-recv", makeAccount({ id: "acc-recv", type: "receivable" })],
    ["acc-debt", makeAccount({ id: "acc-debt", type: "debt" })],
    ["acc-cash", makeAccount({ id: "acc-cash", type: "cash" })],
  ]);
  const merchants = new Map<string, Merchant>([
    ["mch-1", makeMerchant({ id: "mch-1", name: "京东" })],
    ["mch-del", makeMerchant({ id: "mch-del", name: "已闭店商城", is_deleted: true })],
  ]);
  const refStub = {
    categoryPath: (id: string | null | undefined) => (id ? (PATHS[id] ?? null) : null),
    accountMap: accounts,
    merchantMap: merchants,
    getCurrency: () => undefined,
  } as unknown as ReferenceStore;

  function pre(
    rows: Transaction[],
    opts?: { expandPurchases?: boolean },
  ): PrecomputedTransactionRow[] {
    return precomputeTransactionDisplayRows(refStub, rows, opts);
  }

  it("四通用 kind：类型标签文案与色型与 kindLabel/KIND_TAG_TYPE 单源一致", () => {
    const rows = [
      makeTransaction({ id: "t-expense", kind: "expense" }),
      makeTransaction({ id: "t-income", kind: "income" }),
      makeTransaction({ id: "t-transfer", kind: "transfer", to_account_id: "acc-cash" }),
      makeTransaction({ id: "t-refund", kind: "refund" }),
    ];
    const computed = pre(rows);
    rows.forEach((row, i) => {
      expect(computed[i].display.kindLabel, row.kind).toBe(kindLabel(refStub, row));
      expect(computed[i].display.kindTagType).toBe(KIND_TAG_TYPE[row.kind]);
    });
  });

  it("借贷派生四方向：转账两端账户类型派生借出/收回/借入/还款（与 kindLabel 单源一致）", () => {
    const rows = [
      makeTransaction({
        id: "t-lend",
        kind: "transfer",
        account_id: "acc-cash",
        to_account_id: "acc-recv",
      }),
      makeTransaction({
        id: "t-collect",
        kind: "transfer",
        account_id: "acc-recv",
        to_account_id: "acc-cash",
      }),
      makeTransaction({
        id: "t-borrow",
        kind: "transfer",
        account_id: "acc-debt",
        to_account_id: "acc-cash",
      }),
      makeTransaction({
        id: "t-repay",
        kind: "transfer",
        account_id: "acc-cash",
        to_account_id: "acc-debt",
      }),
      // 普通转账（资金互转）：不派生，仍「转账」
      makeTransaction({
        id: "t-normal",
        kind: "transfer",
        account_id: "acc-cash",
        to_account_id: "acc-cash",
      }),
    ];
    const computed = pre(rows);
    rows.forEach((row, i) => {
      expect(computed[i].display.kindLabel, row.id).toBe(kindLabel(refStub, row));
    });
    expect(computed[0].display.kindLabel).toBe("借出");
    expect(computed[1].display.kindLabel).toBe("收回");
    expect(computed[2].display.kindLabel).toBe("借入");
    expect(computed[3].display.kindLabel).toBe("还款");
    expect(computed[4].display.kindLabel).toBe("转账");
  });

  it("账户展示形态：普通转账双账户（转出在前）、buy/sell 双账户、单账户与分派逻辑一致", () => {
    const rows = [
      makeTransaction({
        id: "t-transfer",
        kind: "transfer",
        account_id: "acc-cash",
        to_account_id: "acc-recv",
      }),
      makeTransaction({
        id: "t-buy",
        kind: "buy",
        account_id: "acc-cash",
        funding_account_id: "acc-recv",
      }),
      makeTransaction({
        id: "t-sell",
        kind: "sell",
        account_id: "acc-cash",
        funding_account_id: "acc-recv",
      }),
      makeTransaction({ id: "t-single", kind: "expense", account_id: "acc-cash" }),
    ];
    const [transfer, buy, sell, single] = pre(rows);
    expect(transfer.display.accountPrimaryId).toBe("acc-cash");
    expect(transfer.display.accountSecondaryId).toBe("acc-recv");
    expect(buy.display.accountPrimaryId).toBe("acc-recv");
    expect(buy.display.accountSecondaryId).toBe("acc-cash");
    expect(sell.display.accountPrimaryId).toBe("acc-cash");
    expect(sell.display.accountSecondaryId).toBe("acc-recv");
    expect(single.display.accountPrimaryId).toBe("acc-cash");
    expect(single.display.accountSecondaryId).toBeNull();
  });

  it("出资分解行标注：「等 N 账户」「同账户 N 笔」「按比例自动分解」与既有渲染同口径", () => {
    const rows = [
      makeTransaction({
        id: "t-multi",
        account_id: null,
        fundings: [
          makeFunding({ account_id: "acc-cash" }),
          makeFunding({ account_id: "acc-recv" }),
        ],
      }),
      makeTransaction({
        id: "t-same",
        account_id: null,
        fundings: [
          makeFunding({ account_id: "acc-cash" }),
          makeFunding({ account_id: "acc-cash" }),
        ],
      }),
      makeTransaction({
        id: "t-single",
        account_id: null,
        fundings: [makeFunding({ account_id: "acc-cash" })],
      }),
      makeTransaction({
        id: "t-derived",
        account_id: null,
        fundings: [
          makeFunding({ account_id: "acc-cash", derived: true }),
          makeFunding({ account_id: "acc-recv", derived: true }),
        ],
      }),
    ];
    const [multi, same, single, derived] = pre(rows);
    expect(multi.display.fundingMarker).toBe("等 2 账户");
    expect(multi.display.fundingDerivedMarker).toBeNull();
    expect(multi.display.accountPrimaryId).toBe("acc-cash");
    expect(same.display.fundingMarker).toBe("同账户 2 笔");
    expect(single.display.fundingMarker).toBeNull();
    expect(derived.display.fundingMarker).toBe("等 2 账户");
    expect(derived.display.fundingDerivedMarker).toBe("按比例自动分解");
  });

  it("商户：软删商户可点击（历史交易口径）、未知 id 不可点击，与 MerchantLink 裁决一致", () => {
    const rows = [
      makeTransaction({ id: "t-active", merchant_id: "mch-1" }),
      makeTransaction({ id: "t-deleted", merchant_id: "mch-del" }),
      makeTransaction({ id: "t-unknown", merchant_id: "mch-missing" }),
      makeTransaction({ id: "t-none" }),
    ];
    const [active, deleted, unknown, none] = pre(rows);
    expect(active.display.merchantName).toBe("京东");
    expect(active.display.merchantClickable).toBe(true);
    expect(deleted.display.merchantName).toBe("已闭店商城");
    expect(deleted.display.merchantClickable).toBe(true);
    expect(unknown.display.merchantName).toBeNull();
    expect(unknown.display.merchantClickable).toBe(false);
    expect(none.display.merchantName).toBeNull();
    expect(none.display.merchantClickable).toBe(false);
  });

  it("来源三态：有效/已取消可点击、软删保单不可点击，展示名与状态标注与 SourceLink 同口径", () => {
    const rows = [
      makeTransaction({
        id: "t-policy",
        source: { kind: "policy", entity_id: "pol-1", display_name: "重疾险", status: null },
      }),
      makeTransaction({
        id: "t-cancelled",
        source: {
          kind: "subscription",
          entity_id: "sub-1",
          display_name: "流媒体会员",
          status: "cancelled",
        },
      }),
      makeTransaction({
        id: "t-deleted",
        source: { kind: "policy", entity_id: "pol-2", display_name: "医疗险", status: "deleted" },
      }),
    ];
    const [policy, cancelled, deleted] = pre(rows);
    expect(policy.display.sourceName).toBe("重疾险");
    expect(policy.display.sourceStatusLabel).toBeNull();
    expect(policy.display.sourceClickable).toBe(true);
    expect(cancelled.display.sourceStatusLabel).toBe("已取消");
    expect(cancelled.display.sourceClickable).toBe(true);
    expect(deleted.display.sourceStatusLabel).toBe("已删除");
    expect(deleted.display.sourceClickable).toBe(false);
  });

  it("金额：转换行转出金额、份额调整空值占位符、数字分组与隐私掩码，与 displayAmountText 单源一致", () => {
    const convert = makeTransaction({
      id: "t-convert",
      kind: "convert",
      amount_native_cents: 359062,
      convert: {
        to_instrument_id: "inst-in",
        to_symbol: "519700",
        to_quantity: 10,
        out_amount_cents: 361561,
        in_amount_cents: 361561,
      },
    });
    const split = makeTransaction({ id: "t-split", kind: "split" });
    const grouped = makeTransaction({ id: "t-grouped", amount_native_cents: 1234567 });
    const [convertRow, splitRow, groupedRow] = pre([convert, split, grouped]);
    expect(convertRow.display.amountText).toBe(displayAmountText(refStub, convert));
    expect(convertRow.display.amountText).toBe(formatAmount(361561, undefined));
    expect(splitRow.display.amountText).toBe("-");
    expect(groupedRow.display.amountText).toBe(formatAmount(1234567, undefined));
    // 金额语义色：随主题与 kindSemanticColor 单源一致（主题切换即时换色由
    // 视图 computed 重派生承接，此处锁定单源口径）
    const app = useAppStore();
    app.setTheme("light");
    const lightRow = pre([convert])[0];
    expect(lightRow.display.amountColor).toBe(kindSemanticColor("convert", "light"));
    app.setTheme("dark");
  });

  it("金额隐私模式：开启后预计算金额为恒形掩码，关闭后恢复真实数字", () => {
    const row = makeTransaction({ id: "t-privacy", amount_native_cents: 123456 });
    const before = pre([row])[0].display.amountText;
    amountPrivacyEnabled.value = true;
    const masked = pre([row])[0].display.amountText;
    expect(masked).toBe("••••");
    amountPrivacyEnabled.value = false;
    const restored = pre([row])[0].display.amountText;
    expect(restored).toBe(before);
    expect(masked).not.toBe(restored);
  });

  it("分类路径：多级路径、软删分类历史口径与未知/无分类 '-' 兜底，与 categoryPath 单源一致", () => {
    const rows = [
      makeTransaction({ id: "t-multi", category_id: "cat-food" }),
      makeTransaction({ id: "t-softdel", category_id: "cat-del" }),
      makeTransaction({ id: "t-unknown", category_id: "cat-missing" }),
      makeTransaction({ id: "t-none", category_id: null }),
    ];
    const [multi, softdel, unknown, none] = pre(rows);
    expect(multi.display.categoryPath).toBe("生活 > 餐饮");
    expect(softdel.display.categoryPath).toBe("已删分类 > 历史");
    expect(unknown.display.categoryPath).toBe("-");
    expect(none.display.categoryPath).toBe("-");
  });

  it("购买项展开行：每购买项一行、行序 = 交易序，件数标注与购买项分类随行预计算", () => {
    const tx = makeTransaction({
      id: "t-order",
      category_id: "cat-food",
      purchases: [
        makePurchase({ name: "猫粮", quantity: 2, category_id: "cat-pet" }),
        makePurchase({ name: "洗衣液", quantity: 1 }),
      ],
    });
    const plain = makeTransaction({ id: "t-plain", note: "普通行" });
    const computed = pre([tx, plain], { expandPurchases: true });
    expect(computed).toHaveLength(3);
    expect(computed.map((r) => r.purchaseIndex)).toEqual([0, 1, -1]);
    // 购买项行：分类逐行取购买项自己的分类、件数标注随行
    expect(computed[0].display.categoryPath).toBe("生活 > 宠物");
    expect(computed[0].display.purchaseQuantityLabel).toBe("共 2 件");
    expect(computed[1].display.categoryPath).toBe("-");
    expect(computed[1].display.purchaseQuantityLabel).toBe("共 1 件");
    // 订单级字段随行继承（rowSpan 合并格消费交易级 display）
    expect(computed[0].display.kindLabel).toBe(computed[1].display.kindLabel);
    expect(computed[0].display.amountText).toBe(computed[1].display.amountText);
    // 非购买项行继承交易级展示（普通行分类为空 → '-' 兜底）
    expect(computed[2].display.categoryPath).toBe("-");
    expect(computed[2].display.purchaseQuantityLabel).toBeUndefined();
  });

  it("缺省未展开：每交易一行，零展开零变化", () => {
    const tx = makeTransaction({
      id: "t-order",
      purchases: [makePurchase({ name: "猫粮" })],
    });
    const computed = pre([tx]);
    expect(computed).toHaveLength(1);
    expect(computed[0].purchaseIndex).toBe(-1);
    expect(computed[0].display.purchaseQuantityLabel).toBeUndefined();
  });
});
