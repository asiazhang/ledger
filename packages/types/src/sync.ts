export interface SyncInstrumentInfoResult {
  synced: number;
  skipped: number;
  message: string;
  bulk_degraded: boolean;
}

export interface InstrumentSyncProgress {
  done: number;
  total: number;
}
