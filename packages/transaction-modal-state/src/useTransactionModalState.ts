import { useMessage } from "naive-ui";
import type { Ref } from "vue";
import { useModalIntent } from "@ledger/modal-intent";
import { createLatestWins, type LatestWinsToken } from "@ledger/latest-wins";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import { t } from "@ledger/i18n";
import type {
  CreateFormKind,
  Transaction,
  TransactionConvert,
  TransactionModalRow,
  TransactionSplit,
  TransactionTrade,
} from "@ledger/types";

export type TransactionDetailPayload =
  | { kind: "convert"; convert: TransactionConvert }
  | { kind: "split"; split: TransactionSplit }
  | { kind: "dividend" }
  | { kind: "funding" }
  | { kind: "purchases" }
  | { kind: "order" };

export type TransactionModalIntent =
  | { type: "create"; kind: CreateFormKind }
  | { type: "refund"; row: Transaction }
  | { type: "detail"; row: TransactionModalRow; detail: TransactionDetailPayload }
  | { type: "edit"; row: TransactionModalRow; trade: TransactionTrade | null }
  | { type: "add-item"; row: Transaction };

export type TransactionModalOpenRequest =
  | { type: "create"; kind: CreateFormKind }
  | { type: "refund"; row: Transaction }
  | { type: "detail"; row: TransactionModalRow }
  | { type: "edit"; row: TransactionModalRow }
  | { type: "add-item"; row: Transaction };

export interface UseTransactionModalStateReturn {
  readonly intent: Readonly<Ref<TransactionModalIntent | null>>;
  readonly seq: Readonly<Ref<number>>;
  open(request: TransactionModalOpenRequest): Promise<void>;
  close(): void;
}

export function useTransactionModalState(): UseTransactionModalStateReturn {
  const message = useMessage();

  const {
    intent,
    seq,
    open: landIntent,
    close: clearIntent,
  } = useModalIntent<TransactionModalIntent>();

  const wins = createLatestWins();

  function settle(myToken: LatestWinsToken, next: TransactionModalIntent) {
    if (myToken.isStale()) return;
    landIntent(next);
  }

  async function open(request: TransactionModalOpenRequest): Promise<void> {
    const myToken = wins.begin();
    if (request.type === "create") {
      settle(myToken, { type: "create", kind: request.kind });
      return;
    }
    if (request.type === "refund" || request.type === "add-item") {
      settle(myToken, { type: request.type, row: request.row });
      return;
    }
    const { row } = request;
    if (request.type === "detail") {
      if (row.fundings.length > 0) {
        settle(myToken, { type: "detail", row, detail: { kind: "funding" } });
        return;
      }
      if (row.kind === "dividend") {
        settle(myToken, { type: "detail", row, detail: { kind: "dividend" } });
        return;
      }
      if (row.purchases.length > 0) {
        settle(myToken, { type: "detail", row, detail: { kind: "purchases" } });
        return;
      }
      if (row.kind !== "convert" && row.kind !== "split") {
        if (row.source_order_no != null) {
          settle(myToken, { type: "detail", row, detail: { kind: "order" } });
          return;
        }
        return;
      }
      try {
        const detail: TransactionDetailPayload =
          row.kind === "convert"
            ? { kind: "convert", convert: await api.getTransactionConvert(row.id) }
            : { kind: "split", split: await api.getTransactionSplit(row.id) };
        settle(myToken, { type: "detail", row, detail });
      } catch (e) {
        if (myToken.isStale()) return;
        message.error(t("transactions.detail.loadFailed", { msg: errorMessage(e) }));
      }
      return;
    }
    if (row.kind !== "buy" && row.kind !== "sell") {
      settle(myToken, { type: "edit", row, trade: null });
      return;
    }
    try {
      const trade = await api.getTransactionTrade(row.id);
      settle(myToken, { type: "edit", row, trade });
    } catch (e) {
      if (myToken.isStale()) return;
      message.error(t("transactions.modal.editFailed", { msg: errorMessage(e) }));
    }
  }

  function close() {
    wins.invalidate();
    clearIntent();
  }

  return {
    intent,
    seq,
    open,
    close,
  };
}
