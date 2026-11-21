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
    library_encrypted: false,
  };
}

export function syncNow(_args: Record<string, unknown>): SyncRoundReport {
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
  reqConfig(args);
}

export function testSyncChannelConnection(args: Record<string, unknown>): void {
  reqConfig(args);
}

export function getSyncChannelCheckpoint(): SyncCheckpointInfo | null {
  return null;
}

export function publishSyncCheckpoint(_args: Record<string, unknown>): SyncCheckpointPublished {
  return { generation: 1, size: 0, plaintext_mode: true };
}

export function bootstrapSyncFromChannel(_args: Record<string, unknown>): SyncBootstrapOutcome {
  return { generation: 1, size: 0, reencrypted: false };
}
