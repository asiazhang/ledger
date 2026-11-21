import type { DropdownOption } from "naive-ui";
import {
  AddCircleOutline,
  CashOutline,
  CreateOutline,
  EyeOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import { errorOptionProps, renderRowMenuIcon } from "@/components/row-menu-common";
import { t } from "@ledger/i18n";
import { transactionKindActivation, type Transaction } from "@ledger/types";

export { renderRowMenuIcon };

export type TransactionMenuRow = Pick<
  Transaction,
  "kind" | "fundings" | "purchases" | "source_order_no"
>;

export function supportsRowEdit(row: TransactionMenuRow): boolean {
  return (
    row.fundings.length === 0 &&
    row.purchases.length === 0 &&
    transactionKindActivation(row.kind) === "edit"
  );
}

export function supportsRowDetail(row: TransactionMenuRow): boolean {
  return (
    row.fundings.length > 0 ||
    row.purchases.length > 0 ||
    row.source_order_no != null ||
    transactionKindActivation(row.kind) === "detail"
  );
}

export function buildRowMenuOptions(
  row: TransactionMenuRow,
  opts: { hasItem?: boolean; errorColor?: string } = {},
): DropdownOption[] {
  const options: DropdownOption[] = [];
  if (supportsRowDetail(row)) {
    options.push({
      label: t("transactions.menu.detail"),
      key: "detail",
      icon: renderRowMenuIcon(EyeOutline),
    });
  }
  if (transactionKindActivation(row.kind) === "detail") return options;
  if (supportsRowEdit(row)) {
    options.push({
      label: t("transactions.menu.edit"),
      key: "edit",
      icon: renderRowMenuIcon(CreateOutline),
    });
  }
  if (row.kind === "expense") {
    if (row.fundings.length === 0) {
      options.push({
        label: t("transactions.menu.refund"),
        key: "refund",
        icon: renderRowMenuIcon(CashOutline),
      });
    }
    options.push({
      label: t("transactions.menu.addItem"),
      key: "add-item",
      disabled: opts.hasItem === true,
      icon: renderRowMenuIcon(AddCircleOutline),
    });
  }
  if (options.length > 0) {
    options.push({ type: "divider", key: "menu-divider" });
  }
  const errorProps = errorOptionProps(opts.errorColor);
  options.push({
    label: t("transactions.menu.delete"),
    key: "delete",
    icon: renderRowMenuIcon(TrashOutline),
    ...errorProps,
  });
  return options;
}
