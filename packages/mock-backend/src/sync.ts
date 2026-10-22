import type {
  ParkedOpInfo,
  SyncChannelConfig,
  SyncCheckpointInfo,
  SyncCheckpointPublished,
  SyncBootstrapOutcome,
  SyncRoundReport,
  SyncStatus,
} from "@ledger/types";
import { DEVICE_ID } from "./state";

/**
 * 多端同步域替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不模拟
 * 同步引擎与 S3 通道——状态恒「未配置、无挂起、明文库」（与加密域明文常量
 * 同源），手动同步答零计数轮次报告，挂起清单空；通道配置按未配置空表单
 * 读写（保存不落内存，回显恒空表单），测试连接恒成功；检查点预检答 null
 * （向导走「通道无检查点」支路），发布与引导答占位回执。同步轮次、上传
 * 下载与进度事件都不模拟。
 */

/** 未配置通道的空表单回显（configured=false，S3 七字段 + 同步空间全空）。 */
const UNCONFIGURED_CHANNEL: SyncChannelConfig = {
  space_id: "",
  endpoint: "",
  region: "",
  bucket: "",
  prefix: "",
  access_key: "",
  secret_key: "",
  path_style: false,
  configured: false,
};

/** 必需对象参数窄化（通道表单）：缺失或非对象报错（与 str() 同语义）。 */
function reqConfig(args: Record<string, unknown>): void {
  const v = args.config;
  if (typeof v !== "object" || v === null) throw new Error("替身后端：命令参数 config 缺失");
}

export function getSyncStatus(): SyncStatus {
  return {
    device_id: DEVICE_ID,
    channel_configured: false,
    last_sync_at: null,
    parked_count: 0,
    library_encrypted: false, // 明文库常量（与加密域明文态同源）
  };
}

export function syncNow(_args: Record<string, unknown>): SyncRoundReport {
  // 零计数轮次：未配置通道无任何上传/拉取/重放；明文库上通道 → 明文标记
  return {
    uploaded_segments: 0,
    uploaded_ops: 0,
    downloaded_segments: 0,
    applied: 0,
    deduped: 0,
    superseded: 0,
    skipped: 0,
    parked: 0,
    plaintext_mode: true,
  };
}

export function getParkedOps(): ParkedOpInfo[] {
  return [];
}

export function getSyncChannelConfig(): SyncChannelConfig {
  return { ...UNCONFIGURED_CHANNEL };
}

export function setSyncChannelConfig(args: Record<string, unknown>): void {
  reqConfig(args); // 收表单即成功，不落内存（常量态：回显恒空表单）
}

export function testSyncChannelConnection(args: Record<string, unknown>): void {
  reqConfig(args); // 探针恒成功（不模拟对象读取）
}

export function getSyncChannelCheckpoint(): SyncCheckpointInfo | null {
  return null; // 未配置通道无检查点，引导向导走「不可引导」支路
}

export function publishSyncCheckpoint(_args: Record<string, unknown>): SyncCheckpointPublished {
  return { generation: 1, size: 0, plaintext_mode: true }; // 占位发布回执
}

export function bootstrapSyncFromChannel(_args: Record<string, unknown>): SyncBootstrapOutcome {
  return { generation: 1, size: 0, reencrypted: false }; // 占位引导回执（明文快照）
}
