import { t } from "@ledger/i18n";
import type { ScheduledStatus, ScheduledTransactionOccurrence } from "@ledger/types";

export function scheduledStatusLabel(status: string): string {
  switch (status as ScheduledStatus) {
    case "active":
      return t("scheduled.status.active");
    case "paused":
      return t("scheduled.status.paused");
    case "cancelled":
      return t("scheduled.status.cancelled");
    case "completed":
      return t("scheduled.status.completed");
    default:
      return status;
  }
}

export function occurrenceStatusLabel(status: ScheduledTransactionOccurrence["status"]): string {
  switch (status) {
    case "pending":
      return t("scheduled.occurrenceStatus.pending");
    case "processing":
      return t("scheduled.occurrenceStatus.processing");
    case "completed":
      return t("scheduled.occurrenceStatus.completed");
    case "failed":
      return t("scheduled.occurrenceStatus.failed");
    case "cancelled":
      return t("scheduled.occurrenceStatus.cancelled");
    default:
      return status;
  }
}
