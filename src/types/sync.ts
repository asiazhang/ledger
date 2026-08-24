export interface SyncProgress {
  current: number
  total: number
  market: string
  done: boolean
  total_inserted: number
  total_updated: number
  error: string | null
}
