import type { EncryptionStatus, RememberPassphraseSupport, UnlockOutcome } from "@ledger/types";
import { str } from "./state";

const PLAINTEXT_UNLOCKED: EncryptionStatus = { locked: false, file_encrypted: false };

function codedError(
  code: string,
  message: string,
): { kind: "Invalid"; code: string; message: string } {
  return { kind: "Invalid", code, message };
}

export function getEncryptionStatus(): EncryptionStatus {
  return { ...PLAINTEXT_UNLOCKED };
}

export function unlockEncryption(args: Record<string, unknown>): UnlockOutcome {
  str(args, "passphrase");
  return { relocated: false };
}

export function enableEncryption(args: Record<string, unknown>): void {
  str(args, "passphrase");
}

export function disableEncryption(args: Record<string, unknown>): void {
  str(args, "passphrase");
}

export function changeEncryptionPassphrase(args: Record<string, unknown>): void {
  str(args, "passphrase");
  str(args, "newPassphrase");
}

export function resetAfterForgottenPassphrase(): void {}

export function resetAfterStartupFailure(): void {}

export function getRememberPassphraseSupport(): RememberPassphraseSupport {
  return { supported: false, mode: "biometry" };
}

export function setRememberPassphrase(args: Record<string, unknown>): void {
  str(args, "passphrase");
  throw codedError("encryption.remember-unsupported", "当前平台不支持本机记住主口令");
}

export function clearRememberPassphrase(): void {}

export function unlockWithRememberedPassphrase(): Promise<UnlockOutcome> {
  return Promise.reject(
    codedError("encryption.remember-no-cache", "本机没有缓存的主口令，请手动输入"),
  );
}
