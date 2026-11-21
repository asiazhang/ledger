import { computed, onMounted, onUnmounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/app";
import { formatIsoMinute } from "@ledger/utils/datetime";
import { api } from "@ledger/api";
import type { AutoBackupState, BackupFileInfo, BackupKind } from "@ledger/types";
import { errorMessage } from "@ledger/utils/errors";
import { t } from "@ledger/i18n";
import { useRestoreFromFile } from "@/backup/useRestoreFromFile";
import {
  defaultBackupFileName,
  isManagedBackupPath,
  normalizeBackupDir,
} from "@ledger/utils/backup-name";

const BACKUPS_CHANGED_EVENT = "ledger:backups-changed";

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBackupTime(iso: string): string {
  return formatIsoMinute(iso);
}

function sourceText(kind: BackupKind): string {
  return kind === "auto" ? t("settings.data.source.auto") : t("settings.data.source.manual");
}

function sourceDeviceText(row: Pick<BackupFileInfo, "source_device_id">): string {
  return row.source_device_id ?? t("settings.data.source.deviceUnknown");
}

export interface RestoreIntent {
  path: string;
  backupEncrypted: boolean;
  currentEncrypted: boolean;
  contextPassphrase?: string;
}

export const BACKUP_PASSPHRASE_REQUIRED = "backup.passphrase-required";

export function restoreCrossModeWarningKey(
  backupEncrypted: boolean,
  currentEncrypted: boolean,
): string | null {
  if (backupEncrypted === currentEncrypted) return null;
  return backupEncrypted
    ? "settings.data.msg.restoreToEncryptedWarn"
    : "settings.data.msg.restoreToPlaintextWarn";
}

export function useBackup() {
  const store = useAppStore();
  const message = useMessage();

  const backingUp = ref(false);
  const lastBackup = ref("");
  const lastBackupPath = ref("");
  const backups = ref<BackupFileInfo[]>([]);
  const pruning = ref(false);

  const pruneConfirmShow = ref(false);
  const pruneExcess = ref(0);

  const restoreFlow = useRestoreFromFile({
    pickTitleKey: "settings.data.backup.restoreButton",
    defaultPath: () => store.backupDir || undefined,
  });
  const restoring = restoreFlow.restoring;

  const autoBackupEnabled = ref(true);
  const autoBackupLastAt = ref<string | null>(null);
  const autoBackupFailing = ref(false);
  const autoBackupFailures = ref(0);

  const bookFingerprint = ref<string | null>(null);

  async function refreshBookFingerprint() {
    try {
      bookFingerprint.value = await api.getBookFingerprint();
    } catch {
      bookFingerprint.value = null;
    }
  }

  const deviceId = ref<string | null>(null);

  async function refreshDeviceId() {
    try {
      deviceId.value = await api.getDeviceId();
    } catch {
      deviceId.value = null;
    }
  }

  const autoBackupLastText = computed(() =>
    autoBackupLastAt.value
      ? formatBackupTime(autoBackupLastAt.value)
      : t("settings.data.backup.autoLastNever"),
  );

  async function refreshAutoBackupState() {
    try {
      const s: AutoBackupState = await api.getAutoBackupState();
      autoBackupEnabled.value = s.enabled;
      autoBackupLastAt.value = s.last_backup_at;
      autoBackupFailing.value = s.failure_alerting;
      autoBackupFailures.value = s.consecutive_failures;
    } catch (e: any) {
      message.error(t("settings.data.msg.autoStateFailed", { msg: errorMessage(e) }));
    }
  }

  async function toggleAutoBackup(enabled: boolean) {
    try {
      await api.setAutoBackupEnabled(enabled);
      autoBackupEnabled.value = enabled;
      message.success(enabled ? t("settings.data.msg.autoOn") : t("settings.data.msg.autoOff"));
    } catch (e: any) {
      message.error(t("settings.data.msg.autoToggleFailed", { msg: errorMessage(e) }));
    }
  }

  const backupRows = computed(() =>
    backups.value.map((b) => ({
      ...b,
      size_text: formatSize(b.size_bytes),
      created_at: formatBackupTime(b.created_at),
      source_text: sourceText(b.kind),
      source_device_text: sourceDeviceText(b),
    })),
  );

  async function pickBackupDir() {
    const dir = await open({
      directory: true,
      multiple: false,
      title: t("settings.data.msg.dirPickTitle"),
    });
    if (typeof dir === "string" && dir) {
      store.setBackupDir(dir);
      message.success(t("settings.data.msg.dirSet"));
      await refreshBackups();
    }
  }

  function clearBackupDir() {
    store.setBackupDir("");
    backups.value = [];
  }

  async function refreshBackups() {
    if (!store.backupDir) {
      backups.value = [];
      return;
    }
    try {
      backups.value = await api.listBackups(store.backupDir);
    } catch (e: any) {
      backups.value = [];
      message.error(t("settings.data.msg.listFailed", { msg: errorMessage(e) }));
    }
  }

  const refreshing = ref(false);
  async function refreshList() {
    refreshing.value = true;
    try {
      await refreshBackups();
    } finally {
      refreshing.value = false;
    }
  }

  async function pruneToLimit(keep: number) {
    if (!store.backupDir) return;
    try {
      const r = await api.pruneBackups(store.backupDir, keep);
      await refreshBackups();
      if (r.failed.length > 0) {
        message.warning(
          t("settings.data.msg.prunePartial", {
            deleted: r.deleted.length,
            failed: r.failed.length,
          }),
        );
      } else if (r.deleted.length > 0) {
        message.success(t("settings.data.msg.pruneDone", { n: r.deleted.length }));
      }
    } catch (e: any) {
      message.error(t("settings.data.msg.pruneFailed", { msg: errorMessage(e) }));
    }
  }

  function onBackupMaxCountChange(n: number | null) {
    if (n == null) return;
    const prev = store.backupMaxCount;
    store.setBackupMaxCount(n);
    if (store.backupDir && n < prev) {
      void pruneToLimit(n);
    }
  }

  function manualPrune() {
    if (!store.backupDir) return;
    const ownBucket = deviceId.value
      ? backups.value.filter((b) => b.source_device_id === deviceId.value)
      : [];
    const excess = Math.max(0, ownBucket.length - store.backupMaxCount);
    if (excess === 0) {
      message.info(t("settings.data.msg.pruneNotNeeded"));
      return;
    }
    pruneExcess.value = excess;
    pruneConfirmShow.value = true;
  }

  async function confirmPrune() {
    pruneConfirmShow.value = false;
    pruning.value = true;
    try {
      await pruneToLimit(store.backupMaxCount);
    } finally {
      pruning.value = false;
    }
  }

  function cancelPrune() {
    pruneConfirmShow.value = false;
  }

  async function doBackup(target: string) {
    backingUp.value = true;
    try {
      const r = await api.createBackup(target);
      lastBackup.value = t("settings.data.msg.lastBackupPath", {
        path: r.path,
        size: `${(r.size_bytes / 1024).toFixed(1)} KB`,
      });
      lastBackupPath.value = r.path;
      message.success(t("settings.data.msg.backupOk"));
      if (isManagedBackupPath(target, store.backupDir)) {
        await pruneToLimit(store.backupMaxCount);
      } else {
        await refreshBackups();
      }
    } catch (e: any) {
      message.error(t("settings.data.msg.backupFailed", { msg: errorMessage(e) }));
    } finally {
      backingUp.value = false;
    }
  }

  async function backupOnce() {
    if (store.backupDir) {
      const { dir, sep } = normalizeBackupDir(store.backupDir);
      await doBackup(`${dir}${sep}${defaultBackupFileName(new Date(), bookFingerprint.value)}`);
    } else {
      await backupAs();
    }
  }

  async function backupAs() {
    const defaultName = defaultBackupFileName(new Date(), bookFingerprint.value);
    const path = await save({
      title: t("settings.data.msg.saveAsTitle"),
      defaultPath: store.backupDir ? `${store.backupDir}/${defaultName}` : defaultName,
      filters: [{ name: t("settings.data.msg.filterName"), extensions: ["zip"] }],
    });
    if (typeof path === "string" && path) await doBackup(path);
  }

  async function pickRestore() {
    await restoreFlow.pickRestore();
  }

  async function revealInFinder(path: string) {
    try {
      await api.revealInFileManager(path);
    } catch (e: any) {
      message.error(errorMessage(e));
    }
  }

  async function copyLastBackupPath() {
    try {
      await navigator.clipboard.writeText(lastBackupPath.value);
      message.success(t("settings.data.backup.copyPathOk"));
    } catch (e: any) {
      message.error(t("settings.data.backup.copyPathFailed", { msg: errorMessage(e) }));
    }
  }

  async function confirmRestore(passphrase: string) {
    await restoreFlow.confirmRestore(passphrase);
  }

  let unlistenBackupsChanged: UnlistenFn | null = null;

  onMounted(async () => {
    void refreshBackups();
    void refreshAutoBackupState();
    void refreshBookFingerprint();
    void refreshDeviceId();
    try {
      unlistenBackupsChanged = await listen(BACKUPS_CHANGED_EVENT, () => {
        void refreshBackups();
        void refreshAutoBackupState();
      });
    } catch (e) {
      console.warn("订阅 ledger:backups-changed 失败", e);
    }
  });

  onUnmounted(() => {
    unlistenBackupsChanged?.();
    unlistenBackupsChanged = null;
  });

  return {
    backingUp,
    restoring,
    bookFingerprint,
    lastBackup,
    backups,
    pruning,
    backupRows,
    restoreIntent: restoreFlow.restoreIntent,
    restoreSeq: restoreFlow.restoreSeq,
    closeRestore: restoreFlow.closeRestore,
    confirmRestore,
    pickBackupDir,
    clearBackupDir,
    onBackupMaxCountChange,
    manualPrune,
    pruneConfirmShow,
    pruneExcess,
    confirmPrune,
    cancelPrune,
    backupOnce,
    backupAs,
    pickRestore,
    revealInFinder,
    copyLastBackupPath,
    autoBackupEnabled,
    autoBackupLastText,
    autoBackupFailing,
    autoBackupFailures,
    toggleAutoBackup,
    refreshing,
    refreshList,
  };
}
