export type UpdateCheckStatus = "update" | "same" | "older";

export interface UpdateCheckOutcome {
  status: UpdateCheckStatus;
  version: string | null;
  notes: string | null;
}

export interface UpdateAvailablePayload {
  version: string;
  notes: string | null;
}

export interface UpdateDownloadProgress {
  received: number;
  total: number | null;
}
export type UpdateSurface = "updater" | "release-page";

export interface UpdateDownloadOutcome {
  version: string;
}
