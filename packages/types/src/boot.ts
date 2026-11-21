export type BootPhase = "ready" | "locked" | "failed";

export interface BootStatus {
  phase: BootPhase;
  error_code: string | null;
}
