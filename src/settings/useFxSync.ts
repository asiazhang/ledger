import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useLoadable } from "@ledger/loadable";
import type { ExchangeRateSyncReport, FxSyncProgress, FxSyncStage } from "@ledger/types";

export const FX_SYNC_PROGRESS_EVENT = "ledger:fx-sync-progress";

const report = ref<ExchangeRateSyncReport | null>(null);
const stage = ref<FxSyncStage | null>(null);
const parsedDays = ref<number | null>(null);

const syncLoad = useLoadable(async () => {
  const result = await api.syncExchangeRates();
  report.value = result;
  return result;
});

let inFlightSyncs = 0;

let subscribed = false;

function ensureProgressSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  void listen<FxSyncProgress>(FX_SYNC_PROGRESS_EVENT, (event) => {
    const payload = event.payload as Partial<FxSyncProgress> | undefined;
    if (inFlightSyncs <= 0 || !payload) return;
    if (payload.stage === "fetching") {
      stage.value = "fetching";
      return;
    }
    if (
      payload.stage === "persisting" &&
      typeof payload.days_parsed === "number" &&
      Number.isFinite(payload.days_parsed)
    ) {
      stage.value = "persisting";
      parsedDays.value = payload.days_parsed;
    }
  }).catch((e) => {
    console.warn(`订阅 ${FX_SYNC_PROGRESS_EVENT} 失败`, e);
  });
}

export function resetFxSyncForTest(): void {
  report.value = null;
  stage.value = null;
  parsedDays.value = null;
  syncLoad.invalidate();
  syncLoad.error.value = null;
  inFlightSyncs = 0;
  subscribed = false;
}

export function useFxSync() {
  ensureProgressSubscription();

  const stageText = computed(() => {
    if (stage.value === "fetching") return t("settings.fxSync.stageFetching");
    if (stage.value === "persisting" && parsedDays.value !== null) {
      return t("settings.fxSync.stagePersisting", { days: parsedDays.value });
    }
    return "";
  });

  async function sync(): Promise<ExchangeRateSyncReport | null> {
    stage.value = null;
    parsedDays.value = null;
    report.value = null;
    inFlightSyncs += 1;
    try {
      return await syncLoad.run();
    } finally {
      inFlightSyncs -= 1;
      if (inFlightSyncs <= 0) {
        stage.value = null;
        parsedDays.value = null;
      }
    }
  }

  return {
    syncing: syncLoad.loading,
    syncError: syncLoad.error,
    report,
    stage,
    parsedDays,
    stageText,
    sync,
  };
}
