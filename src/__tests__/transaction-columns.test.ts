import { describe, expect, it, vi } from "vitest";
import { NTag, type DataTableColumn } from "naive-ui";
import type { VNode } from "vue";
import { mount } from "@vue/test-utils";
import { darkTheme, lightTheme, NConfigProvider } from "naive-ui";
import { darkOverrides, lightOverrides } from "@ledger/theme/overrides";
import {
  buildTransactionColumns,
  expandPurchaseRows,
  renderPurchaseLine,
  rowActionsColumn,
} from "@/transaction/transaction-columns";
import {
  kindTagStyle,
  statusTagStyle,
  withTransactionCells,
  type ReferenceStore,
  type TransactionCellRow,
} from "@/transaction/transaction-cells";
import AmountCell from "@/transaction/AmountCell.vue";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { TRANSACTION_KINDS, type Transaction, type TransactionSource } from "@ledger/types";
import { formatAmount } from "@ledger/money";
import { makePurchase, makeTransaction } from "./factories";

/** 交易列配置装配单测（#2010 后形态）：列渲染消费预计算模型（withTransactionCells
 * 装配行）产出纯元素——本文件只测**列配置的装配面**：单元格产物结构（data-action
 * 委托载荷 / 平替类 / title 悬停全文）、行输入轴分支、rowSpan 合并与操作列回调；
 * 逐格文案口径归 transaction-cells.test.ts（预计算单测），渲染期零格式化的计数
 * 守门归 perf/render-gate.test.ts。 */

const reference = {
  categoryPath: () => null,
  accountMap: new Map(),
  merchantMap: new Map(),
  getCurrency: () => undefined,
} as unknown as ReferenceStore;

/** 预装配行（默认暗色主题；列渲染只消费行上的 cells，不再读 reference）。 */
function cellRows(rows: Transaction[], theme: "dark" | "light" = "dark"): TransactionCellRow[] {
  return withTransactionCells(rows, reference, theme);
}

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

function amountColumn(options: Parameters<typeof buildTransactionColumns>[0] = {}) {
  return renderColumnOf(buildTransactionColumns(options), "amount_native_cents");
}

describe("buildTransactionColumns 金额单元格语义着色（消费预计算模型）", () => {
  it("指针轴：纯 span（无子组件实例），携带预计算文案与语义色（暗/亮逐类型）", () => {
    const app = useAppStore();
    for (const theme of ["dark", "light"] as const) {
      app.setTheme(theme);
      const render = amountColumn({ inputMode: { value: "pointer" } });
      for (const kind of TRANSACTION_KINDS) {
        const [row] = cellRows([makeTransaction({ id: `tx-${kind}`, kind })], theme);
        const vnode = render(row, 0) as VNode;
        expect(vnode.type).toBe("span"); // #2010 验收②：金额格零子组件实例
        expect((vnode.props as { class?: string }).class).toContain("amount-cell");
        expect((vnode.children as string).length).toBeGreaterThan(0);
        expect((vnode.props as { style: { color: string } }).style.color).toBe(
          kindSemanticColor(kind, theme),
        );
      }
    }
  });

  it("触控轴：保留 AmountCell 点按气泡（文案与色同为预计算载荷）", () => {
    const [row] = cellRows([makeTransaction({ id: "tx", kind: "expense" })]);
    const vnode = amountColumn({ inputMode: { value: "touch" } })(row, 0) as VNode;
    expect(vnode.type).toBe(AmountCell);
    expect((vnode.props as { text: string }).text).toBe(
      formatAmount(10000, reference.getCurrency(row.currency_code)),
    );
  });

  it("金额文案单点：单元格文本 = displayAmountText 产物（转换行转出金额 / split 空值 '-'）", () => {
    const convert = makeTransaction({
      id: "tx-convert",
      kind: "convert",
      // 行金额锚点 = 结转成本（3590.62），展示口径 = 转出金额（3615.61，ADR-0099）。
      amount_native_cents: 359062,
      convert: {
        to_instrument_id: "inst-in",
        to_symbol: "519700",
        to_quantity: 10,
        out_amount_cents: 361561,
        in_amount_cents: 361561,
      },
    });
    const split = makeTransaction({ id: "tx-split", kind: "split", amount_native_cents: 0 });
    const rows = cellRows([convert, split]);
    const render = amountColumn({ inputMode: { value: "pointer" } });
    expect((render(rows[0], 0) as VNode).children).toBe(
      formatAmount(361561, reference.getCurrency("CNY")),
    );
    expect((render(rows[1], 1) as VNode).children).toBe("-");
  });

  it("主题切换整表换色：换主题重建行集（视图 computed 的数据到达路径）即取新主题色", () => {
    const row = { txn: makeTransaction({ id: "tx-expense", kind: "expense" }) };
    const render = amountColumn({ inputMode: { value: "pointer" } });
    const dark = render(cellRows([row.txn], "dark")[0], 0) as VNode;
    const light = render(cellRows([row.txn], "light")[0], 0) as VNode;
    expect((dark.props as { style: { color: string } }).style.color).toBe(
      kindSemanticColor("expense", "dark"),
    );
    expect((light.props as { style: { color: string } }).style.color).toBe(
      kindSemanticColor("expense", "light"),
    );
  });
});

/** 来源列（spec #704 / issue #706）：列位置与平替产物——图标 + 名称按钮（data-action
 * 委托载荷）+ 状态标注；跳转目标计算与谓词注入归 source-jump / 表格行为委托测试
 *（一缝一测），软删保单不可点击的呈现矩阵在此钉住。 */
describe("buildTransactionColumns 来源列", () => {
  function sourceCellOf(row: Transaction) {
    return renderColumnOf(buildTransactionColumns({ inputMode: { value: "pointer" } }), "source")(
      row,
      0,
    ) as VNode;
  }

  it("列序：来源列位于账户之后、备注之前", () => {
    const keys = buildTransactionColumns({}).map((c) => (c as { key?: string }).key);
    expect(keys.indexOf("account_id")).toBeLessThan(keys.indexOf("source"));
    expect(keys.indexOf("source")).toBeLessThan(keys.indexOf("note"));
  });

  it("保单来源平替：图标 + 名称按钮（委托载荷 kind/entity）+ 类型全称 title", () => {
    const source: TransactionSource = {
      kind: "policy",
      entity_id: "pol-1",
      display_name: "重疾险",
      status: null,
    };
    const vnode = sourceCellOf(cellRows([makeTransaction({ id: "t1", source })])[0]);
    expect((vnode.props as { title: string }).title).toBe("保单");
    const children = vnode.children as VNode[];
    const button = children[1];
    expect(button.type).toBe("button");
    expect(button.props).toMatchObject({
      "data-action": "source",
      "data-kind": "policy",
      "data-entity": "pol-1",
    });
    expect(button.children).toBe("重疾险");
  });

  it("软删保单：名称退纯文本（无委托载荷）、带状态标注文案", () => {
    const source: TransactionSource = {
      kind: "policy",
      entity_id: "pol-2",
      display_name: "医疗险",
      status: "deleted",
    };
    const vnode = sourceCellOf(cellRows([makeTransaction({ id: "t2", source })])[0]);
    const children = vnode.children as VNode[];
    expect(children[1].type).toBe("span");
    expect((children[1].props as Record<string, unknown>)["data-action"]).toBeUndefined();
    const statusTag = children.at(-1) as VNode;
    expect(statusTag.props).toMatchObject({ class: "tx-status-tag" });
    expect(statusTag.children).toBe("已删除");
  });

  it("无来源留空（占位符，手动/AI 导入口径）", () => {
    expect(sourceCellOf(cellRows([makeTransaction({ id: "t3" })])[0])).toBe("-");
  });
});

/** 备注列（显式复制通道，见「界面文本不可选」词条）：平替结构——原生 CSS ellipsis
 *（.tx-ellipsis + title 悬停全文，#2010 验收②）+ 复制按钮委托载荷（验收③）；
 * 复制动作与 toast 归 transaction-table-actions 测试（一缝一测）。 */
describe("buildTransactionColumns 备注列", () => {
  function noteCellOf(row: Transaction) {
    return renderColumnOf(buildTransactionColumns({ inputMode: { value: "pointer" } }), "note")(
      row,
      0,
    );
  }

  it("无备注渲染占位符，不渲染复制按钮（空备注无可复制）", () => {
    expect(noteCellOf(cellRows([makeTransaction({ id: "t1", note: null })])[0])).toBe("-");
  });

  it("有备注：文本自省略 span（title 悬停全文）与复制按钮并排，按钮携带完整备注载荷", () => {
    const vnode = noteCellOf(
      cellRows([makeTransaction({ id: "t2", note: "视频会员月费" })])[0],
    ) as VNode;
    expect((vnode.props as { class: string }).class).toContain("tx-note-cell");
    const children = vnode.children as VNode[];
    expect(children).toHaveLength(2);
    expect(children[0].props).toMatchObject({ class: "tx-ellipsis", title: "视频会员月费" });
    expect(children[0].children).toBe("视频会员月费");
    expect(children[1].props).toMatchObject({
      "data-action": "copy-note",
      "data-note": "视频会员月费",
    });
    expect(children[1].props).toMatchObject({ class: "note-copy-btn tx-copy-btn" });
  });
});

/** 交易行「⋯」常显操作列（ADR-0088 决策 6 / issue #843）：平替真按钮（#2010 逐行
 * NButton 实例退役）；回调随按钮携行，菜单项集合与动作分派归 TransactionsView
 * 组件测试（行菜单编排接缝，一缝一测）。 */
describe("buildTransactionColumns 操作列（「⋯」常显第二入口）", () => {
  const row = makeTransaction({ id: "t-actions", kind: "expense" });

  it("未声明 onRowMenuOpen 不追加操作列（搜索结果保持只读）", () => {
    const keys = buildTransactionColumns({}).map((c) => (c as { key?: string }).key);
    expect(keys).not.toContain("actions");
  });

  it("声明 onRowMenuOpen 追加末位操作列：真按钮「⋯」，点击以事件与目标行回调", () => {
    const onRowMenuOpen = vi.fn();
    const columns = buildTransactionColumns({ onRowMenuOpen });
    const actions = columns.find((c) => (c as { key?: string }).key === "actions");
    expect(actions).toBeTruthy();
    const keys = columns.map((c) => (c as { key?: string }).key);
    expect(keys[keys.length - 1]).toBe("actions");
    const vnode = (actions as unknown as { render: (row: Transaction) => VNode }).render(row);
    expect(vnode.type).toBe("button");
    expect(vnode.children).toBe("⋯");
    const props = vnode.props as {
      class: string;
      "aria-label": string;
      onClick: (e: MouseEvent) => void;
    };
    expect(props.class).toContain("row-actions-btn");
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
    return cellRows(
      expandPurchaseRows([
        makeTransaction({
          id: "t-order",
          purchases: [
            makePurchase({ name: "猫粮", quantity: 1 }),
            makePurchase({ name: "洗衣液", quantity: 2 }),
            makePurchase({ name: "纸巾", quantity: 1 }),
          ],
        }),
        makeTransaction({ id: "t-plain", note: "普通行" }),
      ]),
    );
  }

  function rowSpanOf(columns: DataTableColumn<Transaction>[], key: string) {
    const hit = columns.find((c) => (c as { key?: unknown }).key === key);
    expect(hit, `列 ${key} 应存在`).toBeTruthy();
    return (hit as { rowSpan?: (row: Transaction, index: number) => number }).rowSpan;
  }

  it("缺省（未声明 expandPurchases）任何列都不带 rowSpan，备注列名不变——未展开行集零变化", () => {
    const columns = buildTransactionColumns({ onRowMenuOpen: () => {} });
    for (const key of [...ORDER_LEVEL_KEYS, "category_id", "note"]) {
      expect(rowSpanOf(columns, key)).toBeUndefined();
    }
    const note = columns.find((c) => (c as { key?: unknown }).key === "note");
    expect((note as { title?: string }).title).toBe("备注");
  });

  it("展开装配：订单级列带 rowSpan（首行 = 件数、续行与普通行 = 1）与合并格居中类", () => {
    const columns = buildTransactionColumns({
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
    const columns = buildTransactionColumns({ expandPurchases: true });
    expect(rowSpanOf(columns, "category_id")).toBeUndefined();
    expect(rowSpanOf(columns, "note")).toBeUndefined();
  });

  it("展开装配：备注列名「商品 / 备注」", () => {
    const columns = buildTransactionColumns({ expandPurchases: true });
    const note = columns.find((c) => (c as { key?: unknown }).key === "note");
    expect((note as { title?: string }).title).toBe("商品 / 备注");
  });

  it("购买项单元格：商品名自省略 span（title 悬停全文）+ 件数标注，无复制按钮", () => {
    const render = renderColumnOf(buildTransactionColumns({ expandPurchases: true }), "note");
    const rows = expandedRows();
    const vnode = render(rows[1], 1) as VNode;
    const children = vnode.children as VNode[];
    expect(children).toHaveLength(2);
    expect(children[0].props).toMatchObject({ class: "tx-ellipsis", title: "洗衣液" });
    expect(children[1].children).toBe("共 2 件");
    expect(JSON.stringify(children)).not.toContain("copy-note");
  });

  it("全文形态（卡片 / 详情）整名渲染、自然换行；件数与分类标注共用同一单点拼法", () => {
    const item = makePurchase({
      name: "超长商品名『进口无谷深海鱼油成猫粮专用 10kg 装大袋』",
      quantity: 3,
    });
    const full = renderPurchaseLine(item);
    const fullChildren = full.children as VNode[];
    expect(fullChildren[0].children as string).toContain("10kg 装大袋』");
    expect(fullChildren).toHaveLength(2);
    // 件数标注：与预计算模型（cells.note.qtyText）同一拼法单源
    expect((fullChildren.at(-1) as VNode).children).toBe("共 3 件");
    // 详情形态：分类标注插入在件数标注之前（名称 / 分类 / 件数顺序）
    const withCategory = renderPurchaseLine(item, { suffix: "生活 > 宠物" });
    const children = withCategory.children as VNode[];
    expect(children).toHaveLength(3);
    expect(children[1].children as string).toBe("生活 > 宠物");
  });

  it("展开视图下非购买项行仍走备注形态（有备注带复制按钮载荷）", () => {
    const render = renderColumnOf(buildTransactionColumns({ expandPurchases: true }), "note");
    const rows = expandedRows();
    const vnode = render(rows[3], 3) as VNode;
    const children = vnode.children as VNode[];
    expect(children[0].props).toMatchObject({ class: "tx-ellipsis", title: "普通行" });
    expect(children[1].props).toMatchObject({ "data-action": "copy-note" });
  });

  it("rowActionsColumn 缺省不带合并装配（投资明细页签零变化）", () => {
    const col = rowActionsColumn<Transaction>(() => {});
    expect((col as { rowSpan?: unknown }).rowSpan).toBeUndefined();
    expect((col as { className?: unknown }).className).toBeUndefined();
  });
});

/** NTag 平替色板守门（#2010）：kindTagStyle / statusTagStyle 与真实 NTag 在
 * 「naive 基础主题 ⊕ 应用 overrides」下的内联 CSS 变量逐值一致——naive 升级改
 * 色板 / 应用改 overrides 即红，防止平替样式静默漂移（结构化 style 断言取自
 * NTag 渲染根元素的 --n-* 内联变量）。 */
describe("NTag 平替色板与真实 NTag 逐值一致（守门）", () => {
  const CASES = [
    { theme: "dark" as const, themeObj: darkTheme, overrides: darkOverrides },
    { theme: "light" as const, themeObj: lightTheme, overrides: lightOverrides },
  ];

  /** 以 config-provider 包裹挂载真实 NTag（theme / themeOverrides 与 App.vue 同参）。 */
  function mountTag(
    type: "success" | "warning" | "info" | "default",
    caseItem: (typeof CASES)[number],
  ) {
    return mount(
      {
        components: { NConfigProvider, NTag },
        template: `<NConfigProvider :theme="theme" :theme-overrides="overrides">
          <NTag :type="tagType" :bordered="true" data-testid="tag">x</NTag>
        </NConfigProvider>`,
        data: () => ({
          theme: caseItem.themeObj,
          overrides: caseItem.overrides,
          tagType: type === "default" ? undefined : type,
        }),
      },
      { attachTo: document.body },
    );
  }

  for (const caseItem of CASES) {
    it(`${caseItem.theme}：四语义色型平替样式 == NTag 内联 CSS 变量`, () => {
      for (const type of ["default", "success", "warning", "info"] as const) {
        const wrapper = mountTag(type, caseItem);
        const el = wrapper.find('[data-testid="tag"]').element as HTMLElement;
        const style = el.style;
        const mine = kindTagStyle(type, caseItem.theme);
        expect(style.getPropertyValue("--n-text-color"), `${caseItem.theme}/${type}`).toBe(
          String(mine.color),
        );
        expect(style.getPropertyValue("--n-color"), `${caseItem.theme}/${type}`).toBe(
          String(mine.background),
        );
        expect(style.getPropertyValue("--n-border"), `${caseItem.theme}/${type}`).toBe(
          `1px solid ${String(mine.borderColor)}`,
        );
        expect(style.getPropertyValue("--n-border-radius"), `${caseItem.theme}/${type}`).toBe(
          String(mine.borderRadius),
        );
        wrapper.unmount();
      }
    });

    it(`${caseItem.theme}：状态标注平替样式 == small 无边框 NTag 内联变量`, () => {
      const wrapper = mount(
        {
          components: { NConfigProvider, NTag },
          template: `<NConfigProvider :theme="theme" :theme-overrides="overrides">
            <NTag size="small" :bordered="false" data-testid="tag">已取消</NTag>
          </NConfigProvider>`,
          data: () => ({
            theme: caseItem.themeObj,
            overrides: caseItem.overrides,
          }),
        },
        { attachTo: document.body },
      );
      const el = wrapper.find('[data-testid="tag"]').element as HTMLElement;
      const mine = statusTagStyle(caseItem.theme);
      expect(el.style.getPropertyValue("--n-text-color")).toBe(String(mine.color));
      expect(el.style.getPropertyValue("--n-color")).toBe(String(mine.background));
      expect(el.style.getPropertyValue("--n-border-radius")).toBe(String(mine.borderRadius));
      wrapper.unmount();
    });
  }
});
