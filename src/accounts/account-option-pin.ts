import { t } from "@ledger/i18n";
import type { Account } from "@ledger/types";

export interface AccountOption {
  label: string;
  value: string;
}

export function pinArchivedAccountOption(
  options: AccountOption[],
  currentId: string | null | undefined,
  accountMap: ReadonlyMap<string, Account>,
): AccountOption[] {
  if (!currentId) return options;
  const account = accountMap.get(currentId);
  if (!account?.is_archived) return options;
  if (options.some((o) => o.value === currentId)) return options;
  return [
    { label: `${account.name}${t("accounts.archived.suffix")}`, value: currentId },
    ...options,
  ];
}
