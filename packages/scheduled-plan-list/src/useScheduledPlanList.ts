import { computed, readonly, ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { errorMessage } from "@ledger/utils/errors";
import { t } from "@ledger/i18n";
import { scheduledStatusLabel } from "@ledger/utils/scheduled";
import type {
  RecurrenceType,
  ScheduledKind,
  ScheduledStatus,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from "@ledger/types";

export interface ScheduledRecurrenceOption {
  label: string;
  value: RecurrenceType;
}

export function scheduledRecurrenceOptions(): ScheduledRecurrenceOption[] {
  const labelKeys: Record<RecurrenceType, string> = {
    daily: "scheduled.recurrence.daily",
    weekly: "scheduled.recurrence.weekly",
    monthly: "scheduled.recurrence.monthly",
    yearly: "scheduled.recurrence.yearly",
  };
  return (Object.keys(labelKeys) as RecurrenceType[]).map((value) => ({
    value,
    label: t(labelKeys[value]),
  }));
}

const RECURRENCE_UNIT_KEY: Record<RecurrenceType, string> = {
  daily: "scheduled.recurrence.unitDaily",
  weekly: "scheduled.recurrence.unitWeekly",
  monthly: "scheduled.recurrence.unitMonthly",
  yearly: "scheduled.recurrence.unitYearly",
};

export function scheduledRecurrenceLabel(
  recurrenceType: RecurrenceType | string,
  interval: number,
): string {
  const unitKey = RECURRENCE_UNIT_KEY[recurrenceType as RecurrenceType];
  const unit = unitKey ? t(unitKey) : recurrenceType;
  return interval > 1
    ? t("scheduled.recurrence.everyN", { n: interval, unit })
    : t("scheduled.recurrence.every", { unit });
}

export interface ScheduledPlanRow<E> {
  plan: ScheduledTransactionWithExt;
  detailFailed: boolean;
  ext: E;
}

export interface ScheduledPlanRowAction {
  key: "detail" | "edit" | "pause" | "resume" | "cancel";
  label: string;
  available: boolean;
  confirm: string | null;
  run(): void;
}

export interface ScheduledPlanStatusOption {
  key: ScheduledStatus;
  label: string;
}

const WITH_COMPLETED: readonly ScheduledStatus[] = ["active", "paused", "cancelled", "completed"];
const WITHOUT_COMPLETED: readonly ScheduledStatus[] = ["active", "paused", "cancelled"];

const STATUS_FILTER_KEYS: Record<ScheduledKind, ReadonlyArray<ScheduledStatus>> = {
  scheduled_transfer: WITH_COMPLETED,
  subscription: WITHOUT_COMPLETED,
  installment: WITH_COMPLETED,
};

function buildStatusFilterOptions(kind: ScheduledKind): ScheduledPlanStatusOption[] {
  return STATUS_FILTER_KEYS[kind].map((key) => ({ key, label: scheduledStatusLabel(key) }));
}

export function isUnfinishedStatus(status: ScheduledStatus): boolean {
  return status === "active" || status === "paused";
}

export interface UseScheduledPlanListOptions<E> {
  kind: ScheduledKind;
  expandDetail(plan: ScheduledTransactionWithExt, detail: ScheduledTransactionDetail | null): E;
  cancelConfirmText(): string;
  onStatusChanged?(): void;
  onOpenDetail(row: ScheduledPlanRow<E>): void;
}

export interface UseScheduledPlanListReturn<E> {
  readonly rows: Readonly<Ref<readonly ScheduledPlanRow<E>[]>>;
  readonly loading: Readonly<Ref<boolean>>;
  readonly error: Readonly<Ref<string | null>>;
  readonly statusFilter: Readonly<Ref<ScheduledStatus>>;
  readonly filteredRows: ComputedRef<ScheduledPlanRow<E>[]>;
  readonly statusFilterOptions: ComputedRef<ReadonlyArray<ScheduledPlanStatusOption>>;
  load(): Promise<void>;
  setStatusFilter(status: ScheduledStatus): void;
  changeStatus(id: string, newStatus: ScheduledStatus): Promise<void>;
  rowActions(row: ScheduledPlanRow<E>): ScheduledPlanRowAction[];
}

export function useScheduledPlanList<E>(
  options: UseScheduledPlanListOptions<E>,
): UseScheduledPlanListReturn<E> {
  const { kind, expandDetail, cancelConfirmText, onStatusChanged, onOpenDetail } = options;
  const message = useMessage();

  const rows = ref([]) as Ref<ScheduledPlanRow<E>[]>;
  const statusFilter = ref<ScheduledStatus>("active");

  const filteredRows = computed(() =>
    rows.value.filter((r) => r.plan.core.status === statusFilter.value),
  );

  const statusFilterOptions = computed(() => buildStatusFilterOptions(kind));

  const { loading, error, run } = useLoadable(async (): Promise<ScheduledPlanRow<E>[]> => {
    const plans = (await api.listScheduledTransactions()).filter((p) => p.core.kind === kind);
    return Promise.all(
      plans.map(async (p): Promise<ScheduledPlanRow<E>> => {
        try {
          const detail = await api.getScheduledTransactionDetail(p.core.id);
          return { plan: p, detailFailed: false, ext: expandDetail(p, detail) };
        } catch {
          return { plan: p, detailFailed: true, ext: expandDetail(p, null) };
        }
      }),
    );
  });

  async function load() {
    const loaded = await run();
    if (loaded !== null) rows.value = loaded;
  }

  function setStatusFilter(status: ScheduledStatus) {
    statusFilter.value = status;
  }

  async function changeStatus(id: string, newStatus: ScheduledStatus) {
    try {
      await api.updateScheduledTransactionStatus({ id, new_status: newStatus });
      message.success(
        newStatus === "paused"
          ? t("scheduled.status.paused")
          : newStatus === "active"
            ? t("scheduled.toast.resumed")
            : t("scheduled.status.cancelled"),
      );
      await load();
      onStatusChanged?.();
    } catch (e) {
      message.error(t("scheduled.toast.operationFailed", { message: errorMessage(e) }));
    }
  }

  function rowActions(row: ScheduledPlanRow<E>): ScheduledPlanRowAction[] {
    const id = row.plan.core.id;
    const status = row.plan.core.status;
    return [
      {
        key: "detail",
        label: t("scheduled.action.detail"),
        available: true,
        confirm: null,
        run: () => onOpenDetail(row),
      },
      {
        key: "pause",
        label: t("scheduled.action.pause"),
        available: status === "active",
        confirm: null,
        run: () => void changeStatus(id, "paused"),
      },
      {
        key: "resume",
        label: t("scheduled.action.resume"),
        available: status === "paused",
        confirm: null,
        run: () => void changeStatus(id, "active"),
      },
      {
        key: "cancel",
        label: t("scheduled.action.cancel"),
        available: isUnfinishedStatus(status),
        confirm: cancelConfirmText(),
        run: () => void changeStatus(id, "cancelled"),
      },
    ];
  }

  return {
    rows: readonly(rows) as Readonly<Ref<readonly ScheduledPlanRow<E>[]>>,
    loading: readonly(loading) as Readonly<Ref<boolean>>,
    error: readonly(error) as Readonly<Ref<string | null>>,
    statusFilter: readonly(statusFilter) as Readonly<Ref<ScheduledStatus>>,
    filteredRows,
    statusFilterOptions,
    load,
    setStatusFilter,
    changeStatus,
    rowActions,
  };
}

export function earliestPendingOccurrence(
  detail: ScheduledTransactionDetail,
): ScheduledTransactionOccurrence | null {
  return (
    [...detail.pending_occurrences].sort((a, b) =>
      a.scheduled_date.localeCompare(b.scheduled_date),
    )[0] ?? null
  );
}
