// 交易列配置共享模块（Issue 39 Prefactor，#2010 渲染性能重构）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 单元格文本口径与预计算模型住 transaction-cells（#2010 单点）；本模块只做**渲染**——
// 渲染函数消费行上预装配的 `cells`（withTransactionCells，数据到达时完成），产出
// 纯元素 VNode（span/button + 全局类），渲染期零格式化、零行内组件实例化
// （类型标签 / 商户 / 账户 / 来源 / 备注 / 金额格，#2010 验收②）。
//
// 行级事件委托（#2010 验收③）：账户/商户/来源下钻与备注复制不再逐格挂组件，
// 元素只带 `data-action`（+ data-id / data-kind / data-entity / data-note 载荷）；
// 表格级行为由单一 click 监听器分发——视图以 useTransactionTableActions() 装配后
// 绑在包裹 NDataTable 的壳层 div 上（.tx-table-shell，同时携带主题 CSS 变量，
// transactionTableShellVars）。视觉（强调色 / 下划线 / hover）交 CSS 平替类
// （global.css `.tx-*`）；键盘可达性由真 <button> 承载（Tab 聚焦 + Enter 触发，
// AccountLink 同款语义）。

import { h, type VNode } from "vue";
import { type DataTableBaseColumn, type DataTableColumn } from "naive-ui";
import { CopyOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import type { Transaction, TransactionPurchase } from "@ledger/types";
import AccountLink from "@/accounts/AccountLink.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import type { InputMode } from "@/composables/useInputMode";
import { CELL_ACTIONS } from "@/transaction/transaction-table-actions";
import {
  purchaseIndexOf,
  purchaseQtyText,
  SOURCE_KIND_ICONS,
  type AccountAtom,
  type AccountCellView,
  type ReferenceStore,
  type SourceCellView,
  type TransactionCellRow,
  type TransactionCells,
} from "@/transaction/transaction-cells";
import { accountCellView } from "@/transaction/transaction-cells";

// —— 单源转出口（消费方既有导入面不变，#2010 迁移零波及） ——
export {
  KIND_TAG_TYPE,
  kindLabel,
  displayAmountText,
  purchaseIndexOf,
  type KindTagType,
  type ReferenceStore,
} from "@/transaction/transaction-cells";

/** 展开后的显示行（issue #1883 / ADR-0138 决策 13）：购买项行由主交易行派生——
 * 同一交易带 `purchaseIndex` 复制 N 份（N = 购买项数），无购买项交易复制 1 份
 * （purchaseIndex = -1，呈现零变化）。派生行不是独立排序/分页单元：行序 = 交易序，
 * 同单内按购买项顺序位相邻（分页、total、筛选与排序仍按交易计，ADR-0138 决策 14）。 */
export interface ExpandedTransactionRow extends Transaction {
  /** 本行对应的购买项序位（0 基，对账单顺序）；无购买项行 = -1 */
  purchaseIndex: number;
}

/** 交易行集 → 展开显示行集（列表视图侧唯一接线点）：有购买项的交易每项一行，
 * 其余交易原样一行。浅拷贝保留行只读消费语义（菜单/弹窗读字段，不持行身份）。 */
export function expandPurchaseRows(rows: Transaction[]): ExpandedTransactionRow[] {
  return rows.flatMap((row) =>
    row.purchases.length === 0
      ? [{ ...row, purchaseIndex: -1 }]
      : row.purchases.map((_, purchaseIndex) => ({ ...row, purchaseIndex })),
  );
}

/** 显示行键（naive-ui `row-key`，展开行集的单一收口）：交易 id +「购买项序位」。
 *
 * 必须唯一到**显示行**：展开后同一交易派生多行、行 id 相同，只用 id 会撞键。
 * 而漏传 `row-key`（或键取不到）的后果不只是 Vue 的重复键警告——naive 的 hover 判据是
 * `hoverKey !== null && cordKey[r][c].includes(hoverKey)`，行键为 `undefined` 时
 * `undefined !== null` 为真、合并格的 cordKey 数组里也全是 `undefined`，于是**每个
 * rowspan 合并格都被贴上 `-td--hover`（hover 底色）常亮**：合并格底色与普通行不同，
 * 且该底色与 naive 的行线色接近，相邻两单的合并格连成一条浅色带、块间分隔线被淹没。 */
export function displayRowKey(row: Transaction): string {
  return `${row.id}:${purchaseIndexOf(row)}`;
}

/** 订单级格跨度（naive-ui 列 rowSpan 回调）：订单块首行 = 购买项数（向下合并），
 * 其余行 = 1（被合并覆盖的格子由表格按坐标跳过，不渲染）。 */
function orderLevelRowSpan(row: Transaction): number {
  return purchaseIndexOf(row) === 0 ? Math.max(row.purchases.length, 1) : 1;
}

/** 订单块行类（ADR-0138 决策 13 的呈现边界）：购买项行标 `block-row`、块尾行另标 `block-last`。
 * 样式（global.css）据此抹掉**块内**行线——订单级列纵向合并后，块内行线只覆盖逐行的
 * 「分类 / 商品·备注」两列，这条短行线把一单切成好几行、块边界反而看不出来；抹掉它，
 * 块尾那条整宽行线（合并格 + 本行）就是唯一的订单边界。类只挂在展开出的购买项行上，
 * 普通行 / 未展开行集零影响。 */
const PURCHASE_BLOCK_ROW_CLASS = "purchase-block-row";
const PURCHASE_BLOCK_LAST_CLASS = "purchase-block-last";

/** 行的订单块类（rowProps 单一收口）：非购买项行 undefined（普通行零变化），
 * 购买项行 = 块内行 + 块尾标记（单件订单首行即末行，块内规则因 block-last 不命中、行线保持原样）。 */
export function purchaseRowClass(row: Transaction): string | undefined {
  const index = purchaseIndexOf(row);
  if (index < 0) return undefined;
  return index === row.purchases.length - 1
    ? `${PURCHASE_BLOCK_ROW_CLASS} ${PURCHASE_BLOCK_LAST_CLASS}`
    : PURCHASE_BLOCK_ROW_CLASS;
}

/** 订单级合并格样式类（购买项展开，ADR-0138 决策 13）：rowspan 单元格内容纵向居中，
 * 样式收口 global.css（不依赖浏览器 UA 对 td 的默认对齐）。 */
const PURCHASE_MERGED_CELL_CLASS = "purchase-merged-cell";

/** 购买项行渲染单点（issue #1884 / ADR-0138 决策 15：移动卡片 / 只读详情共用，
 * 不自建第二套）——商品名 +「共 N 件」件数标注，全文形态（整名渲染、自然换行——
 * 触控轴没有悬停，详情是长商品名的全文出口）；桌面列表的截断形态随 #2010 改为
 * 预计算模型 + 原生 CSS ellipsis（.tx-ellipsis），不再经本函数分形。
 * 只读呈现：无复制按钮（备注复制通道不适用商品名）、无价格（存而不显示，决策 10）；
 * `suffix` 额外弱化标注（详情的分类路径）插在件数标注之前。 */
export function renderPurchaseLine(
  item: TransactionPurchase,
  opts: { suffix?: string } = {},
): VNode {
  return h("div", { class: "tx-note-cell" }, [
    h("span", { style: "flex: 1 1 auto; min-width: 0; word-break: break-word;" }, item.name),
    ...(opts.suffix ? [h("span", { style: PURCHASE_QTY_STYLE }, opts.suffix)] : []),
    h("span", { style: PURCHASE_QTY_STYLE }, purchaseQtyText(item)),
  ]);
}

/** 购买项件数标注弱化样式（与账户列「等 N 账户」同型，单源样式串）。 */
const PURCHASE_QTY_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

/** 行的预计算单元格模型（单一收口）：行集经 withTransactionCells 装配后恒在场；
 * 未装配（表格 :data 传了原始行集）属接线错误——开发期尽早抛出，不静默回退
 * （静默回退 = 渲染期重建模型的性能劣化形态，#2012 计数守门的对立形态）。 */
function cellsOf(row: Transaction): TransactionCells {
  const cells = (row as Partial<TransactionCellRow>).cells;
  if (import.meta.env.DEV && cells === undefined) {
    throw new Error(
      "行缺少预计算单元格模型：表格 :data 行集须经 withTransactionCells 装配（#2010）",
    );
  }
  return cells as TransactionCells;
}

/** 列级标签常量（构建期取词，随语言切换由 computed 列数组重建）：渲染期零 t() 调用
 * （#2012 计数守门①项收紧到 0 的组成之一——悬停提示 / 无障碍标签全部前移）。 */
interface CellLabelPack {
  viewAccount: string;
  viewMerchant: string;
  copyNote: string;
}

/** 账户格三形态布局骨架（单一收口，#2010）：形态判定与标注文案来自 accountCellView
 * 模型，桌面平替与移动档链接组件注入各自的**可下钻原子渲染器**共用本骨架——
 * 三形态/标注呈现两端不漂移；黑洞/隐藏账户占位「-」两端同形（纯文本，不下钻）。 */
function accountCellSkeleton(
  view: AccountCellView,
  linkAtom: (a: { accountId: string; name: string }) => VNode,
): VNode {
  const atom = (a: AccountAtom): VNode =>
    a.link ? linkAtom(a) : h("span", { class: "tx-link-placeholder" }, "-");
  if (view.form === "single") return atom(view.account);
  if (view.form === "two") {
    return h("div", { class: "tx-two-account" }, [
      atom(view.from),
      h("span", { class: "tx-account-arrow" }, "→"),
      atom(view.to),
    ]);
  }
  const line = h("div", { class: "tx-two-account" }, [
    atom(view.first),
    ...(view.marker ? [h("span", { class: "tx-dim-suffix" }, view.marker)] : []),
  ]);
  if (!view.derivedMarkerText) return line;
  return h("div", { class: "tx-funding-cell" }, [
    line,
    h("span", { class: "tx-derived-marker" }, view.derivedMarkerText),
  ]);
}

/** 账户格渲染（消费预计算模型；三形态语义与移动档 renderAccountCell 同一
 * accountCellView 单源，#2010 等价约束「双账户/分解行标注语义不变」）。 */
function renderAccountCellFast(view: AccountCellView, labels: CellLabelPack): VNode {
  return accountCellSkeleton(view, (a) =>
    h(
      "button",
      {
        type: "button",
        class: "tx-link",
        title: labels.viewAccount,
        "data-action": CELL_ACTIONS.account,
        "data-id": a.accountId,
      },
      a.name,
    ),
  );
}

/** 来源格渲染（消费预计算模型；语义与 SourceLink 组件同一 sourceCellView 单源）：
 * 图标 + 名称（可点击走委托，软删保单纯文本）+ 触控轴类型小字 + 状态标注平替。 */
function renderSourceCellFast(view: SourceCellView, isTouch: boolean): VNode {
  const name = view.clickable
    ? h(
        "button",
        {
          type: "button",
          class: "tx-link tx-source-link",
          "data-action": CELL_ACTIONS.source,
          "data-kind": view.kind,
          "data-entity": view.entityId,
        },
        view.name,
      )
    : h("span", { class: "tx-source-name" }, view.name);
  return h("span", { class: "tx-source-cell", title: view.typeTitle }, [
    h(SOURCE_KIND_ICONS[view.kind], { class: "tx-source-icon" }),
    name,
    ...(isTouch && view.hasDisplayName
      ? [h("span", { class: "tx-source-type-label" }, view.typeTitle)]
      : []),
    ...(view.statusLabel
      ? [h("span", { class: "tx-status-tag", style: view.statusStyle }, view.statusLabel)]
      : []),
  ]);
}

/** 备注格渲染（消费预计算模型）：省略走原生 CSS ellipsis（.tx-ellipsis）+ `title`
 * 悬停全文（#2010 验收②：NEllipsis 退役，超长文本悬停语义不回退）；复制按钮为
 * 平替元素（真 <button> + data-action 委托，悬停行显现仍由 .note-copy-btn 全局样式
 * 承载——组件化按钮与图标实例随行集消失，替代票面「悬停懒挂载」且更彻底）。 */
function renderNoteCellFast(cells: TransactionCells, labels: CellLabelPack): VNode | string {
  const note = cells.note;
  if (note.form === "empty") return "-";
  if (note.form === "purchase") {
    return h("div", { class: "tx-note-cell" }, [
      h("span", { class: "tx-ellipsis", title: note.name }, note.name),
      h("span", { class: "tx-dim-suffix" }, note.qtyText),
    ]);
  }
  return h("div", { class: "tx-note-cell" }, [
    h("span", { class: "tx-ellipsis", title: note.text }, note.text),
    h(
      "button",
      {
        type: "button",
        class: "note-copy-btn tx-copy-btn",
        title: labels.copyNote,
        "aria-label": labels.copyNote,
        "data-action": CELL_ACTIONS.copyNote,
        "data-note": note.text,
      },
      [h(CopyOutline)],
    ),
  ]);
}

/** buildTransactionColumns 可选装配面：调用方按需声明，缺省即纯只读列（搜索结果同款）。 */
export interface BuildTransactionColumnsOptions {
  /** 交易行「⋯」常显按钮的打开回调（ADR-0088 决策 6，issue #843）：传入即
   * 追加常显操作列，与行右键共用 RowContextMenu 同一 open 入口（账户行先例）；
   * 不传则不渲染该列（搜索结果无行菜单，保持只读）。 */
  onRowMenuOpen?: (event: MouseEvent, row: Transaction) => void;
  /** 购买项展开（issue #1883 / ADR-0138 决策 13）：声明即按「订单块逐购买项行」
   * 装配——订单级列（日期/类型/商户/来源/账户/金额/操作）rowSpan 纵向合并、
   * 分类列逐行取购买项分类、备注列一列两用。仅消费 expandPurchaseRows 展开行集的
   * 视图可声明（交易列表与搜索结果，后者自 #1885 起商品名命中即整单呈现）；未
   * 展开行集不声明，呈现零变化。 */
  expandPurchases?: boolean;
  /** 输入轴（useInputMode 唯一事实源）：金额格触控轴点按气泡（AmountCell）与来源格
   * 触控轴类型小字的渲染分支。缺省按触控轴兜底（保守方向：宁可多渲染一击可达
   * 替代，不漏触控语义，useInputMode 同款取舍）。 */
  inputMode?: { value: InputMode };
}

export function buildTransactionColumns(
  options: BuildTransactionColumnsOptions = {},
): DataTableColumn<Transaction>[] {
  // 列级标签常量（构建期取词，见 CellLabelPack 注）
  const labels: CellLabelPack = {
    viewAccount: t("common.link.viewAccount"),
    viewMerchant: t("common.link.viewMerchant"),
    copyNote: t("transactions.list.copyNote"),
  };
  // 订单级列合并装配（ADR-0138 决策 13）：展开视图给订单级列套 rowSpan 与
  // 合并格居中样式类；分类列、备注列不合并（分类逐行、备注两用）。
  const merged: Partial<Pick<DataTableBaseColumn<Transaction>, "rowSpan" | "className">> =
    options.expandPurchases
      ? { rowSpan: orderLevelRowSpan, className: PURCHASE_MERGED_CELL_CLASS }
      : {};
  const columns: DataTableColumn<Transaction>[] = [
    { title: t("transactions.columns.date"), key: "date", width: 105, ...merged },
    {
      title: t("transactions.columns.kind"),
      key: "kind",
      width: 65,
      ...merged,
      // 类型标签平替（#2010 验收②：不再实例化 NTag）：语义色随预计算样式对象
      // （kindTagStyle，主题/色型单点），布局度量收口 .tx-kind-tag。
      render: (row) => {
        const cells = cellsOf(row);
        return h("span", { class: "tx-kind-tag", style: cells.kindTagStyle }, cells.kindText);
      },
    },
    {
      title: t("transactions.columns.category"),
      key: "category_id",
      width: 150,
      // 布尔 ellipsis（CSS 类省略，无 NEllipsis 实例）：维持 fixed 布局（列宽纪律，
      // 见文件尾注）+ 悬停全文由内层 span 的 title 承载（#2010：原生 ellipsis 化）。
      ellipsis: true,
      // 分类列逐行显示（ADR-0138 决策 13）：展开视图下购买项行显示该购买项自己的分类、
      // 不做一致性合并（层级分类下没有任何一个真值能同时代表一单内的不同分类）；
      // 无购买项行与未展开视图仍显示交易行分类，呈现零变化。
      render: (row) => {
        const text = cellsOf(row).categoryText;
        return h("span", { class: "tx-ellipsis", title: text }, text);
      },
    },
    {
      title: t("transactions.columns.merchant"),
      key: "merchant_id",
      width: 120,
      ...merged,
      // 商户名平替按钮（可下钻，data-action 委托）+ 自身省略号兜底（AccountLink 同款，
      // 列级不设 ellipsis——NEllipsis 会把按钮包装成整体省略，破坏链接自身省略语义）。
      render: (row) => {
        const merchant = cellsOf(row).merchant;
        return merchant
          ? h(
              "button",
              {
                type: "button",
                class: "tx-link",
                title: labels.viewMerchant,
                "data-action": CELL_ACTIONS.merchant,
                "data-id": merchant.merchantId,
              },
              merchant.name,
            )
          : "-";
      },
    },
    {
      title: t("transactions.columns.account"),
      key: "account_id",
      width: 180,
      ...merged,
      render: (row) => renderAccountCellFast(cellsOf(row).account, labels),
    },
    {
      title: t("transactions.columns.source"),
      key: "source",
      width: 140,
      ...merged,
      // 来源格平替（语义与 SourceLink 组件同一 sourceCellView 单源）；无来源显示 "-"
      // （与商户/分类列同款占位惯例，#2030：原注释写「留空」与渲染不符，按可验证行为修正）；
      // 不设列级 ellipsis（账户列同款理由），超长由链接自身省略兜底。
      render: (row) => {
        const source = cellsOf(row).source;
        return source ? renderSourceCellFast(source, options.inputMode?.value === "touch") : "-";
      },
    },
    {
      // 列名两形态（ADR-0138 决策 13）：展开视图整列「商品 / 备注」（一列两用），
      // 未展开视图仍「备注」——列头与其内容形态一致。
      title: options.expandPurchases
        ? t("transactions.columns.itemNote")
        : t("transactions.columns.note"),
      key: "note",
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + title 悬停全文）；
      // 省略与悬停全文由单元格内 .tx-ellipsis 原生承担（NEllipsis 退役，#2010），
      // fixed 布局由分类列的布尔 ellipsis 维持。
      render: (row) => renderNoteCellFast(cellsOf(row), labels),
    },
    {
      title: t("transactions.columns.amount"),
      key: "amount_native_cents",
      width: 125,
      ...merged,
      // 金额按交易类型语义色着色（issue #435，kindSemanticColor 单源）：文案与色均来自
      // 预计算模型（随主题/隐私模式/语言整表重算）。指针轴纯 span（桌面零变化、
      // #2010 验收②）；触控轴保留 AmountCell 点按气泡（「点按查看」，issue #843）。
      render: (row) => {
        const cells = cellsOf(row);
        return options.inputMode?.value === "pointer"
          ? h(
              "span",
              { class: "amount-cell", style: { color: cells.amountColor } },
              cells.amountText,
            )
          : h(AmountCell, { text: cells.amountText, color: cells.amountColor });
      },
    },
  ];
  if (options.onRowMenuOpen) {
    columns.push(rowActionsColumn(options.onRowMenuOpen, merged));
  }
  return columns;
}

/**
 * 交易行「⋯」常显列（ADR-0088 决策 6，issue #843）：与右键共用同一行菜单编排
 * open 入口、以点击坐标弹出，全平台常显（账户行先例，桌面可见变化已裁决）；
 * 消费方（主交易列表与投资明细页签，issue #1781）各自传入回调，搜索结果不追加。
 * 泛型行：列渲染只承载行菜单入口，不读行内容。
 * #2010：NButton 平替为真 <button>（.tx-ghost-btn 全局类）——逐行按钮组件实例退役，
 * 无障碍标签在列构建期取词（渲染期零 t()）。
 */
export function rowActionsColumn<T>(
  onRowMenuOpen: (event: MouseEvent, row: T) => void,
  // 合并装配（issue #1883 / ADR-0138 决策 13）：购买项展开视图传入 rowSpan + 合并格
  // 样式类，操作列在订单块内只渲染一个；其余消费方（投资明细页签）不传，零变化。
  merge: Partial<Pick<DataTableBaseColumn<T>, "rowSpan" | "className">> = {},
): DataTableColumn<T> {
  const ariaLabel = t("transactions.menu.actions");
  return {
    title: t("transactions.columns.actions"),
    key: "actions",
    width: 64,
    ...merge,
    render: (row) =>
      h(
        "button",
        {
          type: "button",
          class: "row-actions-btn touch-hit-area tx-ghost-btn",
          "aria-label": ariaLabel,
          onClick: (e: MouseEvent) => onRowMenuOpen(e, row),
        },
        "⋯",
      ),
  };
}

// —— 移动档卡片列表消费面（#2010 桌面表格平替后保留组件路径：移动档行集有界
//（20/页）且为触控轴，组件交互（真链接组件、NTag 标签）零变化；口径经下方
// 共享单源函数与桌面同源，不产生第二套渲染口径） ——

/** 账户单元格渲染（移动档卡片与移动搜索结果共用）：消费 accountCellView 同一模型
 * 单源（三形态与标注文案不漂移），呈现层仍是 AccountLink 链接组件（下钻/键盘可达）。
 * 桌面表格自 #2010 起消费同一模型的平替元素渲染（renderAccountCellFast）。 */
export function renderAccountCell(reference: ReferenceStore, row: Transaction): VNode {
  // 可下钻原子 = AccountLink 链接组件（下钻/键盘可达；双账户行内 flex 样式随组件注入）
  return accountCellSkeleton(accountCellView(row, reference), (a) =>
    h(AccountLink, {
      accountId: a.accountId,
      style: "flex: 0 1 auto; min-width: 0; text-align: left;",
    }),
  );
}

// —— 列宽约定（Naive UI DataTable，headless Chrome 实测验证）——
// - `ellipsis` 令 table-layout 强制为 fixed；fixed 布局下**未指定 `width` 的列均分剩余空间**，
//   `minWidth`/`maxWidth` 均无效（maxWidth 仅 `resizable` 时生效）。
// - 策略：除备注外所有列显式 `width`（贴合实际内容，不随窗口漂移）；**备注列不设 `width`，
//   作为唯一弹性列吸收剩余空间**——窗口更宽则备注更宽、更窄则备注收缩，表格始终铺满容器，
//   其余列不被挤压也不被拉伸。备注超长时由单元格内原生 ellipsis 省略 + title 悬停全文
//   （#2010；复制按钮并排，见 renderNoteCellFast）。
// - 不要覆盖 table 的 `width`（改 `auto` 会让带 ellipsis 的列被长文本撑宽）。
// - 使用方以「所有固定列（有 `width` 的列，含金额列；备注不计入）宽度总和」作为 `scroll-x`，
//   作为窄窗口下的横向滚动下限。备注为弹性列，窗口变窄时先由备注收缩吸收，各固定列宽保持恒定——
//   只有当内容区窄于固定列宽总和时才出现横向滚动（固定列总和 965，含来源列 140，窄窗口可能触发，
//   由 scroll-x 提供横向滚动底线）。
// - 宽度按实际内容估算：日期 105 / 类型 65 / 分类 150（最长路径 ≈149px）/ 商户 120 / 账户 180（转账行需容纳「转出 → 转入」两个账户名 + 箭头，长名由链接自身省略号兜底）/ 来源 140（图标 + 实体名 + 状态标注，spec #704）/ 金额 125。固定列总和 965。
