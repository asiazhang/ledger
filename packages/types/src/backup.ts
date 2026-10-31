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

/** 备份触发来源（与后端权威枚举 `commands::backup::BackupKind` 对应，issue #129）。 */
export type BackupKind = "auto" | "manual";

export interface BackupFileInfo {
  file_name: string;
  path: string;
  size_bytes: number;
  created_at: string;
  /** 备份触发来源（issue #129）：元数据为权威，旧版本备份回落 manual。 */
  kind: BackupKind;
  /** 加密标记（issue #572 / ADR-0075 决策 7）：密文备份列表显示锁形标记；
   *  旧备份缺标记视为明文（向后兼容）。 */
  encrypted: boolean;
  /** 来源设备标识（ADR-0144 决策 5，issue #1985）：产物内层元数据的
   *  `device_id` 明文水印；缺水印（旧产物/残缺包/产出轮次设备不可得）为 null。 */
  source_device_id: string | null;
  /** 来源是否未知（issue #1985）：元数据读不出设备水印 = true，列表徽标呈
   *  「未知来源」；与 source_device_id 互为表里。 */
  source_unknown: boolean;
}

/** 备份包元数据摘要（issue #572）：单个备份文件的来源 + 加密标记，恢复确认弹窗消费。 */
export interface BackupMetaSummary {
  kind: BackupKind;
  encrypted: boolean;
}

export interface PruneResult {
  kept: number;
  deleted: string[];
  failed: string[];
}

/** 自动备份设置页状态（issue #128）：开关与上次自动备份时间（设置页仅消费这两项）。 */
export interface AutoBackupState {
  /** 自动备份开关（默认开启，状态存 ledger.db）。 */
  enabled: boolean;
  /** 上次成功自动备份时间（UTC ISO）；null 表示从未自动备份过。 */
  last_backup_at: string | null;
  /** 自动备份连续失败次数（issue #1456）：执行失败累加、成功清零；提示文案消费。 */
  consecutive_failures: number;
  /** 连续失败达提示阈值（后端单点判定，前端不复刻阈值）：
   *  true 时设置页自动备份卡片呈现失败提示。 */
  failure_alerting: boolean;
}
