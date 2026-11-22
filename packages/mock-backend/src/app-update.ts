import type { UpdateCheckOutcome, UpdateDownloadOutcome, UpdateSurface } from "@ledger/types";
import { MOCK_APP_VERSION } from "./settings";

export function checkForUpdates(): UpdateCheckOutcome {
  return { status: "same", version: null, notes: null };
}

export function getUpdateSurface(): UpdateSurface {
  return "updater";
}

export function downloadUpdate(): UpdateDownloadOutcome {
  return { version: MOCK_APP_VERSION };
}

export function installUpdate(): void {}
