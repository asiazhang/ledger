import type {
  AutoBackupState,
  BackupFileInfo,
  BackupMetaSummary,
  BackupResult,
  PruneResult,
  RestoreResult,
} from "@ledger/types";
import { nowIso, reqBool, str } from "./state";

/**
 * 备份域替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不模拟文件
 * 系统备份引擎——创建/恢复答占位回执（路径回显 plugin-dialog 假路径，文件
 * 不落盘），元数据答明文手动占位，清单空、修剪零动作；自动备份状态答
 * 「默认开启、从未备份、零失败」常量，开关与目录推送 void。不发
 * `ledger:backups-changed`：mock 不真建备份，清单恒空无失效可通知。
 */

/**
 * 占位 schema 版本：手写对齐迁移数量（user_version 随迁移数增长，ADR-0143
 * 「代价」节：字面量漂移无害，前端不消费该值）。
 */
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
  str(args, "backupPath"); // 口令可空（明文备份传 null），只窄化必需的路径
  return { schema_version: MOCK_SCHEMA_VERSION, restored_at: nowIso() };
}

export function getBackupMeta(args: Record<string, unknown>): BackupMetaSummary {
  str(args, "path");
  return { kind: "manual", encrypted: false }; // 明文手动备份占位
}

export function listBackups(args: Record<string, unknown>): BackupFileInfo[] {
  str(args, "dir");
  return [];
}

export function getBookFingerprint(): string | null {
  // mock 无库可算指纹：答 null（前端手动命名退化为无标识旧命名，产物可靠）。
  return null;
}

export function getDeviceId(): string | null {
  // mock 无库可取设备标识：答 null（清理确认弹窗待删数量判定为空，不误报）。
  return null;
}

export function pruneBackups(args: Record<string, unknown>): PruneResult {
  str(args, "dir");
  // keep 参数 api 层恒传；mock 清单恒空，保留数无消费面，收下不校验上界
  return { kept: 0, deleted: [], failed: [] };
}

export function setAutoBackupDir(args: Record<string, unknown>): void {
  str(args, "dir"); // 收镜像推送即成功，不落内存
}

export function getAutoBackupState(): AutoBackupState {
  return { enabled: true, last_backup_at: null, consecutive_failures: 0, failure_alerting: false };
}

export function setAutoBackupEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled"); // 收开关推送即成功，状态保持常量（前端自持显示值）
}
