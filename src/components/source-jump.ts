import { groupOfView, type SidebarGroupId } from "@/stores/sidebar-order";
import type { TransactionSourceKind } from "@ledger/types";

export type { TransactionSourceKind } from "@ledger/types";

export type ScheduledFormTab = "subscriptions" | "installments" | "transfers";

export type SourceTargetView = "scheduled" | "policies" | "items" | "investments";

const KIND_TARGETS: Record<
  TransactionSourceKind,
  { view: SourceTargetView; formTab?: ScheduledFormTab }
> = {
  installmentPlan: { view: "scheduled", formTab: "installments" },
  subscription: { view: "scheduled", formTab: "subscriptions" },
  scheduledTransfer: { view: "scheduled", formTab: "transfers" },
  policy: { view: "policies" },
  item: { view: "items" },
  instrument: { view: "investments" },
};

type SourceJumpRouteName = SourceTargetView | "bookkeeping-more" | "assets-more" | "insights-more";

const GROUP_MORE_ROUTE: Record<SidebarGroupId, SourceJumpRouteName> = {
  bookkeeping: "bookkeeping-more",
  assets: "assets-more",
  insights: "insights-more",
};

export interface SourceJumpTarget {
  name: SourceJumpRouteName;
  query: Record<string, string>;
}

export function resolveSourceJumpTarget(
  kind: TransactionSourceKind,
  entityId: string,
  isContained: (view: SourceTargetView) => boolean,
  isClosed: (view: SourceTargetView) => boolean,
): SourceJumpTarget {
  const { view, formTab } = KIND_TARGETS[kind];

  if (isContained(view) && !isClosed(view)) {
    const gid = groupOfView(view);
    if (gid) {
      const query: Record<string, string> = { tab: view, focus: entityId };
      if (formTab) query.scheduledTab = formTab;
      return { name: GROUP_MORE_ROUTE[gid], query };
    }
  }

  const query: Record<string, string> = { focus: entityId };
  if (formTab) query.tab = formTab;
  return { name: view, query };
}
