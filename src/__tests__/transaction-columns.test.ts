import { describe, expect, it, vi } from "vitest";
import { NEllipsis, type DataTableColumn } from "naive-ui";
import type { VNode } from "vue";
import {
  buildTransactionColumns,
  expandPurchaseRows,
  renderPurchaseLine,
  rowActionsColumn,
  type ReferenceStore,
} from "@/transaction/transaction-columns";
import SourceLink from "@/transaction/SourceLink.vue";
import NoteCopyButton from "@ledger/ui-kit/NoteCopyButton.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { TRANSACTION_KINDS, type Transaction, type TransactionSource } from "@ledger/types";
import { formatAmount } from "@ledger/money";
import { makePurchase, makeTransaction } from "./factories";

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
    expect(children[0].type).toBe(NEllipsis);
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

  it("缺省（未声明 expandPurchases）任何列都不带 rowSpan，备注列名不变——搜索结果零变化", () => {
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
    expect(children[0].type).toBe(NEllipsis);
    expect((children[1].children as string).length).toBeGreaterThan(0);
  });

  it("截断形态（列表）与全文形态（卡片 / 详情）经同一渲染单点分形：NEllipsis vs 整名渲染", () => {
    const item = makePurchase({
      name: "超长商品名『进口无谷深海鱼油成猫粮专用 10kg 装大袋』",
      quantity: 3,
    });
    // 列表形态：名内 NEllipsis（省略 + 悬停全文）+ 件数标注
    const truncated = renderPurchaseLine(item, { truncate: true });
    const truncChildren = truncated.children as VNode[];
    expect(truncChildren[0].type).toBe(NEllipsis);
    expect(truncChildren).toHaveLength(2);
    // 全文形态（卡片 / 详情）：整名渲染、自然换行——触控轴无悬停，详情是全文出口
    const full = renderPurchaseLine(item);
    const fullChildren = full.children as VNode[];
    expect(fullChildren[0].type).not.toBe(NEllipsis);
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
    expect(children[0].type).toBe(NEllipsis);
    expect(children[1].type).toBe(NoteCopyButton);
  });

  it("rowActionsColumn 缺省不带合并装配（投资明细页签零变化）", () => {
    const col = rowActionsColumn<Transaction>(() => {});
    expect((col as { rowSpan?: unknown }).rowSpan).toBeUndefined();
    expect((col as { className?: unknown }).className).toBeUndefined();
  });
});
