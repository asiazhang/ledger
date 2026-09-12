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

/** 备份触发来源（与后端权威枚举 `commands::backup::BackupKind` 对应，issue #129）。 */
export type BackupKind = "auto" | "manual";

export interface BackupFileInfo {
  file_name: string
  path: string
  size_bytes: number
  created_at: string
  /** 备份触发来源（issue #129）：元数据为权威，旧版本备份回落 manual。 */
  kind: BackupKind
  /** 加密标记（issue #572 / ADR-0075 决策 7）：密文备份列表显示锁形标记；
   *  旧备份缺标记视为明文（向后兼容）。 */
  encrypted: boolean
}

/** 备份包元数据摘要（issue #572）：单个备份文件的来源 + 加密标记，恢复确认弹窗消费。 */
export interface BackupMetaSummary {
  kind: BackupKind
  encrypted: boolean
}

export interface PruneResult {
  kept: number
  deleted: string[]
  failed: string[]
}

/** 自动备份设置页状态（issue #128）：开关与上次自动备份时间（设置页仅消费这两项）。 */
export interface AutoBackupState {
  /** 自动备份开关（默认开启，状态存 ledger.db）。 */
  enabled: boolean
  /** 上次成功自动备份时间（UTC ISO）；null 表示从未自动备份过。 */
  last_backup_at: string | null
}
