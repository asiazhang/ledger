export interface DataLocationInfo {
  active_dir: string;
  configured_dir: string | null;
  pending_restart: boolean;
  fallback_reason: string | null;
}

export interface DataLocationChangeOutcome {
  requires_choice: boolean;
  committed: boolean;
  target_dir: string | null;
}
