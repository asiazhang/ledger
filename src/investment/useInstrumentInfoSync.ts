import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import type { InstrumentSyncProgress, SyncInstrumentInfoResult } from "@ledger/types";

type InstrumentInfoSyncStatus = "idle" | "success" | "error";
export type InstrumentInfoSyncOutcome = Exclude<InstrumentInfoSyncStatus, "idle">;

export const INSTRUMENT_SYNC_PROGRESS_EVENT = "ledger:instrument-sync-progress";

const syncing = ref(false);
const resultMessage = ref<string | null>(null);
const status = ref<InstrumentInfoSyncStatus>("idle");
const lastResult = ref<SyncInstrumentInfoResult | null>(null);
const degraded = ref(false);
const progress = ref<InstrumentSyncProgress | null>(null);

let inFlightSyncs = 0;

let inFlight: Promise<InstrumentInfoSyncOutcome> | null = null;

let subscribed = false;

function ensureProgressSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  void listen<InstrumentSyncProgress>(INSTRUMENT_SYNC_PROGRESS_EVENT, (event) => {
    const payload = event.payload as Partial<InstrumentSyncProgress> | undefined;
    if (
      inFlightSyncs > 0 &&
      payload &&
      typeof payload.done === "number" &&
      Number.isFinite(payload.done) &&
      typeof payload.total === "number" &&
      Number.isFinite(payload.total)
    ) {
      progress.value = { done: payload.done, total: payload.total };
    }
  }).catch((e) => {
    console.warn(`订阅 ${INSTRUMENT_SYNC_PROGRESS_EVENT} 失败`, e);
  });
}

export function resetInstrumentInfoSyncForTest(): void {
  syncing.value = false;
  resultMessage.value = null;
  status.value = "idle";
  lastResult.value = null;
  progress.value = null;
  degraded.value = false;
  inFlightSyncs = 0;
  inFlight = null;
  subscribed = false;
}

export function useInstrumentInfoSync() {
  ensureProgressSubscription();

  function sync(): Promise<InstrumentInfoSyncOutcome> {
    if (inFlight) return inFlight;
    syncing.value = true;
    resultMessage.value = null;
    status.value = "idle";
    lastResult.value = null;
    progress.value = null;
    degraded.value = false;
    inFlightSyncs += 1;
    inFlight = (async () => {
      try {
        const res = await api.syncInstrumentInfo();
        lastResult.value = res;
        resultMessage.value = res.message;
        degraded.value = res.bulk_degraded === true;
        status.value = "success";
        return "success";
      } catch (e: any) {
        const detail = e instanceof Error ? e.message : String(e);
        resultMessage.value = t("investments.sync.failed", { message: detail });
        status.value = "error";
        return "error";
      } finally {
        syncing.value = false;
        inFlightSyncs -= 1;
        progress.value = null;
        inFlight = null;
      }
    })();
    return inFlight;
  }

  return { syncing, resultMessage, status, lastResult, progress, degraded, sync };
}
