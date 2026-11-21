import type {
  AutoBackupState,
  BackupFileInfo,
  BackupMetaSummary,
  BackupResult,
  PruneResult,
  RestoreResult,
} from "@ledger/types";
import { nowIso, reqBool, str } from "./state";

const MOCK_SCHEMA_VERSION = 36;

export function createBackup(args: Record<string, unknown>): BackupResult {
  return {
    path: str(args, "targetPath"),
    size_bytes: 0,
    schema_version: MOCK_SCHEMA_VERSION,
    created_at: nowIso(),
  };
}

export function restoreBackup(args: Record<string, unknown>): RestoreResult {
  str(args, "backupPath");
  return { schema_version: MOCK_SCHEMA_VERSION, restored_at: nowIso() };
}

export function getBackupMeta(args: Record<string, unknown>): BackupMetaSummary {
  str(args, "path");
  return { kind: "manual", encrypted: false };
}

export function listBackups(args: Record<string, unknown>): BackupFileInfo[] {
  str(args, "dir");
  return [];
}

export function getBookFingerprint(): string | null {
  return null;
}

export function getDeviceId(): string | null {
  return null;
}

export function pruneBackups(args: Record<string, unknown>): PruneResult {
  str(args, "dir");
  return { kept: 0, deleted: [], failed: [] };
}

export function setAutoBackupDir(args: Record<string, unknown>): void {
  str(args, "dir");
}

export function getAutoBackupState(): AutoBackupState {
  return { enabled: true, last_backup_at: null, consecutive_failures: 0, failure_alerting: false };
}

export function setAutoBackupEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled");
}
