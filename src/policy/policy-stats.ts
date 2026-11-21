import { formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import type { PolicyStats } from "@ledger/types";

export function policyStatAmountText(
  stats: PolicyStats | null | undefined,
  pick: (s: PolicyStats) => number,
): string {
  if (!stats) return "—";
  const currency = useReferenceStore().getCurrency(stats.native_currency);
  return formatAmount(pick(stats), currency);
}
