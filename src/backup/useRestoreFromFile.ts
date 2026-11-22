import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import type { BackupMetaSummary } from "@ledger/types";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { restartAppShortly } from "@/backup/restart";
import { useModalIntent } from "@ledger/modal-intent";
import type { RestoreIntent } from "@/backup/useBackup";

export function useRestoreFromFile(options: {
  pickTitleKey: string;
  defaultPath?: () => string | undefined;
  contextPassphrase?: () => string;
}) {
  const message = useMessage();

  const restoreModal = useModalIntent<RestoreIntent>();
  const restoring = ref(false);

  async function pickRestore() {
    const path = await open({
      title: t(options.pickTitleKey),
      directory: false,
      multiple: false,
      defaultPath: options.defaultPath?.(),
      filters: [{ name: t("settings.data.msg.filterName"), extensions: ["zip", "db"] }],
    });
    if (typeof path !== "string" || !path) return;
    let meta: BackupMetaSummary;
    try {
      meta = await api.getBackupMeta(path);
    } catch (e) {
      message.error(t("settings.data.msg.backupMetaFailed", { msg: errorMessage(e) }));
      return;
    }
    let currentEncrypted: boolean;
    try {
      currentEncrypted = (await api.getEncryptionStatus()).file_encrypted;
    } catch (e) {
      message.error(t("settings.data.msg.encryptionStatusFailed", { msg: errorMessage(e) }));
      return;
    }
    const contextPassphrase = options.contextPassphrase?.() ?? "";
    restoreModal.open({
      path,
      backupEncrypted: meta.encrypted,
      currentEncrypted,
      ...(contextPassphrase.trim() ? { contextPassphrase } : {}),
    });
  }

  async function confirmRestore(passphrase: string) {
    const intent = restoreModal.intent.value;
    if (!intent) return;
    restoring.value = true;
    try {
      const r = await api.restoreBackup(
        intent.path,
        intent.backupEncrypted || passphrase ? passphrase : null,
      );
      restoreModal.close();
      message.success(t("settings.data.msg.restoreOk", { version: r.schema_version }));
      restartAppShortly();
    } finally {
      restoring.value = false;
    }
  }

  return {
    restoring,
    restoreIntent: restoreModal.intent,
    restoreSeq: restoreModal.seq,
    closeRestore: restoreModal.close,
    confirmRestore,
    pickRestore,
  };
}
