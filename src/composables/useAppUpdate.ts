import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import type { UpdateSurface } from "@ledger/types";
import { useAppStore } from "@/stores/app";

export const UPDATE_AVAILABLE_EVENT = "ledger:update-available";

export const UPDATE_DOWNLOAD_PROGRESS_EVENT = "ledger:update-download-progress";

export const RELEASE_PAGE_URL = "https://github.com/asiazhang/ledger/releases/latest";

type UpdateStage = "idle" | "remind" | "downloading" | "downloaded";

type ManualCheckState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "error"; message: string };

const surface = ref<UpdateSurface | null>(null);

const stage = ref<UpdateStage>("idle");
const version = ref<string | null>(null);
const notes = ref<string | null>(null);
const received = ref(0);
const total = ref<number | null>(null);

const dismissed = ref(false);

const flowError = ref<string | null>(null);
const installing = ref(false);

const manualCheck = ref<ManualCheckState>({ kind: "idle" });

const modalOpen = computed(() => stage.value !== "idle" && !dismissed.value);

const downloadPercent = computed<number | null>(() => {
  if (stage.value !== "downloading" || total.value === null || total.value <= 0) return null;
  return Math.min(100, Math.floor((received.value / total.value) * 100));
});

let subscribed = false;

function ensureEventSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  void listen<{ version?: unknown; notes?: unknown }>(UPDATE_AVAILABLE_EVENT, (event) => {
    const payload = event.payload;
    if (typeof payload?.version !== "string" || payload.version === "") return;
    offerUpdate(payload.version, typeof payload.notes === "string" ? payload.notes : null);
  }).catch((e) => {
    console.warn(`订阅 ${UPDATE_AVAILABLE_EVENT} 失败`, e);
    subscribed = false;
  });
  void listen<{ received?: unknown; total?: unknown }>(UPDATE_DOWNLOAD_PROGRESS_EVENT, (event) => {
    const payload = event.payload;
    if (stage.value !== "downloading") return;
    if (typeof payload?.received !== "number" || !Number.isFinite(payload.received)) return;
    received.value = payload.received;
    total.value =
      typeof payload.total === "number" && Number.isFinite(payload.total) ? payload.total : null;
  }).catch((e) => {
    console.warn(`订阅 ${UPDATE_DOWNLOAD_PROGRESS_EVENT} 失败`, e);
  });
}

function offerUpdate(newVersion: string, newNotes: string | null): void {
  if (surface.value !== "updater") return;
  const store = useAppStore();
  if (store.updateRemindedVersion === newVersion) return;
  if (stage.value === "downloading" || stage.value === "downloaded") return;
  version.value = newVersion;
  notes.value = newNotes;
  stage.value = "remind";
  dismissed.value = false;
  flowError.value = null;
  store.markUpdateReminded(newVersion);
}

async function init(): Promise<void> {
  if (surface.value === null) {
    try {
      surface.value = await api.getUpdateSurface();
    } catch (e) {
      console.warn("查询更新入口形态失败", e);
      surface.value = "release-page";
    }
  }
  ensureEventSubscription();
}

async function checkManually(): Promise<void> {
  if (surface.value !== "updater" || manualCheck.value.kind === "checking") return;
  manualCheck.value = { kind: "checking" };
  try {
    const outcome = await api.checkForUpdates();
    if (outcome.status === "update" && outcome.version !== null) {
      manualCheck.value = {
        kind: "available",
        version: outcome.version,
        notes: outcome.notes,
      };
    } else {
      manualCheck.value = { kind: "latest" };
    }
  } catch (e) {
    manualCheck.value = { kind: "error", message: errorMessage(e) };
  }
}

async function startDownload(): Promise<void> {
  if (stage.value === "idle") {
    if (manualCheck.value.kind !== "available") return;
    version.value = manualCheck.value.version;
    notes.value = manualCheck.value.notes;
    stage.value = "remind";
  }
  if (stage.value !== "remind" || version.value === null) return;
  stage.value = "downloading";
  received.value = 0;
  total.value = null;
  flowError.value = null;
  try {
    const outcome = await api.downloadUpdate();
    stage.value = "downloaded";
    version.value = outcome.version;
  } catch (e) {
    stage.value = "remind";
    flowError.value = errorMessage(e);
  }
}

function dismissFlow(): void {
  if (stage.value === "remind") {
    stage.value = "idle";
    version.value = null;
    notes.value = null;
    flowError.value = null;
    return;
  }
  dismissed.value = true;
}

function reopenFlow(): void {
  dismissed.value = false;
}

async function confirmInstall(): Promise<void> {
  if (stage.value !== "downloaded" || installing.value) return;
  flowError.value = null;
  installing.value = true;
  try {
    await api.installUpdate();
  } catch (e) {
    flowError.value = errorMessage(e);
  } finally {
    installing.value = false;
  }
}

async function openReleasePage(): Promise<void> {
  try {
    await openUrl(RELEASE_PAGE_URL);
  } catch (e) {
    console.warn("打开发布页失败", e);
  }
}

export function resetAppUpdateForTest(): void {
  surface.value = null;
  stage.value = "idle";
  version.value = null;
  notes.value = null;
  received.value = 0;
  total.value = null;
  dismissed.value = false;
  flowError.value = null;
  installing.value = false;
  manualCheck.value = { kind: "idle" };
  subscribed = false;
}

export function useAppUpdate() {
  return {
    surface,
    stage,
    version,
    notes,
    downloadPercent,
    modalOpen,
    flowError,
    installing,
    manualCheck,
    init,
    checkManually,
    startDownload,
    dismissFlow,
    reopenFlow,
    confirmInstall,
    openReleasePage,
  };
}
