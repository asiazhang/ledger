import {
  ArchiveOutline,
  ArrowUndoOutline,
  CreateOutline,
  SwapHorizontalOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import type { DropdownOption } from "naive-ui";
import { t } from "@ledger/i18n";
import { errorOptionProps, renderRowMenuIcon } from "@/components/row-menu-common";

export function buildAccountRowMenuOptions(opts: { errorColor?: string } = {}): DropdownOption[] {
  return [
    { label: t("accounts.menu.edit"), key: "edit", icon: renderRowMenuIcon(CreateOutline) },
    {
      label: t("accounts.menu.adjustBalance"),
      key: "adjust-balance",
      icon: renderRowMenuIcon(SwapHorizontalOutline),
    },
    {
      label: t("accounts.menu.archive"),
      key: "archive",
      icon: renderRowMenuIcon(ArchiveOutline),
    },
    { type: "divider", key: "menu-divider" },
    {
      label: t("accounts.menu.delete"),
      key: "delete",
      icon: renderRowMenuIcon(TrashOutline),
      ...errorOptionProps(opts.errorColor),
    },
  ];
}

export function buildArchivedAccountRowMenuOptions(): DropdownOption[] {
  return [
    {
      label: t("accounts.menu.restore"),
      key: "restore",
      icon: renderRowMenuIcon(ArrowUndoOutline),
    },
  ];
}
