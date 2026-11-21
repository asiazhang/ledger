// 交易列配置共享模块（Issue 39 Prefactor）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 展示文本经 precomputeTransactionDisplayRows 在**数据到达 / 参考上下文变化时**一次性
// 预计算（issue #2188）：列渲染函数只读取预计算 display 记录，不在表格 patch 期间
// 逐格调用国际化取词、金额格式化、分类路径解析或借贷方向派生（计数型渲染守门
// render-gate.test.ts ① 收紧到 0 的接线面）；未接预计算行集的消费方（移动档卡片
// 列表、投资明细页签、单测直调）回落渲染期单源计算路径——两路径共用同一批派生
// 单源函数，零口径漂移。

import { h, type VNode } from "vue";
import { NPerformantEllipsis, type DataTableBaseColumn, type DataTableColumn } from "naive-ui";
import { formatAmount } from "@ledger/money";
import type {
  Transaction,
  TransactionKind,
  TransactionModalRow,
  TransactionPurchase,
  TransactionSource,
} from "@ledger/types";
import type { ComputedRef } from "vue";
import { sharedInputMode, type InputMode } from "@/composables/useInputMode";
import type { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { accentColor } from "@ledger/theme/overrides";
import {
  borderedTagColors,
  neutralTagColors,
  type BorderedTagColors,
} from "@ledger/theme/tag-colors";
import { t } from "@ledger/i18n";
import {
  TX_CELL_LINK_CLASS,
  TX_CELL_PLACEHOLDER_CLASS,
  TX_KIND_TAG_CLASS,
  TX_NOTE_CELL_CLASS,
  TX_SOURCE_CELL_CLASS,
  TX_SOURCE_ICON_CLASS,
  TX_SOURCE_STATUS_TAG_CLASS,
  TX_SOURCE_TYPE_LABEL_CLASS,
} from "./transaction-cell.css.ts";
import { copyIcon, sourceKindIcon } from "@/transaction/static-icons";
import { CELL_ACTION_ATTR } from "@/transaction/table-cell-actions";
import AccountLink from "@/accounts/AccountLink.vue";
import MerchantLink from "@/merchants/MerchantLink.vue";
import SourceLink from "@/transaction/SourceLink.vue";
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
function displayAmountCents(row: TransactionModalRow): number | null {
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
 *   其余列不被挤压也不被拉伸。备注超长时由单元格内懒挂载省略（NPerformantEllipsis）
 *   省略 + 悬停全文（复制按钮并排，见 buildTransactionColumns 备注列渲染）。
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

/** 订单级合并格样式类（购买项展开，ADR-0138 决策 13）：rowspan 单元格内容纵向居中，
 * 样式收口 global.css（不依赖浏览器 UA 对 td 的默认对齐）。 */
const PURCHASE_MERGED_CELL_CLASS = "purchase-merged-cell";

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

/** 行的购买项序位（行渲染回调的单一收口）：未展开行集按无购买项行（-1）处理；
 * 订单块首行 = 0（承载合并格，订单级交互入口所在），续行 > 0（纯购买项行，不可交互）。 */
export function purchaseIndexOf(row: Transaction): number {
  return (row as ExpandedTransactionRow).purchaseIndex ?? -1;
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

/** 行对应的购买项条目；非购买项行或序位越界返回 null（退回原渲染）。 */
function purchaseOf(row: Transaction): TransactionPurchase | null {
  const index = purchaseIndexOf(row);
  return index < 0 ? null : (row.purchases[index] ?? null);
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

/** 类型标签色型（KIND_TAG_TYPE 值闭集的具名导出，预计算记录与列渲染共用）。 */
export type KindTagType = (typeof KIND_TAG_TYPE)[TransactionKind];

/**
 * 单条显示行的预计算展示记录（issue #2188）：数据到达 / 参考上下文变化时
 * 一次性产出，表格 patch 期间只读消费——渲染期不再逐格调用国际化取词、
 * 金额格式化、分类路径解析或借贷方向派生（计数型渲染守门 ① 收紧到 0）。
 *
 * 各字段均为既有派生单源的产物（kindLabel / displayAmountText / categoryPath /
 * kindSemanticColor / merchantMap / SourceLink 与出资分解标注同口径），预计算层
 * 不建立第二套口径；列渲染函数与消费方组件读到的是同一批函数的同一时点输出。
 * 语言 / 主题 / 金额隐私模式 / 参考数据随预计算调用点的响应式上下文即时生效。
 */
export interface TransactionRowDisplay {
  /** 类型标签文案（transfer 经借贷方向派生取四方向文案，其余取 kind 文案） */
  kindLabel: string;
  /** 类型标签色型（KIND_TAG_TYPE 单源） */
  kindTagType: KindTagType;
  /** 分类路径（多级「A > B」，未知 / 无分类 '-' 兜底，与分类列原渲染同口径） */
  categoryPath: string;
  /** 金额文案（displayAmountText 单点：转换转出金额 / split '-' / 隐私掩码 / 数字分组） */
  amountText: string;
  /** 金额语义色（kindSemanticColor 单点，预计算时点主题） */
  amountColor: string;
  /** 账户列展示形态首端：单账户 / 双账户转出在前端 / 分解行首条出资账户 */
  accountPrimaryId: string;
  /** 首端账户展示名（accountMap 命中名；黑洞/隐藏/未知账户 null → 渲染 '-' 占位不可点击，
   * 与 AccountLink 裁决一致） */
  accountPrimaryName: string | null;
  /** 双账户形态第二端（普通转账转入端 / buy/sell 另一端）；单账户形态 null */
  accountSecondaryId: string | null;
  /** 第二端账户展示名（口径同首端）；单账户形态 null */
  accountSecondaryName: string | null;
  /** 出资分解行标注：「等 N 账户」/「同账户 N 笔」；单条出资项 null */
  fundingMarker: string | null;
  /** 退款派生分解行标注「按比例自动分解」；非派生行 null */
  fundingDerivedMarker: string | null;
  /** 商户名称（merchantMap 含软删；未知 id null——渲染 '-' 占位不可点击） */
  merchantName: string | null;
  /** 商户可点击下钻（解析到商户即可，软删商户照常；与 MerchantLink 裁决一致） */
  merchantClickable: boolean;
  /** 来源展示名（display_name 缺省按来源类型名兜底）；无来源 null */
  sourceName: string | null;
  /** 来源类型全称（指针轴悬停 title 与触控轴常驻小字共用，随预计算取词）；无来源 null */
  sourceKindLabel: string | null;
  /** 来源状态标注文案（已取消 / 已删除等）；无状态 null */
  sourceStatusLabel: string | null;
  /** 来源可点击（软删保单不可点击，与 SourceLink 裁决一致） */
  sourceClickable: boolean;
  /** 购买项行件数标注（「共 N 件」）；仅购买项行 */
  purchaseQuantityLabel?: string;
}

/** 展开且带预计算记录的显示行（交易主列表与搜索视图的表格行形态）。 */
export interface PrecomputedTransactionRow extends ExpandedTransactionRow {
  display: TransactionRowDisplay;
}

/** 渲染期预计算记录读取口（唯一）：未接预计算行集的行（移动卡片 / 投资页签 /
 * 单测直调）返回 null，列渲染回落渲染期单源计算——回落即「删除预计算接线」的
 * 守门形态（计数型渲染守门 ① 变红的可失败路径）。 */
function precomputedDisplayOf(row: Transaction): TransactionRowDisplay | null {
  return (row as Partial<PrecomputedTransactionRow>).display ?? null;
}

/** 来源三态展示数据（与 SourceLink 口径逐字段一致：展示名兜底 / 类型全称 / 状态标注 / 可点击裁决）。 */
function sourceDisplayOf(
  source: TransactionSource | null,
): Pick<
  TransactionRowDisplay,
  "sourceName" | "sourceKindLabel" | "sourceStatusLabel" | "sourceClickable"
> {
  if (!source)
    return {
      sourceName: null,
      sourceKindLabel: null,
      sourceStatusLabel: null,
      sourceClickable: false,
    };
  return {
    sourceName: source.display_name || t(`transactions.source.kind.${source.kind}`),
    sourceKindLabel: t(`transactions.source.kind.${source.kind}`),
    sourceStatusLabel: source.status ? t(`transactions.source.status.${source.status}`) : null,
    sourceClickable: source.status !== "deleted",
  };
}

/** 出资分解行标注派生（单一出处）：多账户「等 N 账户」、同账户多条「同账户 N 笔」、
 * 单条无标注；退款派生分解行另附「按比例自动分解」。预计算层与渲染回落路径
 * 共用本函数，两路径零口径漂移。 */
function fundingMarkersOf(row: Transaction): { marker: string | null; derived: string | null } {
  const distinctAccounts = new Set(row.fundings.map((f) => f.account_id)).size;
  return {
    marker:
      distinctAccounts > 1
        ? t("transactions.funding.accountCount", { n: distinctAccounts })
        : row.fundings.length > 1
          ? t("transactions.funding.sameAccountEntries", { n: row.fundings.length })
          : null,
    derived: row.fundings.some((f) => f.derived) ? t("transactions.funding.autoDerived") : null,
  };
}

/** 账户列展示形态与出资分解标注（与 renderAccountCell 分派逻辑逐字段一致的单源
 * 数据面，renderFundingAccountCell 预计算路径与移动档卡片共用）。 */
function accountDisplayOf(
  row: Transaction,
): Pick<
  TransactionRowDisplay,
  "accountPrimaryId" | "accountSecondaryId" | "fundingMarker" | "fundingDerivedMarker"
> {
  if (row.fundings.length > 0) {
    // 出资分解行（分解子行在场 ⇔ account_id 读回 null，ADR-0138 决策 6）：
    // 首条出资账户 + 标注（fundingMarkersOf 单源）。
    const markers = fundingMarkersOf(row);
    return {
      accountPrimaryId: row.fundings[0].account_id,
      accountSecondaryId: null,
      fundingMarker: markers.marker,
      fundingDerivedMarker: markers.derived,
    };
  }
  const accountId = row.account_id ?? "";
  if (row.kind === "transfer" && row.to_account_id)
    return {
      accountPrimaryId: accountId,
      accountSecondaryId: row.to_account_id,
      fundingMarker: null,
      fundingDerivedMarker: null,
    };
  if (row.kind === "buy" && row.funding_account_id)
    return {
      accountPrimaryId: row.funding_account_id,
      accountSecondaryId: accountId,
      fundingMarker: null,
      fundingDerivedMarker: null,
    };
  if (row.kind === "sell" && row.funding_account_id)
    return {
      accountPrimaryId: accountId,
      accountSecondaryId: row.funding_account_id,
      fundingMarker: null,
      fundingDerivedMarker: null,
    };
  return {
    accountPrimaryId: accountId,
    accountSecondaryId: null,
    fundingMarker: null,
    fundingDerivedMarker: null,
  };
}

/** 预计算装配面：调用方按需声明，缺省即未展开（每交易一行）。 */
export interface PrecomputeTransactionDisplayOptions {
  /** 购买项展开（与 expandPurchaseRows 同语义）：每购买项一行、行序 = 交易序 */
  expandPurchases?: boolean;
}

/**
 * 交易行集 → 展开显示行集 + 展示文本预计算（issue #2188 展示行预计算入口）：
 * 以当前页交易行集与只读参考上下文为输入，一次性产出每条显示行的展示文本与
 * 属性（类型标签文案与色型、借贷派生四方向标签、分类路径、商户名称与可点击
 * 状态、账户展示形态与出资分解标注、来源展示名与状态标注、格式化金额与类型
 * 语义色、购买项件数标注）。
 *
 * 纯函数：无副作用、不触达表格渲染；语言 / 外观主题 / 金额隐私模式 / 参考数据
 * 经既有单源的响应式上下文读取（t / useAppStore().theme / formatAmount 内部
 * 开关 / reference 派生映射），调用方以 computed 包裹即随上下文变化整体重派生
 * ——表格展示文本与颜色即时更新，无需重新请求后端。
 */
export function precomputeTransactionDisplayRows(
  reference: ReferenceStore,
  rows: Transaction[],
  options: PrecomputeTransactionDisplayOptions = {},
): PrecomputedTransactionRow[] {
  const theme = useAppStore().theme;
  const expandPurchases = options.expandPurchases ?? false;
  return rows.flatMap((row) => {
    const merchant = row.merchant_id ? reference.merchantMap.get(row.merchant_id) : undefined;
    const accountShape = accountDisplayOf(row);
    const base: PrecomputedTransactionRow = {
      ...row,
      purchaseIndex: -1,
      display: {
        kindLabel: kindLabel(reference, row),
        kindTagType: KIND_TAG_TYPE[row.kind],
        categoryPath: row.category_id ? reference.categoryPath(row.category_id) || "-" : "-",
        amountText: displayAmountText(reference, row),
        amountColor: kindSemanticColor(row.kind, theme),
        ...accountShape,
        // 账户展示名随预计算解析（accountMap 含软删历史口径；黑洞/隐藏/未知 null
        // → 轻量渲染 '-' 占位不可点击，与 AccountLink 裁决一致）。
        accountPrimaryName: reference.accountMap.get(accountShape.accountPrimaryId)?.name ?? null,
        accountSecondaryName: accountShape.accountSecondaryId
          ? (reference.accountMap.get(accountShape.accountSecondaryId)?.name ?? null)
          : null,
        merchantName: merchant?.name ?? null,
        merchantClickable: !!merchant,
        ...sourceDisplayOf(row.source),
      },
    };
    if (!expandPurchases || row.purchases.length === 0) return [base];
    return row.purchases.map((item, purchaseIndex): PrecomputedTransactionRow => ({
      ...base,
      purchaseIndex,
      display: {
        ...base.display,
        categoryPath: item.category_id ? reference.categoryPath(item.category_id) || "-" : "-",
        purchaseQuantityLabel: t("transactions.purchase.quantity", { n: item.quantity }),
      },
    }));
  });
}

/** 购买项单元格件数标注样式（弱化灰后缀注记，与账户列「等 N 账户」同型）。 */
const PURCHASE_QTY_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

/** 购买项行渲染单点（issue #1884 / ADR-0138 决策 15：列表 / 移动卡片 / 只读详情共用，
 * 不自建第二套）——商品名 +「共 N 件」件数标注，两形态可选：
 * - truncate（桌面列表，「商品 / 备注」列的商品形态，#1883）：名内懒挂载省略
 *   （NPerformantEllipsis：初始纯 span，指针悬停才挂带提示弹层的完整省略组件，
 *   #2188）+ 悬停全文，长商品名不再只剩「…」；
 * - 全文（缺省，移动卡片与详情）：整名渲染、自然换行——触控轴没有悬停，详情是长
 *   商品名的全文出口（决策 15）。
 * 件数标注文案：`quantityText` 传入即直接使用（预计算件数标注，表格渲染期零取词，
 * #2188）；缺省回落 t() 单源（移动卡片 / 详情渲染期取词，口径同一键）。只读呈现：
 * 无复制按钮（备注复制通道不适用商品名）、无价格（存而不显示，决策 10）；
 * `suffix` 额外弱化标注（详情的分类路径）插在件数标注之前。 */
export function renderPurchaseLine(
  item: TransactionPurchase,
  opts: { truncate?: boolean; suffix?: string; quantityText?: string } = {},
): VNode {
  const name = opts.truncate
    ? h(
        NPerformantEllipsis,
        { style: "flex: 1 1 auto; min-width: 0;" },
        { default: () => item.name },
      )
    : h("span", { style: "flex: 1 1 auto; min-width: 0; word-break: break-word;" }, item.name);
  const quantityText =
    opts.quantityText ?? t("transactions.purchase.quantity", { n: item.quantity });
  return h("div", { style: NOTE_CELL_STYLE }, [
    name,
    ...(opts.suffix ? [h("span", { style: PURCHASE_QTY_STYLE }, opts.suffix)] : []),
    h("span", { style: PURCHASE_QTY_STYLE }, quantityText),
  ]);
}

/** 「商品 / 备注」列的商品形态（issue #1883 / ADR-0138 决策 13）：截断形态 + 悬停全文；
 * 备注列商品形态只承载商品名与件数（订单号出口在详情订单区，#1928）。
 * 件数标注读预计算（#2188：表格 patch 期零取词），display 缺省回落 t 单源。 */
function renderPurchaseCell(
  item: TransactionPurchase,
  display: TransactionRowDisplay | null,
): VNode {
  return renderPurchaseLine(item, { truncate: true, quantityText: display?.purchaseQuantityLabel });
}

/** 轻量「⋯」按钮公共形态（issue #2189 操作列瘦身）：原生 button 替代 NButton
 * tiny quaternary——移除每行波纹层/加载态/主题混入的组件实例开销，保留钩子类
 * （row-actions-btn 触控热区、global.css 显隐/基座规则）与无障碍标签；按钮基座
 * 与焦点环样式收口 transaction-cell.css.ts（仅表格内作用）。事件绑定两形态：
 * 交易列表 = 表级委托（data-tx-action，见 buildTransactionColumns rowActions），
 * 投资明细页签 = 直连回调（rowActionsColumn，消费方零迁移）。 */
function lightActionsButton(
  label: string,
  extra: Record<string, unknown>,
  content: string | VNode[] = "⋯",
): VNode {
  return h(
    "button",
    {
      type: "button",
      class: "row-actions-btn touch-hit-area",
      "aria-label": label,
      style: "width: 22px; height: 22px; line-height: 1; font-size: 14px;",
      ...extra,
    },
    content,
  );
}

/** buildTransactionColumns 可选装配面：调用方按需声明，缺省即纯只读列（搜索结果同款）。 */
export interface BuildTransactionColumnsOptions {
  /** 触控轴信号（视图 setup 期经 useInputMode 消费登记后传入，issue #2187 共享单例）：
   * 金额单元格（触控轴保留点按全文气泡封装、指针轴轻量文本）与来源类型全称
   * 常驻小字按轴分流；缺省保守触控轴（一击可达替代不缺席）。 */
  inputMode?: ComputedRef<InputMode>;
  /** 交易行「⋯」常显操作列（ADR-0088 决策 6，issue #843；issue #2189 起委托形态）：
   * 传入即追加常显操作列——按钮只携带 data-tx-action="row-menu" + data-tx-row-key，
   * 打开动作经表级事件委托分发到 RowContextMenu 同一 open 入口（视图侧
   * useTransactionTableCellActions 装配）；不传则不渲染该列（搜索结果只读）。 */
  rowActions?: boolean;
  /** 购买项展开（issue #1883 / ADR-0138 决策 13）：声明即按「订单块逐购买项行」
   * 装配——订单级列（日期/类型/商户/来源/账户/金额/操作）rowSpan 纵向合并、
   * 分类列逐行取购买项分类、备注列一列两用。仅消费 expandPurchaseRows 展开行集的
   * 视图可声明（交易列表与搜索结果，后者自 #1885 起商品名命中即整单呈现）；未
   * 展开行集不声明，呈现零变化。 */
  expandPurchases?: boolean;
}

/**
 * 交易基础列：日期/类型/分类/账户/来源/备注/金额（搜索结果与交易列表共用）。
 * 行内高频单元格在指针轴下全部为轻量原生元素（issue #2189）：类型标签 = 语义
 * 样式元素（色型经 @ledger/theme tag-colors 按主题解析，与 NTag 公式对齐）、
 * 账户/商户/来源 = 原生按钮/占位文本 + data-tx-action 语义属性（动作经表级
 * 事件委托分发，见 table-cell-actions）、备注复制 = 原生按钮 + 共享静态图标、
 * 金额 = 轻量文本（触控轴保留 AmountCell 交互封装）。列级按当前主题解析一次
 * 的强调色/标签色型与全列恒常文案在列构建期取定；预计算 display 记录缺省时
 * （未接预计算行集/单测直调）回落组件渲染——回落即「删除瘦身接线」的守门
 * 形态（计数型渲染守门 ② 变红的可失败路径）。
 */
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
  // —— 列级一次解析（当前主题与语言）：视图以 computed 重建列，主题/语言切换
  // 即整体换装换文案；表格 patch 期间只读这些闭包常量，零逐格解析。 ——
  const theme = useAppStore().theme;
  const accent = accentColor(theme);
  const tagColors = borderedTagColors(theme);
  const statusTagColors = neutralTagColors(theme);
  const viewAccountLabel = t("common.link.viewAccount");
  const viewMerchantLabel = t("common.link.viewMerchant");
  const noteCopyLabel = t("transactions.list.copyNote");
  const actionsAriaLabel = t("transactions.menu.actions");
  // 输入轴分流：调用方显式传入信号用之，缺省读共享信号（无消费登记时保守
  // 触控轴——一击可达替代不缺席，见 useInputMode.sharedInputMode）。
  const inputMode = options.inputMode ?? sharedInputMode;
  const isTouch = () => inputMode.value === "touch";

  /** 强调色内联样式（列级解析一次；--tx-accent-hover 供 hover/focus 态 CSS 消费）。 */
  const accentStyle = `color: ${accent.base}; --tx-accent-hover: ${accent.hover};`;

  /** 轻量账户链接（可下钻）：原生按钮 + data-tx-action（委托分发）。
   * layoutStyle 缺省 = 限宽省略（max-width: 100%，单账户形态）；容器内形态传
   * ACCOUNT_CELL_LINK_STYLE（flex 内容宽度项，收缩省略由容器约束）。 */
  const accountLink = (accountId: string, name: string, layoutStyle?: string): VNode =>
    h(
      "button",
      {
        type: "button",
        class: TX_CELL_LINK_CLASS,
        title: viewAccountLabel,
        style: `${accentStyle}${layoutStyle ?? "max-width: 100%;"}`,
        [CELL_ACTION_ATTR]: "account-drill",
        "data-tx-account": accountId,
      },
      name,
    );

  /** 轻量占位文本（黑洞/隐藏账户、未知商户、软删保单名称：不可点击）。 */
  const placeholder = (text: string, style?: string): VNode =>
    h("span", { class: TX_CELL_PLACEHOLDER_CLASS, style }, text);

  const columns: DataTableColumn<Transaction>[] = [
    { title: t("transactions.columns.date"), key: "date", width: 105, ...merged },
    {
      title: t("transactions.columns.kind"),
      key: "kind",
      width: 65,
      ...merged,
      // 类型标签轻量化（issue #2189）：语义样式元素承载既有四类色型（色型经
      // tag-colors 与 NTag 公式对齐，圆角随中性 token）；文案与色型读预计算
      // （#2188：patch 期零取词、零借贷派生），display 缺省回落渲染期单源。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        const tagType = display ? display.kindTagType : KIND_TAG_TYPE[row.kind];
        const label = display ? display.kindLabel : kindLabel(reference, row);
        const colors: BorderedTagColors = tagColors[tagType];
        return h(
          "span",
          {
            class: TX_KIND_TAG_CLASS,
            style: {
              color: colors.text,
              background: colors.background,
              border: colors.border,
              borderRadius: colors.radius,
            },
          },
          label,
        );
      },
    },
    {
      title: t("transactions.columns.category"),
      key: "category_id",
      width: 150,
      // 懒挂载省略（#2188）：performant-ellipsis 初始只输出纯文本包裹节点，
      // 指针悬停才挂带提示弹层的完整省略组件——初始渲染与翻页不再为每格
      // 同步实例化弹层子树；固定列宽与悬停全文语义不变。
      ellipsis: { tooltip: true },
      ellipsisComponent: "performant-ellipsis",
      // 分类列逐行显示（ADR-0138 决策 13）：展开视图下购买项行显示该购买项自己的分类、
      // 不做一致性合并（层级分类下没有任何一个真值能同时代表一单内的不同分类）；
      // 无购买项行与未展开视图仍显示交易行分类，呈现零变化。
      // 路径文本读预计算（#2188），display 缺省回落 categoryPath 单源。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        if (display) return display.categoryPath;
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
      // 懒挂载省略（#2188，分类列同款理由）
      ellipsis: { tooltip: true },
      ellipsisComponent: "performant-ellipsis",
      // 商户名经 merchantMap（含软删）解析并可点击下钻（issue #191）；未知/无商户
      // 渲染 '-' 占位不可点击。轻量形态（#2189）：原生按钮 + data-tx-action 委托
      // 下钻，与 MerchantLink 组件（表外消费方）同视觉同裁决。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        if (!display)
          return row.merchant_id ? h(MerchantLink, { merchantId: row.merchant_id }) : "-";
        if (!display.merchantName || !row.merchant_id) return "-";
        return h(
          "button",
          {
            type: "button",
            class: TX_CELL_LINK_CLASS,
            title: viewMerchantLabel,
            style: `${accentStyle}max-width: 100%;`,
            [CELL_ACTION_ATTR]: "merchant-drill",
            "data-tx-merchant": row.merchant_id,
          },
          display.merchantName,
        );
      },
    },
    {
      title: t("transactions.columns.account"),
      key: "account_id",
      width: 180,
      ...merged,
      // 轻量账户列（#2189）：账户名原生按钮（委托下钻）或 '-' 占位文本；
      // 三形态（单账户/双账户/分解标注）与 renderAccountCell 分派同构，
      // 名称/可点击性读预计算（accountMap 解析，黑洞账户占位）。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        if (!display) return renderAccountCell(row);
        // 容器形态（双账户 / 分解标注行）：链接作 flex 内容宽度项（收缩省略）；
        // 单账户形态无容器包裹，限宽由列宽约束（max-width: 100%，与 AccountLink 同）。
        const inContainer = row.fundings.length > 0 || display.accountSecondaryId !== null;
        const layoutStyle = inContainer ? ACCOUNT_CELL_LINK_STYLE : undefined;
        const primary = display.accountPrimaryName
          ? accountLink(display.accountPrimaryId, display.accountPrimaryName, layoutStyle)
          : placeholder("-", ACCOUNT_CELL_LINK_STYLE);
        // 出资分解行：首条出资账户 + 「等 N 账户 / 同账户 N 笔」标注（+ 派生徽标）
        if (row.fundings.length > 0) {
          const line = h(
            "div",
            {
              style:
                "display: inline-flex; align-items: center; justify-content: flex-start; gap: 4px; width: 100%; max-width: 100%;",
            },
            [
              primary,
              ...(display.fundingMarker
                ? [h("span", { style: ACCOUNT_CELL_SUFFIX_STYLE }, display.fundingMarker)]
                : []),
            ],
          );
          if (!display.fundingDerivedMarker) return line;
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
                  style: FUNDING_DERIVED_MARKER_STYLE,
                },
                display.fundingDerivedMarker,
              ),
            ],
          );
        }
        // 双账户形态（转账「转出 → 转入」、buy/sell「出资账户 → 投资账户」）
        if (display.accountSecondaryId !== null) {
          const secondary = display.accountSecondaryName
            ? accountLink(display.accountSecondaryId, display.accountSecondaryName, layoutStyle)
            : placeholder("-", ACCOUNT_CELL_LINK_STYLE);
          return h(
            "div",
            {
              style:
                "display: inline-flex; align-items: center; justify-content: flex-start; gap: 4px; width: 100%; max-width: 100%;",
            },
            [primary, h("span", { style: "flex: none; opacity: 0.5;" }, "→"), secondary],
          );
        }
        return primary;
      },
    },
    {
      title: t("transactions.columns.source"),
      key: "source",
      width: 140,
      ...merged,
      // 来源列（spec #704 / issue #706）：图标 + 实体名 + 状态标注。轻量形态
      // （#2189）：静态矢量图标（零组件实例）+ 原生按钮（data-tx-action 委托，
      // 点击仍经来源跳转深模块解析路由、注入收纳/功能开关谓词——分发收口
      // table-cell-actions）；软删保单占位文本不可点击；触控轴类型全称常驻小字
      // （title 不可达的一击可达替代，issue #843）。与 SourceLink 组件
      //（表外消费方）同视觉同裁决。不设列级 ellipsis（账户列同款理由）。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        if (!display) return row.source ? h(SourceLink, { source: row.source }) : "-";
        const source = row.source;
        if (!source) return "-";
        const kindLabel = display.sourceKindLabel ?? "";
        const name = display.sourceClickable
          ? h(
              "button",
              {
                type: "button",
                class: TX_CELL_LINK_CLASS,
                style: `${accentStyle}max-width: 100%;`,
                [CELL_ACTION_ATTR]: "source-jump",
                "data-tx-source-kind": source.kind,
                "data-tx-source-entity": source.entity_id,
              },
              display.sourceName ?? kindLabel,
            )
          : placeholder(display.sourceName ?? "-");
        return h("span", { class: TX_SOURCE_CELL_CLASS, title: kindLabel }, [
          h("span", { class: TX_SOURCE_ICON_CLASS }, [sourceKindIcon(source.kind)]),
          name,
          ...(isTouch() && source.display_name
            ? [h("span", { class: TX_SOURCE_TYPE_LABEL_CLASS }, kindLabel)]
            : []),
          ...(display.sourceStatusLabel
            ? [
                h(
                  "span",
                  {
                    class: TX_SOURCE_STATUS_TAG_CLASS,
                    style: {
                      color: statusTagColors.text,
                      background: statusTagColors.background,
                      borderRadius: statusTagColors.radius,
                    },
                  },
                  display.sourceStatusLabel,
                ),
              ]
            : []),
        ]);
      },
    },
    {
      // 列名两形态（ADR-0138 决策 13）：展开视图整列「商品 / 备注」（一列两用），
      // 未展开视图仍「备注」——列头与其内容形态一致。
      title: options.expandPurchases
        ? t("transactions.columns.itemNote")
        : t("transactions.columns.note"),
      key: "note",
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + 悬停显示全文）；
      // 不设列级 ellipsis（账户/来源列同款理由：会把复制按钮一起包进省略容器），
      // 省略与悬停全文由单元格内懒挂载省略承担（fixed 布局由分类/商户列维持）
      render: (row) => {
        const item = options.expandPurchases ? purchaseOf(row) : null;
        // 购买项行：商品名 + 件数标注（只读呈现，无复制按钮——备注复制通道
        // 不适用商品名，issue #1884 / ADR-0138 决策 15）
        if (item) return renderPurchaseCell(item, precomputedDisplayOf(row));
        // 备注单元格（显式复制通道，见「界面文本不可选」）：懒挂载省略承载文本
        // （自省略 + 悬停全文，#2188），轻量复制按钮（#2189：原生 button + 共享
        // 静态图标 + data-tx-action 委托剪贴板写入与统一 toast）悬停行显现
        //（显隐样式收口 global.css）；无备注落 '-'，不渲染复制按钮。
        const { note } = row;
        if (!note) return "-";
        return h("div", { class: TX_NOTE_CELL_CLASS }, [
          h(
            NPerformantEllipsis,
            { style: "flex: 1 1 auto; min-width: 0;" },
            { default: () => note },
          ),
          lightActionsButton(
            noteCopyLabel,
            {
              class: "note-copy-btn touch-hit-area",
              title: noteCopyLabel,
              [CELL_ACTION_ATTR]: "copy-note",
              "data-tx-note": note,
              style: "flex: none; width: 22px; height: 22px;",
            },
            [copyIcon()],
          ),
        ]);
      },
    },
    {
      title: t("transactions.columns.amount"),
      key: "amount_native_cents",
      width: 125,
      ...merged,
      // 金额按交易类型语义色着色（issue #435）：色值单一来源在
      // @ledger/theme/semantic-colors（六类型亮/暗两套）。文案与语义色读预计算
      // （#2188：patch 期零金额格式化），display 缺省回落渲染期单源。
      // 输入轴分流（issue #2189）：指针轴直接渲染轻量文本元素（零组件实例），
      // 触控轴保留 AmountCell 交互封装（点按弹出全文气泡，继续纳入
      // Overlay Suppression 弹层注册表）。
      render: (row) => {
        const display = precomputedDisplayOf(row);
        const text = display ? display.amountText : displayAmountText(reference, row);
        const color = display
          ? display.amountColor
          : kindSemanticColor(row.kind, useAppStore().theme);
        if (isTouch()) return h(AmountCell, { text, color });
        return h("span", { class: "amount-cell", style: { color } }, text);
      },
    },
  ];
  // 行菜单操作列（「⋯」常显第二入口，委托形态）：按钮只携带语义属性，
  // 打开动作经表级事件委托分发（视图装配 useTransactionTableCellActions）。
  if (options.rowActions) {
    columns.push({
      title: t("transactions.columns.actions"),
      key: "actions",
      width: 64,
      ...merged,
      render: (row) =>
        lightActionsButton(actionsAriaLabel, {
          [CELL_ACTION_ATTR]: "row-menu",
          // 显示行键随按钮携带（displayRowKey 唯一）：视图按键解析目标行打开
          // RowContextMenu（与行右键同一 open 入口，购买项续行无按钮不可触发）。
          "data-tx-row-key": displayRowKey(row),
        }),
    });
  }
  return columns;
}

/**
 * 交易行「⋯」常显列——直连回调形态（投资明细页签消费方，issue #1781）：与右键
 * 共用同一行菜单编排 open 入口、以点击坐标弹出，全平台常显。交易列表的操作列
 * 已改走委托形态（buildTransactionColumns rowActions 选项，#2189）；本形态保留
 * 泛型直连回调签名，投资页签消费零迁移（按钮形态轻量化同规）。
 */
export function rowActionsColumn<T>(
  onRowMenuOpen: (event: MouseEvent, row: T) => void,
  // 合并装配（issue #1883 / ADR-0138 决策 13）：购买项展开视图传入 rowSpan + 合并格
  // 样式类，操作列在订单块内只渲染一个；其余消费方（投资明细页签）不传，零变化。
  merge: Partial<Pick<DataTableBaseColumn<T>, "rowSpan" | "className">> = {},
): DataTableColumn<T> {
  // 无障碍标签在列构建时取词一次（#2188）：它是全列恒常文案，不随行变化——
  // 逐行渲染期取词是计数型渲染守门 ① 的最大单点（每行一次），收拢到数据/
  // 上下文到达阶段（列数组重建时点）。
  const actionsAriaLabel = t("transactions.menu.actions");
  return {
    title: t("transactions.columns.actions"),
    key: "actions",
    width: 64,
    ...merge,
    render: (row) =>
      lightActionsButton(actionsAriaLabel, {
        onClick: (e: MouseEvent) => onRowMenuOpen(e, row),
      }),
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

/** 退款派生分解徽标样式（轻量列渲染与 renderFundingAccountCell 组件版共用单点）。 */
const FUNDING_DERIVED_MARKER_STYLE =
  "font-size: 11px; line-height: 18px; padding: 0 6px; border-radius: 9px; white-space: nowrap; color: var(--n-info-color, #2080f0); border: 1px dashed currentColor;";

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
  // 标注文案读预计算（#2188：patch 期零取词）；display 缺省回落渲染期单源计算
  //（fundingMarkersOf 与预计算层共用同一派生函数，两路径零口径漂移）。
  const display = precomputedDisplayOf(row);
  const markers = display
    ? { marker: display.fundingMarker, derived: display.fundingDerivedMarker }
    : fundingMarkersOf(row);
  const markerText = markers.marker;
  const derivedMarkerText = markers.derived;
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
  if (!derivedMarkerText) return line;
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
          style: FUNDING_DERIVED_MARKER_STYLE,
        },
        derivedMarkerText,
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
