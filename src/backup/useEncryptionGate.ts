import { ref } from "vue";
import { api } from "@ledger/api";
import { useAppStore } from "@/stores/app";
import type { RememberPassphraseSupport } from "@ledger/types";

const locked = ref<boolean | null>(null);

const bootFailed = ref(false);

const bootErrorCode = ref<string | null>(null);

export const AUTO_UNLOCK_TIMEOUT_MS = 30_000;

export type AutoUnlockWait = { status: "unlocked"; relocated: boolean } | { status: "timeout" };

const rememberSupport = ref<RememberPassphraseSupport | null>(null);

export function useEncryptionGate() {
  async function probe(): Promise<void> {
    try {
      const status = await api.getBootStatus();
      if (status.phase === "failed") {
        bootFailed.value = true;
        bootErrorCode.value = status.error_code;
      } else {
        bootErrorCode.value = null;
        locked.value = status.phase === "locked";
      }
    } catch (e) {
      console.warn("启动状态探测失败，按锁定处理（fail-closed）", e);
      locked.value = true;
    }
  }

  async function unlock(passphrase: string): Promise<boolean> {
    const outcome = await api.unlockEncryption(passphrase);
    locked.value = false;
    return outcome.relocated;
  }

  async function loadRememberSupport(): Promise<void> {
    if (rememberSupport.value) return;
    try {
      rememberSupport.value = await api.getRememberPassphraseSupport();
    } catch (e) {
      console.warn("读取本机记住主口令能力失败，按不支持处理", e);
      rememberSupport.value = { supported: false, mode: "biometry" };
    }
  }

  async function unlockWithRemembered(): Promise<AutoUnlockWait> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let expired = false;
    const core = api
      .unlockWithRememberedPassphrase()
      .then(async (outcome): Promise<AutoUnlockWait> => {
        if (expired) {
          const status = await api.getBootStatus();
          if (status.phase !== "ready") {
            return { status: "unlocked", relocated: outcome.relocated };
          }
        }
        locked.value = false;
        return { status: "unlocked", relocated: outcome.relocated };
      });
    core.catch(() => {});
    try {
      return await Promise.race([
        core,
        new Promise<AutoUnlockWait>((resolve) => {
          timer = setTimeout(() => {
            expired = true;
            resolve({ status: "timeout" });
          }, AUTO_UNLOCK_TIMEOUT_MS);
        }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  }

  async function clearRememberCache(): Promise<void> {
    useAppStore().setRememberPassphrase(false);
    try {
      await api.clearRememberPassphrase();
    } catch {}
  }

  async function syncRememberCache(passphrase: string, checked: boolean): Promise<boolean> {
    if (!checked) {
      await clearRememberCache();
      return true;
    }
    const store = useAppStore();
    store.setRememberPassphrase(true);
    try {
      await api.setRememberPassphrase(passphrase);
      return true;
    } catch {
      store.setRememberPassphrase(false);
      return false;
    }
  }

  async function reset(): Promise<void> {
    await api.resetAfterForgottenPassphrase();
    locked.value = false;
  }

  async function resetFromFailure(): Promise<void> {
    await api.resetAfterStartupFailure();
    bootFailed.value = false;
    bootErrorCode.value = null;
    locked.value = false;
  }

  return {
    locked,
    bootFailed,
    bootErrorCode,
    rememberSupport,
    probe,
    unlock,
    unlockWithRemembered,
    loadRememberSupport,
    syncRememberCache,
    clearRememberCache,
    reset,
    resetFromFailure,
  };
}
