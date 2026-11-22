import type { DataLocationChangeOutcome, DataLocationInfo } from "@ledger/types";
import { bool, str } from "./state";

const MOCK_DATA_DIR = "/mock/data";

export function getDataLocationInfo(): DataLocationInfo {
  return {
    active_dir: MOCK_DATA_DIR,
    configured_dir: null,
    pending_restart: false,
    fallback_reason: null,
  };
}

export function submitDataLocationChange(args: Record<string, unknown>): DataLocationChangeOutcome {
  const targetDir = str(args, "targetDir");
  bool(args, "adoptExisting");
  return { requires_choice: false, committed: true, target_dir: targetDir };
}

export function restoreDefaultDataLocation(
  args: Record<string, unknown>,
): DataLocationChangeOutcome {
  bool(args, "adoptExisting");
  return { requires_choice: false, committed: true, target_dir: MOCK_DATA_DIR };
}
