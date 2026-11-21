import { useRouter } from "vue-router";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { resolveSourceJumpTarget, type TransactionSourceKind } from "@/components/source-jump";
import { copyNoteWithToast } from "@ledger/ui-kit/note-copy";
import type { MessageApi } from "naive-ui";

export const CELL_ACTION_ATTR = "data-tx-action";

export interface TableCellActionHandlers {
  accountDrill(accountId: string): void;
  merchantDrill(merchantId: string): void;
  sourceJump(kind: TransactionSourceKind, entityId: string): void;
  copyNote(note: string): void;
  rowMenu?(event: MouseEvent, rowKey: string): void;
}

export function dispatchTableCellAction(
  event: MouseEvent,
  handlers: TableCellActionHandlers,
): void {
  const target = event.target as HTMLElement | null;
  const el = target?.closest?.(`[${CELL_ACTION_ATTR}]`) as HTMLElement | null;
  if (!el) return;
  const dataset = el.dataset;
  switch (dataset.txAction) {
    case "account-drill":
      if (dataset.txAccount) handlers.accountDrill(dataset.txAccount);
      return;
    case "merchant-drill":
      if (dataset.txMerchant) handlers.merchantDrill(dataset.txMerchant);
      return;
    case "source-jump":
      if (dataset.txSourceKind && dataset.txSourceEntity) {
        handlers.sourceJump(dataset.txSourceKind as TransactionSourceKind, dataset.txSourceEntity);
      }
      return;
    case "copy-note":
      handlers.copyNote(dataset.txNote ?? "");
      return;
    case "row-menu":
      if (handlers.rowMenu && dataset.txRowKey) handlers.rowMenu(event, dataset.txRowKey);
      return;
    default:
      return;
  }
}

export function useTransactionTableCellActions(options: {
  message: MessageApi;
  onRowMenu?: (event: MouseEvent, rowKey: string) => void;
}): (event: MouseEvent) => void {
  const router = useRouter();
  const sidebarOrder = useSidebarOrderStore();
  const featureToggles = useFeatureToggleStore();
  return (event: MouseEvent) =>
    dispatchTableCellAction(event, {
      accountDrill: (accountId) => {
        void router.push({ name: "transactions", query: { account: accountId } });
      },
      merchantDrill: (merchantId) => {
        void router.push({ name: "transactions", query: { merchant: merchantId } });
      },
      sourceJump: (kind, entityId) => {
        void router.push(
          resolveSourceJumpTarget(
            kind,
            entityId,
            (v) => sidebarOrder.isViewContained(v),
            (v) => featureToggles.isFeatureClosed(v),
          ),
        );
      },
      copyNote: (note) => {
        void copyNoteWithToast(options.message, note);
      },
      rowMenu: options.onRowMenu,
    });
}
