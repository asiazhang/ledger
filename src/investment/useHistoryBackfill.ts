import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import type { InstrumentSyncProgress } from "@ledger/types";

export const HISTORY_BACKFILL_PROGRESS_EVENT = "ledger:history-backfill-progress";

const progress = ref<InstrumentSyncProgress | null>(null);

let subscribed = false;

function ensureProgressSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  void listen<InstrumentSyncProgress>(HISTORY_BACKFILL_PROGRESS_EVENT, (event) => {
    const payload = event.payload as Partial<InstrumentSyncProgress> | undefined;
    if (
      !payload ||
      typeof payload.done !== "number" ||
      Number.isFinite(payload.done) === false ||
      typeof payload.total !== "number" ||
      Number.isFinite(payload.total) === false
    ) {
      return;
    }
    progress.value =
      payload.done >= payload.total ? null : { done: payload.done, total: payload.total };
  }).catch((e) => {
    console.warn(`订阅 ${HISTORY_BACKFILL_PROGRESS_EVENT} 失败`, e);
  });
}

export function resetHistoryBackfillForTest(): void {
  progress.value = null;
  subscribed = false;
}

export function useHistoryBackfill() {
  ensureProgressSubscription();
  return { progress };
}
