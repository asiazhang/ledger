export interface BackupResult {
  path: string
  size_bytes: number
  schema_version: number
  created_at: string
}

export interface RestoreResult {
  schema_version: number
  restored_at: string
}

export interface BackupFileInfo {
  file_name: string
  path: string
  size_bytes: number
  created_at: string
}

export interface PruneResult {
  kept: number
  deleted: string[]
  failed: string[]
}
