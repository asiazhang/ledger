import { computed, ref } from "vue";
import { useTransferForm } from "@/transaction/useTransferForm";
import { useFormShared, usePinnedAccountOptions } from "@/composables/useFormShared";
import { t } from "@ledger/i18n";
import {
  LENDING_DIRECTION_SIDES,
  accountMatchesSide,
  resolveLendingDirection,
  type LendingFormDirection,
} from "@/transaction/lending";
import type { AccountType, TransactionModalRow } from "@ledger/types";

export function useLendingForm(options?: {
  initialDirection?: LendingFormDirection;
  onCreated?: () => void;
  onUpdated?: () => void;
  editing?: () => TransactionModalRow | null;
}) {
  const { reference } = useFormShared();

  const accountTypeOf = (id: string | null | undefined): AccountType | undefined =>
    id == null ? undefined : reference.accountMap.get(id)?.type;

  const editingTx = options?.editing?.() ?? null;
  const direction = ref<LendingFormDirection>(
    editingTx
      ? (resolveLendingDirection(editingTx, accountTypeOf) ?? options?.initialDirection ?? "lend")
      : (options?.initialDirection ?? "lend"),
  );

  const transfer = useTransferForm({
    onCreated: options?.onCreated,
    onUpdated: options?.onUpdated,
    editing: options?.editing,
    createdMessage: () =>
      t("transactions.lending.created", {
        dir: t(`transactions.lending.${direction.value}`),
      }),
  });

  function selectableOptionsFor(sideKey: "from" | "to") {
    return computed(() => {
      const side = LENDING_DIRECTION_SIDES[direction.value][sideKey];
      return reference.selectableAccounts
        .filter((a) => accountMatchesSide(a.type, side))
        .map((a) => ({ label: a.name, value: a.id }));
    });
  }

  const fromAccountOptions = usePinnedAccountOptions(
    transfer.accountId,
    selectableOptionsFor("from"),
  );

  const toAccountOptions = usePinnedAccountOptions(
    transfer.toAccountId,
    selectableOptionsFor("to"),
  );

  function setDirection(next: LendingFormDirection) {
    if (next === direction.value) return;
    const { from, to } = LENDING_DIRECTION_SIDES[next];
    const fromId = transfer.accountId.value;
    const toId = transfer.toAccountId.value;
    if (
      fromId != null &&
      toId != null &&
      accountMatchesSide(accountTypeOf(toId), from) &&
      accountMatchesSide(accountTypeOf(fromId), to)
    ) {
      transfer.accountId.value = toId;
      transfer.toAccountId.value = fromId;
    } else {
      if (fromId != null && !accountMatchesSide(accountTypeOf(fromId), from)) {
        transfer.accountId.value = null;
      }
      if (toId != null && !accountMatchesSide(accountTypeOf(toId), to)) {
        transfer.toAccountId.value = null;
      }
    }
    direction.value = next;
  }

  return {
    ...transfer,
    direction,
    fromAccountOptions,
    toAccountOptions,
    setDirection,
  };
}
