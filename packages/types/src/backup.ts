export interface BackupResult {
  path: string;
  size_bytes: number;
  schema_version: number;
  created_at: string;
}

export interface RestoreResult {
  schema_version: number;
  restored_at: string;
}

export type BackupKind = "auto" | "manual";

export interface BackupFileInfo {
  file_name: string;
  path: string;
  size_bytes: number;
  created_at: string;
  kind: BackupKind;
  encrypted: boolean;
  source_device_id: string | null;
  source_unknown: boolean;
}

export interface BackupMetaSummary {
  kind: BackupKind;
  encrypted: boolean;
}

export interface PruneResult {
  kept: number;
  deleted: string[];
  failed: string[];
}

export interface AutoBackupState {
  enabled: boolean;
  last_backup_at: string | null;
  consecutive_failures: number;
  failure_alerting: boolean;
}
