import type { AccountType, CreateFormKind, Transaction, TransactionKind } from "@ledger/types";

export type LendingDirection = "lend" | "collect" | "borrow" | "repay" | "none";

export type LendingFormDirection = Exclude<LendingDirection, "none">;

export type LendingAccountSide = "fund" | "receivable" | "debt" | "unknown";

export function lendingAccountSide(type: AccountType | null | undefined): LendingAccountSide {
  if (type === "receivable" || type === "debt") return type;
  return type == null ? "unknown" : "fund";
}

export function accountMatchesSide(
  type: AccountType | null | undefined,
  side: LendingAccountSide,
): boolean {
  return lendingAccountSide(type) === side;
}

export const LENDING_DIRECTION_SIDES: Record<
  LendingFormDirection,
  { from: LendingAccountSide; to: LendingAccountSide }
> = {
  lend: { from: "fund", to: "receivable" },
  collect: { from: "receivable", to: "fund" },
  borrow: { from: "debt", to: "fund" },
  repay: { from: "fund", to: "debt" },
};

export const LENDING_FORM_DIRECTIONS = Object.keys(
  LENDING_DIRECTION_SIDES,
) as LendingFormDirection[];

const DIRECTION_BY_SIDE_PAIR: Record<
  LendingAccountSide,
  Partial<Record<LendingAccountSide, LendingDirection>>
> = { fund: {}, receivable: {}, debt: {}, unknown: {} };
for (const [direction, sides] of Object.entries(LENDING_DIRECTION_SIDES) as [
  LendingFormDirection,
  { from: LendingAccountSide; to: LendingAccountSide },
][]) {
  DIRECTION_BY_SIDE_PAIR[sides.from][sides.to] = direction;
}

function directionalSide(type: AccountType | null | undefined): LendingAccountSide {
  const side = lendingAccountSide(type);
  return side === "unknown" ? "fund" : side;
}

export function deriveLendingDirection(
  kind: TransactionKind,
  fromType: AccountType | null | undefined,
  toType: AccountType | null | undefined,
): LendingDirection {
  if (kind !== "transfer") return "none";
  return DIRECTION_BY_SIDE_PAIR[directionalSide(fromType)][directionalSide(toType)] ?? "none";
}

export function lendingLabelKey(direction: LendingDirection): string {
  return direction === "none" ? "transactions.kind.transfer" : `transactions.lending.${direction}`;
}

export function resolveLendingDirection(
  tx: Pick<Transaction, "kind" | "account_id" | "to_account_id">,
  accountType: (id: string) => AccountType | undefined,
): LendingFormDirection | null {
  const direction = deriveLendingDirection(
    tx.kind,
    tx.account_id == null ? undefined : accountType(tx.account_id),
    tx.to_account_id == null ? undefined : accountType(tx.to_account_id),
  );
  return direction === "none" ? null : direction;
}

export function isLendingEntryKind(kind: CreateFormKind): kind is "lend" | "borrow" {
  return kind === "lend" || kind === "borrow";
}
