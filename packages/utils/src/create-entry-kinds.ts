import {
  CREATE_KINDS,
  type CreateTransactionKind,
  type TransactionPageCreateKind,
} from "@ledger/types";

export function isCreateKindAvailable(kind: CreateTransactionKind): boolean {
  return (CREATE_KINDS as readonly string[]).includes(kind);
}

export function availableCreateKinds(): TransactionPageCreateKind[] {
  return CREATE_KINDS.filter(isCreateKindAvailable) as TransactionPageCreateKind[];
}
