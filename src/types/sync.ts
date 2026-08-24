export interface SyncProgress {
  current: number
  total: number
  market: string
  done: boolean
  total_inserted: number
  total_updated: number
  error: string | null
}

/// 同步完成后的展示结果（由 SyncProgress 的 total_inserted/total_updated 派生）。
export interface SyncResult {
  inserted: number
  updated: number
}
