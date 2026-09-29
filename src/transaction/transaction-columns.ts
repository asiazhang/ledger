// 交易列配置共享模块（Issue 39 Prefactor）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 渲染函数在运行时读取 store 的响应式数据，构建一次即可，无需 computed 包裹。

import { h, type VNode } from "vue";
import { NEllipsis, NButton, NTag, type DataTableBaseColumn, type DataTableColumn } from "naive-ui";
import { formatAmount } from "@ledger/money";
import type {
  Transaction,
  TransactionKind,
  TransactionModalRow,
  TransactionPurchase,
} from "@ledger/types";
import type { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { t } from "@ledger/i18n";
import AccountLink from "@/accounts/AccountLink.vue";
import MerchantLink from "@/merchants/MerchantLink.vue";
import SourceLink from "@/transaction/SourceLink.vue";
import NoteCopyButton from "@ledger/ui-kit/NoteCopyButton.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import { lendingLabelKey, resolveLendingDirection } from "@/transaction/lending";

export type ReferenceStore = ReturnType<typeof useReferenceStore>;

export const KIND_TAG_TYPE: Record<TransactionKind, "success" | "warning" | "info" | "default"> = {
  income: "success",
  expense: "warning",
  refund: "info",
  transfer: "default",
  buy: "default",
  sell: "default",
  // 基金转换（ADR-0099）取 info 蓝色标注：与退款同色型但标签文案不同，
  // 一眼区分于买入/卖出的中性标签（投资类默认色）。
  convert: "info",
  // 份额调整（ADR-0106 / #1049）：与转换同为「无现金腿」kind，同取 info 蓝色标注，
  // 标签文案区分二者。
  split: "info",
  // 现金分红（ADR-0109 / #1078）：投资现金流入（income 语义），取 success 绿色标注，
  // 与买入/卖出的中性标签区分。
  dividend: "success",
};

/**
 * 列表/卡片金额展示口径单点：基金转换行展示**转出金额**（确认单权威，ADR-0099），
 * 份额调整行无现金腿、无金额（ADR-0106）返回 `null`（空值口径，渲染 '-'），
 * 其余行展示行金额锚点（本位币口径）。
 *
 * 转换行的行金额锚点是服务端按 FIFO 消耗算出的**结转成本**（不是用户看到的转出金额），
 * 故展示必须走扩展字段；扩展缺失（旧数据/直读快照）时回退行金额，不抛错、不显空。
 * 表格金额列与移动卡片共用 `displayAmountText`，两处各自分支即口径漂移。
 */
export function displayAmountCents(row: TransactionModalRow): number | null {
  if (row.kind === "convert" && row.convert) return row.convert.out_amount_cents;
  // 份额调整（ADR-0106 决策 1）：无现金腿、无金额——按空值语义返回 null，
  // 不以 0 伪装「已知为零」（同持仓缺价行的 '-' 口径）。
  if (row.kind === "split") return null;
  return row.amount_native_cents;
}

/** 金额展示文案单点（表格金额列与移动卡片共用）：空值口径（无现金腿的 split 无金额）
 * 渲染 '-'，其余经 `formatAmount`（含金额隐私模式与数字分组）。 */
export function displayAmountText(reference: ReferenceStore, row: TransactionModalRow): string {
  const cents = displayAmountCents(row);
  return cents === null ? "-" : formatAmount(cents, reference.getCurrency(row.currency_code));
}

/** 交易基础列：日期/类型/分类/账户/备注/金额（搜索结果与交易列表共用，只读）。
 * 列名经 t() 取当前语言：使用方以 computed 构造列数组（TransactionsView/SearchView），
 * 语言切换时重建列，表头即时更新。
 *
 * 列宽约定（Naive UI DataTable，headless Chrome 实测验证）：
 * - `ellipsis` 令 table-layout 强制为 fixed；fixed 布局下**未指定 `width` 的列均分剩余空间**，
 *   `minWidth`/`maxWidth` 均无效（maxWidth 仅 `resizable` 时生效）。
 * - 策略：除备注外所有列显式 `width`（贴合实际内容，不随窗口漂移）；**备注列不设 `width`，
 *   作为唯一弹性列吸收剩余空间**——窗口更宽则备注更宽、更窄则备注收缩，表格始终铺满容器，
 *   其余列不被挤压也不被拉伸。备注超长时由单元格内 NEllipsis 省略 + 悬停全文
 *   （复制按钮并排，见 renderNoteCell）。
 * - 不要覆盖 table 的 `width`（改 `auto` 会让带 `ellipsis` 的列被长文本撑宽，实测分类
 *   150→286px、备注 240→398px）。
 * - 使用方以「所有固定列（有 `width` 的列，含金额列；备注不计入）宽度总和」作为 `scroll-x`，
 *   作为窄窗口下的横向滚动下限。备注为弹性列，窗口变窄时先由备注收缩吸收，各固定列宽保持恒定——
 *   只有当内容区窄于固定列宽总和时才出现横向滚动（固定列总和 965，含来源列 140，窄窗口可能触发，
 *   由 scroll-x 提供横向滚动底线）。
 * - 宽度按实际内容估算：日期 105 / 类型 65 / 分类 150（最长路径 ≈149px）/ 商户 120 / 账户 180（转账行需容纳「转出 → 转入」两个账户名 + 箭头，长名由链接自身省略号兜底）/ 来源 140（图标 + 实体名 + 状态标注，spec #704）/ 金额 125。固定列总和 965。 */
/** 类型标签（issue #374）：借贷是 transfer 的派生视角——两端账户类型构成借贷
 * （receivable/debt）的转账显示借出/收回/借入/还款专属文案，普通转账仍显示「转账」；
 * 非 transfer kind 不参与派生、按自身 kind 标签。历史数据实时派生、无数据迁移。
 * 方向识别收口 domain 层借贷模块（与表单分派/回填共用同一函数），
 * 标签随账户映射响应式更新（同 categoryPath 的响应式纪律）。
 * 导出面（issue #846）：移动档卡片列表消费同一派生，与表格类型列单源同文案。 */
export function kindLabel(reference: ReferenceStore, row: Transaction): string {
  if (row.kind !== "transfer") return t(`transactions.kind.${row.kind}`);
  const direction = resolveLendingDirection(row, (id) => reference.accountMap.get(id)?.type);
  return t(lendingLabelKey(direction ?? "none"));
}

/** 备注单元格布局：文本占满剩余宽度（自省略），复制按钮固定宽度靠右。 */
const NOTE_CELL_STYLE =
  "display: flex; align-items: center; gap: 2px; width: 100%; max-width: 100%;";

/** 订单徽章单元格样式（行尾静态徽章，issue #1862 / ADR-0138 决策 9）：弱化描边、
 * 不随主题强调色（静态标注非操作件），固定宽度不收缩。 */
export const ORDER_BADGE_STYLE =
  "flex: none; font-size: 11px; line-height: 18px; padding: 0 6px; border-radius: 9px; white-space: nowrap; color: var(--n-text-color-disabled, #999); border: 1px dashed currentColor;";

/** 来源订单号静态徽章（行尾「订单 <单号>」，桌面表格与移动卡片共用单源）：
 * 静态、不可点击、无过滤、不分组（ADR-0138 决策 9 否决位置性交互）——来源订单号
 * 列有值才渲染，无值零变化（不占独立行位，徽章随行渲染）。 */
export function renderOrderBadge(sourceOrderNo: string): VNode {
  return h(
    "span",
    { style: ORDER_BADGE_STYLE },
    t("transactions.order.badge", { no: sourceOrderNo }),
  );
}

/** 订单级合并格样式类（购买项展开，ADR-0138 决策 13）：rowspan 单元格内容纵向居中，
 * 样式收口 global.css（不依赖浏览器 UA 对 td 的默认对齐）。 */
export const PURCHASE_MERGED_CELL_CLASS = "purchase-merged-cell";

/**
 * 展开后的显示行（issue #1883 / ADR-0138 决策 13）：购买项行由主交易行派生——
 * 同一交易带 `purchaseIndex` 复制 N 份（N = 购买项数），无购买项交易复制 1 份
 * （purchaseIndex = -1，呈现零变化）。派生行不是独立排序/分页单元：行序 = 交易序，
 * 同单内按购买项顺序位相邻（分页、total、筛选与排序仍按交易计，ADR-0138 决策 14）。
 */
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

/** 行的购买项序位（行渲染回调的单一收口）：未展开行集（搜索结果等）按无购买项行（-1）处理；
 * 订单块首行 = 0（承载合并格，订单级交互入口所在），续行 > 0（纯购买项行，不可交互）。 */
export function purchaseIndexOf(row: Transaction): number {
  return (row as ExpandedTransactionRow).purchaseIndex ?? -1;
}

/** 行对应的购买项条目；非购买项行或序位越界返回 null（退回原渲染）。 */
function purchaseOf(row: Transaction): TransactionPurchase | null {
  const index = purchaseIndexOf(row);
  return index < 0 ? null : (row.purchases[index] ?? null);
}

/** 订单级格跨度（naive-ui 列 rowSpan 回调）：订单块首行 = 购买项数（向下合并），
 * 其余行 = 1（被合并覆盖的格子由表格按坐标跳过，不渲染）。 */
export function orderLevelRowSpan(row: Transaction): number {
  return purchaseIndexOf(row) === 0 ? Math.max(row.purchases.length, 1) : 1;
}

/** 购买项单元格件数标注样式（弱化灰后缀注记，与账户列「等 N 账户」同型）。 */
const PURCHASE_QTY_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

/** 购买项行渲染单点（issue #1884 / ADR-0138 决策 15：列表 / 移动卡片 / 只读详情共用，
 * 不自建第二套）——商品名 +「共 N 件」件数标注，两形态可选：
 * - truncate（桌面列表，「商品 / 备注」列的商品形态，#1883）：名内 NEllipsis 自省略 +
 *   悬停全文，长商品名不再只剩「…」；
 * - 全文（缺省，移动卡片与详情）：整名渲染、自然换行——触控轴没有悬停，详情是长
 *   商品名的全文出口（决策 15）。
 * 只读呈现：无复制按钮（备注复制通道不适用商品名）、无价格（存而不显示，决策 10）；
 * `suffix` 额外弱化标注（详情的分类路径）插在件数标注之前。 */
export function renderPurchaseLine(
  item: TransactionPurchase,
  opts: { truncate?: boolean; suffix?: string } = {},
): VNode {
  const name = opts.truncate
    ? h(NEllipsis, { style: "flex: 1 1 auto; min-width: 0;" }, { default: () => item.name })
    : h("span", { style: "flex: 1 1 auto; min-width: 0; word-break: break-word;" }, item.name);
  return h("div", { style: NOTE_CELL_STYLE }, [
    name,
    ...(opts.suffix ? [h("span", { style: PURCHASE_QTY_STYLE }, opts.suffix)] : []),
    h(
      "span",
      { style: PURCHASE_QTY_STYLE },
      t("transactions.purchase.quantity", { n: item.quantity }),
    ),
  ]);
}

/** 「商品 / 备注」列的商品形态（issue #1883 / ADR-0138 决策 13）：截断形态 + 悬停全文；
 * 订单徽章不随购买项行渲染（备注列商品形态只承载商品名与件数，订单号出口在详情）。 */
function renderPurchaseCell(item: TransactionPurchase): VNode {
  return renderPurchaseLine(item, { truncate: true });
}

/** 备注单元格渲染（显式复制通道，见 CONTEXT-ui-interaction「界面文本不可选」）：
 * - 无备注且无订单徽章渲染 '-'，不渲染复制按钮（空备注无可复制）；
 * - 有备注：单元格内 flex——NEllipsis 承载文本（自省略 + 悬停全文，同账户/来源列的
 *   单元格内省略模式），NoteCopyButton 复制完整备注（clipboard API + toast），
 *   按钮悬停行显现（显隐样式收口 global.css）；
 * - 来源订单号列有值时行尾追加静态订单徽章（无备注行也渲染徽章，不落 '-'）。
 * - 展开视图（issue #1883）下仅无购买项行走本渲染，购买项行走 renderPurchaseCell。 */
function renderNoteCell(row: Transaction): VNode | string {
  const { note } = row;
  const badge = row.source_order_no ? renderOrderBadge(row.source_order_no) : null;
  if (!note) return badge ?? "-";
  const children: VNode[] = [
    h(NEllipsis, { style: "flex: 1 1 auto; min-width: 0;" }, { default: () => note }),
    h(NoteCopyButton, { note, style: "flex: none;" }),
  ];
  if (badge) children.push(badge);
  return h("div", { style: NOTE_CELL_STYLE }, children);
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
   * 视图可声明（交易列表）；未展开行集（搜索结果）不声明，呈现零变化。 */
  expandPurchases?: boolean;
}

export function buildTransactionColumns(
  reference: ReferenceStore,
  options: BuildTransactionColumnsOptions = {},
): DataTableColumn<Transaction>[] {
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
      render: (row) => h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => kindLabel(reference, row)),
    },
    {
      title: t("transactions.columns.category"),
      key: "category_id",
      width: 150,
      ellipsis: { tooltip: true },
      // 分类列逐行显示（ADR-0138 决策 13）：展开视图下购买项行显示该购买项自己的分类、
      // 不做一致性合并（层级分类下没有任何一个真值能同时代表一单内的不同分类）；
      // 无购买项行与未展开视图仍显示交易行分类，呈现零变化。
      render: (row) => {
        const item = options.expandPurchases ? purchaseOf(row) : null;
        const categoryId = item ? item.category_id : row.category_id;
        return categoryId ? reference.categoryPath(categoryId) || "-" : "-";
      },
    },
    {
      title: t("transactions.columns.merchant"),
      key: "merchant_id",
      width: 120,
      ...merged,
      ellipsis: { tooltip: true },
      // 商户名经 merchantMap（含软删）解析并可点击下钻（issue #191）；未知/无商户回退 '-'
      render: (row) => (row.merchant_id ? h(MerchantLink, { merchantId: row.merchant_id }) : "-"),
    },
    {
      title: t("transactions.columns.account"),
      key: "account_id",
      width: 180,
      ...merged,
      render: (row) => renderAccountCell(row),
    },
    {
      title: t("transactions.columns.source"),
      key: "source",
      width: 140,
      ...merged,
      // 来源列（spec #704 / issue #706）：图标 + 实体名 + 状态标注，点击经来源
      // 跳转深模块落地（SourceLink 内部收口）；无来源留空（手动/AI 导入口径）。
      // 不设列级 ellipsis（账户列同款理由：NEllipsis 会把图标/名称/标注包装成
      // 整体省略，破坏链接自身省略与标注并排语义），超长由链接自身省略号兜底。
      render: (row) => (row.source ? h(SourceLink, { source: row.source }) : "-"),
    },
    {
      // 列名两形态（ADR-0138 决策 13）：展开视图整列「商品 / 备注」（一列两用），
      // 未展开视图（搜索结果）仍「备注」——列头与其内容形态一致。
      title: options.expandPurchases
        ? t("transactions.columns.itemNote")
        : t("transactions.columns.note"),
      key: "note",
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + 悬停显示全文）；
      // 不设列级 ellipsis（账户/来源列同款理由：会把复制按钮一起包进省略容器），
      // 省略与悬停全文由单元格内 NEllipsis 承担（fixed 布局由分类/商户列维持）
      render: (row) => {
        const item = options.expandPurchases ? purchaseOf(row) : null;
        return item ? renderPurchaseCell(item) : renderNoteCell(row);
      },
    },
    {
      title: t("transactions.columns.amount"),
      key: "amount_native_cents",
      width: 125,
      ...merged,
      // 金额按交易类型语义色着色（issue #435）：色值单一来源在
      // @ledger/theme/semantic-colors（六类型亮/暗两套）。主题在渲染时读取 app store
      // 响应式取值：切换外观主题即时换色，无需重建列；借出/借入/收回/还款是
      // transfer 的派生视角（ADR-0053），随 transfer 同紫，不做派生级区分。
      // 单元格交互归 AmountCell（issue #843）：指针轴纯 span 零变化，触控轴
      // 点按弹出全文（悬停一击可达；文案与色在此单点计算后传入）。
      render: (row) =>
        h(AmountCell, {
          text: displayAmountText(reference, row),
          color: kindSemanticColor(row.kind, useAppStore().theme),
        }),
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
 */
export function rowActionsColumn<T>(
  onRowMenuOpen: (event: MouseEvent, row: T) => void,
  // 合并装配（issue #1883 / ADR-0138 决策 13）：购买项展开视图传入 rowSpan + 合并格
  // 样式类，操作列在订单块内只渲染一个；其余消费方（投资明细页签）不传，零变化。
  merge: Partial<Pick<DataTableBaseColumn<T>, "rowSpan" | "className">> = {},
): DataTableColumn<T> {
  return {
    title: t("transactions.columns.actions"),
    key: "actions",
    width: 64,
    ...merge,
    render: (row) =>
      h(
        NButton,
        {
          size: "tiny",
          quaternary: true,
          class: "row-actions-btn touch-hit-area",
          "aria-label": t("transactions.menu.actions"),
          onClick: (e: MouseEvent) => onRowMenuOpen(e, row),
        },
        () => "⋯",
      ),
  };
}

/** 转账/出资账户行单元格内账户链接的布局样式：内容宽度 + 允许收缩省略 + 文本左对齐。
 * 经 attrs 透传到 AccountLink 根按钮，与组件内部强调色样式合并。
 * 用内容宽度（flex-grow:0）而非均分剩余宽度：单账户行「花呗」是内容宽度、自然靠左，
 * 双账户行首账户名若也均分半宽会因 <button> 默认 text-align:center 被水平居中、顶不到列左缘
 * （与上方单账户行错位）。内容宽度让首名紧贴列左缘、与单账户行对齐；
 * 收缩项仍由 min-width:0 允许收缩（长名省略号兜底、不溢出）。 */
const ACCOUNT_CELL_LINK_STYLE = "flex: 0 1 auto; min-width: 0; text-align: left;";

/** 双账户单元格（转账「转出 → 转入」、出资 buy/sell「出资账户 → 投资账户」）公共渲染：
 * inline-flex 容器，两个链接内容宽度、箭头固定宽度，整组 justify-content:flex-start
 * 靠左；长账户名由链接自身 ellipsis（见 AccountLink）省略号兜底、不溢出（列宽 180 时收缩省略）。
 * 首账户名因此与单账户行（如「花呗」）左侧对齐；不设列级 ellipsis（fixed 布局由备注列的
 * ellipsis 维持），否则 NEllipsis 会把两个按钮包装成整体省略，破坏各自可点击语义。
 * 导出面（issue #846）：移动档卡片列表消费同一渲染，账户呈现两形态单源。 */
function renderTwoAccountCell(fromAccountId: string, toAccountId: string): VNode {
  return h(
    "div",
    {
      style:
        "display: inline-flex; align-items: center; justify-content: flex-start; gap: 4px; width: 100%; max-width: 100%;",
    },
    [
      h(AccountLink, { accountId: fromAccountId, style: ACCOUNT_CELL_LINK_STYLE }),
      h("span", { style: "flex: none; opacity: 0.5;" }, "→"),
      h(AccountLink, { accountId: toAccountId, style: ACCOUNT_CELL_LINK_STYLE }),
    ],
  );
}

/** 账户单元格标注文字（弱化灰、随标注元素固定宽度不收缩）：语义为「共 N 个」后缀注记。 */
const ACCOUNT_CELL_SUFFIX_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

/** 出资分解行账户单元格（issue #1861 / ADR-0138 决策 8）：分解行主表 account_id 为 null，
 * 账户列显示首条出资账户链接（可下钻，与单账户行同语义）+ 规则标注——
 * - 多账户：「等 N 账户」（N = 去重账户数，组合支付形态）；
 * - 同账户多条：退化「同账户 N 笔」（N = 出资项条数，预售定金 + 尾款形态）；
 * - 单条出资项：仅首账户（无标注）；
 * - 退款派生分解行（读时推导、不落库，ADR-0138 决策 5）另起一行「按比例自动分解」标注。
 * 导出面（issue #846）：移动档卡片列表消费同一渲染，账户呈现三形态单源。 */
function renderFundingAccountCell(row: Transaction): VNode {
  const first = row.fundings[0];
  const distinctAccounts = new Set(row.fundings.map((f) => f.account_id)).size;
  // 标注文案：多账户取「等 N 账户」、同账户多条退化「同账户 N 笔」、单条无标注
  const markerText =
    distinctAccounts > 1
      ? t("transactions.funding.accountCount", { n: distinctAccounts })
      : row.fundings.length > 1
        ? t("transactions.funding.sameAccountEntries", { n: row.fundings.length })
        : null;
  const line = h(
    "div",
    {
      style:
        "display: inline-flex; align-items: center; justify-content: flex-start; gap: 4px; width: 100%; max-width: 100%;",
    },
    [
      h(AccountLink, { accountId: first.account_id, style: ACCOUNT_CELL_LINK_STYLE }),
      ...(markerText ? [h("span", { style: ACCOUNT_CELL_SUFFIX_STYLE }, markerText)] : []),
    ],
  );
  // 派生标注（任意条目 derived=true 即派生分解）另起一行，不与账户名挤同一行
  if (!row.fundings.some((f) => f.derived)) return line;
  return h(
    "div",
    {
      style:
        "display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%;",
    },
    [
      line,
      h(
        "span",
        {
          style:
            "font-size: 11px; line-height: 18px; padding: 0 6px; border-radius: 9px; white-space: nowrap; color: var(--n-info-color, #2080f0); border: 1px dashed currentColor;",
        },
        t("transactions.funding.autoDerived"),
      ),
    ],
  );
}

/** 账户单元格渲染（issue #99 / #937，方向修正 issue #1030）：
 * - 出资分解行（ADR-0138 决策 8，issue #1861）：首条出资账户 + 「等 N 账户 / 同账户 N 笔」
 *   标注（退款派生分解行另带「按比例自动分解」标注），见 renderFundingAccountCell；
 * - 转账行显示「转出 → 转入」双向账户名（to_account_id 存在时），两个名字各自可点击、
 *   各自下钻到对应账户的过滤视图；
 * - 带出资账户的 buy/sell 行按「资金流出方在前」显示双向账户名（ADR-0096 决策 6：
 *   buy「出资账户 → 投资账户」、sell「投资账户 → 出资账户」，与出资账户的流入/流出
 *   契约语义及转账行阅读顺序一致），两端各自可点击下钻；
 * - 其余交易类型（含不带出资账户的 buy/sell）仍显示主账户名（可点击下钻，issue #97）。
 *
 * 出资账户为空投资账户照常；出资账户命中时资金实际流出方在前（buy：出资账户，
 * sell：投资账户），与转账「资金流出方在前」的阅读顺序一致（issue #1030）。 */
export function renderAccountCell(row: Transaction): VNode {
  // 分解行（分解子行在场 ⇔ account_id 读回 null，ADR-0138 决策 6）走分解形态；
  // 账户列三形态（单账户 / 双账户 / 分解标注）与移动档卡片列表共用本渲染单源。
  if (row.fundings.length > 0) {
    return renderFundingAccountCell(row);
  }
  const accountId = row.account_id ?? "";
  if (row.kind === "transfer" && row.to_account_id) {
    return renderTwoAccountCell(accountId, row.to_account_id);
  }
  if (row.kind === "buy" && row.funding_account_id) {
    return renderTwoAccountCell(row.funding_account_id, accountId);
  }
  if (row.kind === "sell" && row.funding_account_id) {
    return renderTwoAccountCell(accountId, row.funding_account_id);
  }
  return h(AccountLink, { accountId });
}
