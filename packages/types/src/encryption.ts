export interface EncryptionStatus {
  locked: boolean;
  file_encrypted: boolean;
}

export interface UnlockOutcome {
  relocated: boolean;
}

export type RememberPassphraseMode = "biometry" | "dev-fallback";

export interface RememberPassphraseSupport {
  supported: boolean;
  mode: RememberPassphraseMode;
}
