export interface SyncStatus {
  device_id: string;
  channel_configured: boolean;
  last_sync_at: string | null;
  parked_count: number;
  library_encrypted: boolean;
}

export interface SyncChannelConfig {
  space_id: string;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  access_key: string;
  secret_key: string;
  path_style: boolean;
  configured: boolean;
}

export interface SyncChannelConfigInput {
  space_id?: string;
  endpoint?: string;
  region?: string;
  bucket?: string;
  prefix?: string;
  access_key?: string;
  secret_key?: string;
  path_style?: boolean;
}

export interface SyncRoundReport {
  uploaded_segments: number;
  uploaded_ops: number;
  downloaded_segments: number;
  applied: number;
  deduped: number;
  superseded: number;
  skipped: number;
  parked: number;
  plaintext_mode: boolean;
}

export interface ParkedOpInfo {
  op_id: string;
  device_id: string;
  entity: string;
  entity_id: string;
  code: string;
  params: string[];
  message: string;
  parked_at: string;
}

export interface SyncCheckpointInfo {
  generation: number;
  size: number;
  created_at: string;
}

export interface SyncCheckpointPublished {
  generation: number;
  size: number;
  plaintext_mode: boolean;
}

export interface SyncBootstrapOutcome {
  generation: number;
  size: number;
  reencrypted: boolean;
}
