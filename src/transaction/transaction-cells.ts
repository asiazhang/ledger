// 交易单元格展示模型（issue #2010 翻页渲染性能：文本预计算单点）：
// 数据到达时（行集进入表格前）一次性把每行每格格式化为最终展示串/派生标注——
// 类型标签文案、金额（含隐私模式与数字分组）、分类路径（购买项行取商品分类）、
// 借贷派生、双账户/出资分解标注、来源名与状态、备注/商品行——渲染期单元格只消费
// 纯数据，不再逐格执行 t() / formatAmount / categoryPath / resolveLendingDirection
// （渲染期格式化是 Linux(WebKitGTK) 翻页迟滞的根因之一，#2010 诊断）。
//
// 口径单源纪律：本模块的每格派生**只消费既有单源函数**（kindLabel /
// displayAmountText / resolveLendingDirection / categoryPath / merchantMap），
// 移动档卡片列表与详情弹窗继续直接消费这些函数；表格列渲染与本模块同源，不产生
// 第二套渲染口径（词汇表「单元格预计算」）。函数纯（输入行集 + ReferenceStore +
// 主题），在调用方 computed 内求值时依赖（参考数据 / 界面语言 / 隐私模式 / 主题）
// 被追踪——任一变化整表重算，语言切换与账户改名照常即时刷新（等价约束，#2010）。
//
// 类型标签语义色板的「平替 NTag」派生：色值公式镜像 naive tag 主题 self()
// （bordered 中号：文字 = 语义色、边框 = 语义色 30% 透明、底色亮 10% / 暗透明），
// 输入是「naive 基础主题 ⊕ 应用 overrides」的合成 common——与 NConfigProvider 运行期
// 合成同构；NTag 平替与真实 NTag 的逐值一致由 transaction-columns 测试守门
// （naive 升级改色板即红， conscious 更新）。

import type { Component } from "vue";
import type { CSSProperties } from "vue";
import { darkTheme, lightTheme } from "naive-ui";
import {
  CalendarClearOutline,
  CubeOutline,
  RepeatOutline,
  ShieldCheckmarkOutline,
  SwapHorizontalOutline,
  TrendingUpOutline,
} from "@vicons/ionicons5";
import { darkOverrides, lightOverrides, accentColor } from "@ledger/theme/overrides";
import type { Theme } from "@ledger/theme";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import type {
  Transaction,
  TransactionKind,
  TransactionModalRow,
  TransactionPurchase,
  TransactionSource,
  TransactionSourceKind,
} from "@ledger/types";
import type { useReferenceStore } from "@/stores/reference";
import { lendingLabelKey, resolveLendingDirection } from "@/transaction/lending";

export type ReferenceStore = ReturnType<typeof useReferenceStore>;

// —— 类型标签语义色（原住 transaction-columns，随预计算单点迁入；columns 转出口不变） ——

/** 类型标签语义色型（naive NTag type 闭集子集） */
export type KindTagType = "success" | "warning" | "info" | "default";

export const KIND_TAG_TYPE: Record<TransactionKind, KindTagType> = {
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

/** 类型标签文案（原住 transaction-columns，随预计算单点迁入；columns 转出口不变）：
 * 借贷是 transfer 的派生视角——两端账户类型构成借贷（receivable/debt）的转账显示
 * 借出/收回/借入/还款专属文案，普通转账仍显示「转账」；非 transfer kind 按自身
 * kind 标签。方向识别收口 domain 层借贷模块（与表单分派/回填共用同一函数）。
 * 导出面：移动档卡片列表与桌面表格共用（单源同文案，issue #846）。 */
export function kindLabel(reference: ReferenceStore, row: Transaction): string {
  if (row.kind !== "transfer") return t(`transactions.kind.${row.kind}`);
  const direction = resolveLendingDirection(row, (id) => reference.accountMap.get(id)?.type);
  return t(lendingLabelKey(direction ?? "none"));
}

// —— 金额展示口径（原住 transaction-columns，随预计算单点迁入；columns 转出口不变） ——

/** 列表/卡片金额展示口径单点：基金转换行展示**转出金额**（确认单权威，ADR-0099），
 * 份额调整行无现金腿、无金额（ADR-0106）返回 `null`（空值口径，渲染 '-'），
 * 其余行展示行金额锚点（本位币口径）。
 *
 * 转换行的行金额锚点是服务端按 FIFO 消耗算出的**结转成本**（不是用户看到的转出金额），
 * 故展示必须走扩展字段；扩展缺失（旧数据/直读快照）时回退行金额，不抛错、不显空。
 * 表格金额列与移动卡片共用，两处各自分支即口径漂移。 */
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

/** 购买项件数标注文案单点（预计算与 renderPurchaseLine 全文形态共用，不出现第二拼法）。 */
export function purchaseQtyText(item: TransactionPurchase): string {
  return t("transactions.purchase.quantity", { n: item.quantity });
}

// —— NTag 平替色板（主题派生常量；公式镜像 naive tag 主题，见文件头注） ——

/** naive 基础主题 ⊕ 应用 overrides 的合成 common（与 NConfigProvider 运行期合成同构）。 */
const MERGED_COMMON: Record<Theme, Record<string, string>> = {
  dark: { ...darkTheme.common, ...darkOverrides.common },
  light: { ...lightTheme.common, ...lightOverrides.common },
};

/** #RRGGBB → `rgba(r, g, b, a)`（seemly changeColor 的输出格式镜像）；
 * 非六位十六进制输入原样返回（透明度派生只作用于 hex 语义色）。 */
function withAlpha(color: string, alpha: number): string {
  if (/^#[0-9a-fA-F]{6}$/.test(color)) {
    const r = parseInt(color.slice(1, 3), 16);
    const g = parseInt(color.slice(3, 5), 16);
    const b = parseInt(color.slice(5, 7), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  return color;
}

/** 亮色 bordered 底色/边框透明度（naive tag light self 逐值镜像：success/info 同档，
 * warning 单独提档——底色 .12 / 边框 .35）；暗色 bordered 底色恒透明、边框 .3。 */
const TYPED_TAG_ALPHAS: Record<Exclude<KindTagType, "default">, { bg: number; border: number }> = {
  success: { bg: 0.1, border: 0.3 },
  info: { bg: 0.1, border: 0.3 },
  warning: { bg: 0.12, border: 0.35 },
};

const KIND_TAG_STYLE_CACHE = new Map<string, CSSProperties>();

/** 类型标签平替样式（bordered 中号 NTag 的色值镜像：文字 = 语义色、边框 = 语义色
 * 30% 透明、底色亮色 10% 着色 / 暗色透明——naive tag self 公式逐值一致，守门测试
 * 见 transaction-columns.test.ts）。布局度量（28px 高 / 14px 字号 / 0 7px 内边距）
 * 收口 global.css `.tx-kind-tag`，本函数只出随主题变的色值与圆角。 */
export function kindTagStyle(type: KindTagType, theme: Theme): CSSProperties {
  const key = `${type}:${theme}`;
  const cached = KIND_TAG_STYLE_CACHE.get(key);
  if (cached) return cached;
  const c = MERGED_COMMON[theme];
  let style: CSSProperties;
  if (type === "default") {
    style = {
      color: c.textColor2,
      borderColor: c.borderColor,
      background: theme === "light" ? "rgb(250, 250, 252)" : "#0000",
    };
  } else {
    const base =
      type === "success" ? c.successColor : type === "warning" ? c.warningColor : c.infoColor;
    const alpha = TYPED_TAG_ALPHAS[type];
    style = {
      color: base,
      borderColor: withAlpha(base, theme === "light" ? alpha.border : 0.3),
      background: theme === "light" ? withAlpha(base, alpha.bg) : "#0000",
    };
  }
  const full = { ...style, borderRadius: c.borderRadiusSmall };
  KIND_TAG_STYLE_CACHE.set(key, full);
  return full;
}

/** 来源状态标注平替样式（small 无边框 default NTag 镜像：文字 textColor2、底 tagColor、
 * 圆角 borderRadiusSmall；度量收口 global.css `.tx-status-tag`）。 */
const STATUS_TAG_STYLE_CACHE = new Map<string, CSSProperties>();

export function statusTagStyle(theme: Theme): CSSProperties {
  const cached = STATUS_TAG_STYLE_CACHE.get(theme);
  if (cached) return cached;
  const c = MERGED_COMMON[theme];
  const style: CSSProperties = {
    color: c.textColor2,
    background: c.tagColor,
    borderRadius: c.borderRadiusSmall,
  };
  STATUS_TAG_STYLE_CACHE.set(theme, style);
  return style;
}

/** 来源类型 → 图标（六类闭集穷尽映射，编译器强制补行；与侧栏视图图标同源惯例：
 * 保单=盾、物品=立方、标的=走势、订阅=循环、转账=双向、分期=日历）。
 * 单源住址随 #2010 从 SourceLink 迁入（表格平替渲染与组件同源消费）。 */
export const SOURCE_KIND_ICONS: Record<TransactionSourceKind, Component> = {
  installmentPlan: CalendarClearOutline,
  subscription: RepeatOutline,
  scheduledTransfer: SwapHorizontalOutline,
  policy: ShieldCheckmarkOutline,
  item: CubeOutline,
  instrument: TrendingUpOutline,
};

/** 桌面表格壳层 CSS 变量（主题派生单点）：视图挂在包裹 NDataTable 的 div 上
 * （#2010 行级事件委托的监听容器），供 `.tx-link` 等平替类消费——强调色随主题
 * 即时换色（accentColor 选择器单源），hover 底色与小控件圆角取合成 common。 */
export function transactionTableShellVars(theme: Theme): Record<string, string> {
  const accent = accentColor(theme);
  const c = MERGED_COMMON[theme];
  return {
    "--tx-accent": accent.base,
    "--tx-accent-hover": accent.hover,
    "--tx-hover-tint": c.buttonColor2Hover,
    "--tx-ctrl-radius": c.borderRadius,
  };
}

// —— 单元格展示模型 ——

/** 账户格原子：可下钻的可见账户，或黑洞/隐藏账户占位（渲染纯文本「-」，不下钻）。 */
export type AccountAtom =
  | { link: true; accountId: string; name: string }
  | { link: false; placeholder: "-" };

/** 账户格三形态（issue #99 / #937 / #1030 方向修正；分解行 #1861 / ADR-0138 决策 8）：
 * 单账户 / 双账户（转账「转出 → 转入」、buy/sell「资金流出方在前」，ADR-0096 决策 6）/
 * 出资分解（首条出资账户 + 「等 N 账户 / 同账户 N 笔」标注，派生分解行另带
 * 「按比例自动分解」标注）。语义与移动档卡片（renderAccountCell）同一模型单源。 */
export type AccountCellView =
  | { form: "single"; account: AccountAtom }
  | { form: "two"; from: AccountAtom; to: AccountAtom }
  | {
      form: "funding";
      first: AccountAtom;
      marker: string | null;
      derivedMarkerText: string | null;
    };

/** 来源格模型（spec #704 / #706；语义与 SourceLink 组件同一单源）：
 * 展示名空串按来源类型名兜底；软删保单（status=deleted）不可点击；类型全称供
 * 悬停 title 与触控轴常驻小字（触控轴仅在有自定义名时渲染，不与兜底名重复）。 */
export interface SourceCellView {
  kind: TransactionSourceKind;
  entityId: string;
  name: string;
  /** 类型全称（悬停 title / 触控轴小字文案单点） */
  typeTitle: string;
  /** 是否有自定义展示名（触控轴类型小字的渲染判定） */
  hasDisplayName: boolean;
  statusLabel: string | null;
  /** 状态标注平替样式（statusTagStyle 缓存共享对象，随主题） */
  statusStyle: CSSProperties;
  clickable: boolean;
}

/** 备注格三形态：空（'-'）/ 备注（悬停全文 title + 复制通道）/ 购买项行
 * （商品名 + 「共 N 件」标注，订单号出口在详情订单区 #1928）。 */
export type NoteCellView =
  | { form: "empty" }
  | { form: "note"; text: string }
  | { form: "purchase"; name: string; qtyText: string };

/** 行的全部预计算单元格（与现渲染口径逐字段一致，验收单测见
 * src/__tests__/transaction-cells.test.ts）。 */
export interface TransactionCells {
  /** 类型标签文案（借贷派生经 kindLabel 单源） */
  kindText: string;
  kindTagType: KindTagType;
  /** 类型标签平替样式（kindTagStyle 缓存共享对象，随主题/语义色型） */
  kindTagStyle: CSSProperties;
  /** 分类路径（展开视图购买项行取商品自己的分类，ADR-0138 决策 13；空值 '-'） */
  categoryText: string;
  /** 商户格：可解析商户（含软删）或 null（无/未知 → '-'） */
  merchant: { merchantId: string; name: string } | null;
  account: AccountCellView;
  source: SourceCellView | null;
  note: NoteCellView;
  /** 金额文案（displayAmountText 单源，含隐私模式与数字分组） */
  amountText: string;
  /** 金额语义色（kindSemanticColor 单源，随主题） */
  amountColor: string;
}

/** 带预计算单元格的行（withTransactionCells 的返回行型；表格 :data 的行集类型）。 */
export type TransactionCellRow<R extends Transaction = Transaction> = R & {
  cells: TransactionCells;
};

/** 账户格原子解析：可见账户（accountMap 命中）可下钻；黑洞/隐藏账户不在
 * accountMap（list_accounts 不含隐藏账户），渲染占位「-」——AccountLink 同口径。 */
function accountAtom(reference: ReferenceStore, accountId: string): AccountAtom {
  const account = reference.accountMap.get(accountId);
  return account
    ? { link: true, accountId, name: account.name }
    : { link: false, placeholder: "-" };
}

/** 账户格三形态判定（移动档 renderAccountCell 与桌面表格同一单源）。 */
export function accountCellView(row: Transaction, reference: ReferenceStore): AccountCellView {
  // 分解行（分解子行在场 ⇔ account_id 读回 null，ADR-0138 决策 6）走分解形态
  if (row.fundings.length > 0) {
    const distinctAccounts = new Set(row.fundings.map((f) => f.account_id)).size;
    const marker =
      distinctAccounts > 1
        ? t("transactions.funding.accountCount", { n: distinctAccounts })
        : row.fundings.length > 1
          ? t("transactions.funding.sameAccountEntries", { n: row.fundings.length })
          : null;
    return {
      form: "funding",
      first: accountAtom(reference, row.fundings[0].account_id),
      marker,
      derivedMarkerText: row.fundings.some((f) => f.derived)
        ? t("transactions.funding.autoDerived")
        : null,
    };
  }
  const accountId = row.account_id ?? "";
  if (row.kind === "transfer" && row.to_account_id) {
    return {
      form: "two",
      from: accountAtom(reference, accountId),
      to: accountAtom(reference, row.to_account_id),
    };
  }
  if (row.kind === "buy" && row.funding_account_id) {
    return {
      form: "two",
      from: accountAtom(reference, row.funding_account_id),
      to: accountAtom(reference, accountId),
    };
  }
  if (row.kind === "sell" && row.funding_account_id) {
    return {
      form: "two",
      from: accountAtom(reference, accountId),
      to: accountAtom(reference, row.funding_account_id),
    };
  }
  return { form: "single", account: accountAtom(reference, accountId) };
}

/** 来源格模型（SourceLink 组件语义的单源化：组件与桌面表格同消费）。 */
export function sourceCellView(source: TransactionSource, theme: Theme): SourceCellView {
  return {
    kind: source.kind,
    entityId: source.entity_id,
    name: source.display_name || t(`transactions.source.kind.${source.kind}`),
    typeTitle: t(`transactions.source.kind.${source.kind}`),
    hasDisplayName: !!source.display_name,
    statusLabel: source.status ? t(`transactions.source.status.${source.status}`) : null,
    statusStyle: statusTagStyle(theme),
    // 软删保单不在列表、无详情面——不提供落空的跳转（词汇表「不可点击范围仅软删保单」）
    clickable: source.status !== "deleted",
  };
}

/** 行的购买项序位（单一收口，#2010 自 transaction-columns 迁入正源）：未展开行集
 * 按无购买项行（-1）处理；订单块首行 = 0（承载合并格，订单级交互入口所在），
 * 续行 > 0（纯购买项行，不可交互）。 */
export function purchaseIndexOf(row: Transaction): number {
  return (row as { purchaseIndex?: number }).purchaseIndex ?? -1;
}

/** 行对应的购买项条目；非购买项行或序位越界返回 null。 */
function purchaseOf(row: Transaction): TransactionPurchase | null {
  const index = purchaseIndexOf(row);
  return index < 0 ? null : (row.purchases[index] ?? null);
}

/** 单行全部单元格预计算（各格派生见字段注；全部经既有单源函数）。 */
function buildCells(row: Transaction, reference: ReferenceStore, theme: Theme): TransactionCells {
  const purchase = purchaseOf(row);
  const merchant = row.merchant_id ? reference.merchantMap.get(row.merchant_id) : undefined;
  const note: NoteCellView = purchase
    ? { form: "purchase", name: purchase.name, qtyText: purchaseQtyText(purchase) }
    : row.note
      ? { form: "note", text: row.note }
      : { form: "empty" };
  return {
    kindText: kindLabel(reference, row),
    kindTagType: KIND_TAG_TYPE[row.kind],
    kindTagStyle: kindTagStyle(KIND_TAG_TYPE[row.kind], theme),
    // 分类逐行（展开视图购买项行取商品分类，无一致性合并，ADR-0138 决策 13）
    categoryText: (() => {
      const categoryId = purchase ? purchase.category_id : row.category_id;
      return categoryId ? reference.categoryPath(categoryId) || "-" : "-";
    })(),
    merchant: merchant ? { merchantId: merchant.id, name: merchant.name } : null,
    account: accountCellView(row, reference),
    source: row.source ? sourceCellView(row.source, theme) : null,
    note,
    amountText: displayAmountText(reference, row),
    amountColor: kindSemanticColor(row.kind, theme),
  };
}

/** 行集 → 预计算单元格行集（数据到达时一次性装配，#2010 预计算接线单点）：
 * 浅拷贝行并挂 `cells`（与 expandPurchaseRows 的浅拷贝同纪律——行只读消费）。
 * 在 computed 内调用时依赖（参考数据 / 语言 / 隐私模式 / 主题）被追踪，任一变化
 * 整表重算；渲染期零格式化（计数守门 #2012 ①项收紧到 0 的形态基础）。 */
export function withTransactionCells<R extends Transaction>(
  rows: R[],
  reference: ReferenceStore,
  theme: Theme,
): TransactionCellRow<R>[] {
  return rows.map((row) => ({ ...row, cells: buildCells(row, reference, theme) }));
}
