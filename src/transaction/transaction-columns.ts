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
  convert: "info",
  split: "info",
  dividend: "success",
};

function displayAmountCents(row: TransactionModalRow): number | null {
  if (row.kind === "convert" && row.convert) return row.convert.out_amount_cents;
  if (row.kind === "split") return null;
  return row.amount_native_cents;
}

export function displayAmountText(reference: ReferenceStore, row: TransactionModalRow): string {
  const cents = displayAmountCents(row);
  return cents === null ? "-" : formatAmount(cents, reference.getCurrency(row.currency_code));
}

export function kindLabel(reference: ReferenceStore, row: Transaction): string {
  if (row.kind !== "transfer") return t(`transactions.kind.${row.kind}`);
  const direction = resolveLendingDirection(row, (id) => reference.accountMap.get(id)?.type);
  return t(lendingLabelKey(direction ?? "none"));
}

const NOTE_CELL_STYLE =
  "display: flex; align-items: center; gap: 2px; width: 100%; max-width: 100%;";

const PURCHASE_MERGED_CELL_CLASS = "purchase-merged-cell";

export interface ExpandedTransactionRow extends Transaction {
  purchaseIndex: number;
}

export function expandPurchaseRows(rows: Transaction[]): ExpandedTransactionRow[] {
  return rows.flatMap((row) =>
    row.purchases.length === 0
      ? [{ ...row, purchaseIndex: -1 }]
      : row.purchases.map((_, purchaseIndex) => ({ ...row, purchaseIndex })),
  );
}

export function purchaseIndexOf(row: Transaction): number {
  return (row as ExpandedTransactionRow).purchaseIndex ?? -1;
}

export function displayRowKey(row: Transaction): string {
  return `${row.id}:${purchaseIndexOf(row)}`;
}

function purchaseOf(row: Transaction): TransactionPurchase | null {
  const index = purchaseIndexOf(row);
  return index < 0 ? null : (row.purchases[index] ?? null);
}

function orderLevelRowSpan(row: Transaction): number {
  return purchaseIndexOf(row) === 0 ? Math.max(row.purchases.length, 1) : 1;
}

const PURCHASE_BLOCK_ROW_CLASS = "purchase-block-row";
const PURCHASE_BLOCK_LAST_CLASS = "purchase-block-last";

export function purchaseRowClass(row: Transaction): string | undefined {
  const index = purchaseIndexOf(row);
  if (index < 0) return undefined;
  return index === row.purchases.length - 1
    ? `${PURCHASE_BLOCK_ROW_CLASS} ${PURCHASE_BLOCK_LAST_CLASS}`
    : PURCHASE_BLOCK_ROW_CLASS;
}

export type KindTagType = (typeof KIND_TAG_TYPE)[TransactionKind];

export interface TransactionRowDisplay {
  kindLabel: string;
  kindTagType: KindTagType;
  categoryPath: string;
  amountText: string;
  amountColor: string;
  accountPrimaryId: string;
  accountPrimaryName: string | null;
  accountSecondaryId: string | null;
  accountSecondaryName: string | null;
  fundingMarker: string | null;
  fundingDerivedMarker: string | null;
  merchantName: string | null;
  merchantClickable: boolean;
  sourceName: string | null;
  sourceKindLabel: string | null;
  sourceStatusLabel: string | null;
  sourceClickable: boolean;
  purchaseQuantityLabel?: string;
}

export interface PrecomputedTransactionRow extends ExpandedTransactionRow {
  display: TransactionRowDisplay;
}

function precomputedDisplayOf(row: Transaction): TransactionRowDisplay | null {
  return (row as Partial<PrecomputedTransactionRow>).display ?? null;
}

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

function accountDisplayOf(
  row: Transaction,
): Pick<
  TransactionRowDisplay,
  "accountPrimaryId" | "accountSecondaryId" | "fundingMarker" | "fundingDerivedMarker"
> {
  if (row.fundings.length > 0) {
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

export interface PrecomputeTransactionDisplayOptions {
  expandPurchases?: boolean;
}

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

const PURCHASE_QTY_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

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

function renderPurchaseCell(
  item: TransactionPurchase,
  display: TransactionRowDisplay | null,
): VNode {
  return renderPurchaseLine(item, { truncate: true, quantityText: display?.purchaseQuantityLabel });
}

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

export interface BuildTransactionColumnsOptions {
  inputMode?: ComputedRef<InputMode>;
  rowActions?: boolean;
  expandPurchases?: boolean;
}

export function buildTransactionColumns(
  reference: ReferenceStore,
  options: BuildTransactionColumnsOptions = {},
): DataTableColumn<Transaction>[] {
  const merged: Partial<Pick<DataTableBaseColumn<Transaction>, "rowSpan" | "className">> =
    options.expandPurchases
      ? { rowSpan: orderLevelRowSpan, className: PURCHASE_MERGED_CELL_CLASS }
      : {};
  const theme = useAppStore().theme;
  const accent = accentColor(theme);
  const tagColors = borderedTagColors(theme);
  const statusTagColors = neutralTagColors(theme);
  const viewAccountLabel = t("common.link.viewAccount");
  const viewMerchantLabel = t("common.link.viewMerchant");
  const noteCopyLabel = t("transactions.list.copyNote");
  const actionsAriaLabel = t("transactions.menu.actions");
  const inputMode = options.inputMode ?? sharedInputMode;
  const isTouch = () => inputMode.value === "touch";

  const accentStyle = `color: ${accent.base}; --tx-accent-hover: ${accent.hover};`;

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

  const placeholder = (text: string, style?: string): VNode =>
    h("span", { class: TX_CELL_PLACEHOLDER_CLASS, style }, text);

  const columns: DataTableColumn<Transaction>[] = [
    { title: t("transactions.columns.date"), key: "date", width: 105, ...merged },
    {
      title: t("transactions.columns.kind"),
      key: "kind",
      width: 65,
      ...merged,
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
      ellipsis: { tooltip: true },
      ellipsisComponent: "performant-ellipsis",
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
      ellipsis: { tooltip: true },
      ellipsisComponent: "performant-ellipsis",
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
      render: (row) => {
        const display = precomputedDisplayOf(row);
        if (!display) return renderAccountCell(row);
        const inContainer = row.fundings.length > 0 || display.accountSecondaryId !== null;
        const layoutStyle = inContainer ? ACCOUNT_CELL_LINK_STYLE : undefined;
        const primary = display.accountPrimaryName
          ? accountLink(display.accountPrimaryId, display.accountPrimaryName, layoutStyle)
          : placeholder("-", ACCOUNT_CELL_LINK_STYLE);
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
      title: options.expandPurchases
        ? t("transactions.columns.itemNote")
        : t("transactions.columns.note"),
      key: "note",
      render: (row) => {
        const item = options.expandPurchases ? purchaseOf(row) : null;
        if (item) return renderPurchaseCell(item, precomputedDisplayOf(row));
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
  if (options.rowActions) {
    columns.push({
      title: t("transactions.columns.actions"),
      key: "actions",
      width: 64,
      ...merged,
      render: (row) =>
        lightActionsButton(actionsAriaLabel, {
          [CELL_ACTION_ATTR]: "row-menu",
          "data-tx-row-key": displayRowKey(row),
        }),
    });
  }
  return columns;
}

export function rowActionsColumn<T>(
  onRowMenuOpen: (event: MouseEvent, row: T) => void,
  merge: Partial<Pick<DataTableBaseColumn<T>, "rowSpan" | "className">> = {},
): DataTableColumn<T> {
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

const ACCOUNT_CELL_LINK_STYLE = "flex: 0 1 auto; min-width: 0; text-align: left;";

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

const FUNDING_DERIVED_MARKER_STYLE =
  "font-size: 11px; line-height: 18px; padding: 0 6px; border-radius: 9px; white-space: nowrap; color: var(--n-info-color, #2080f0); border: 1px dashed currentColor;";

const ACCOUNT_CELL_SUFFIX_STYLE =
  "flex: none; color: var(--n-text-color-disabled, #999); font-size: 12px;";

function renderFundingAccountCell(row: Transaction): VNode {
  const first = row.fundings[0];
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

export function renderAccountCell(row: Transaction): VNode {
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
