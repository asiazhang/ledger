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

/** 自动备份调度状态（issue #128）：设置页读写开关与展示上次自动备份时间。 */
export interface AutoBackupState {
  /** 自动备份开关（默认开启，状态存 ledger.db）。 */
  enabled: boolean
  /** 脏标记：数据变动后置真，自动备份成功后复位。 */
  dirty: boolean
  /** 上次成功自动备份时间（UTC ISO）；null 表示从未自动备份过。 */
  last_backup_at: string | null
}
