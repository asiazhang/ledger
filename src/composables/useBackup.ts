import { computed, onMounted, onUnmounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/app";
import { api } from "@/api";
import type {
  AutoBackupState,
  BackupFileInfo,
  BackupKind,
} from "@/types";
import { errorMessage } from "@/utils/errors";
import { t } from "@/i18n";
import {
  defaultBackupFileName,
  isManagedBackupPath,
  normalizeBackupDir,
} from "@/utils/backup-name";

// 备份文件列表与滚动清理。命名规则与后端受管备份规则保持一致
// （前缀集合与受管判定收口在 `src/utils/backup-name.ts`，issue #127）。
// 后端在自动备份完成 / 备份清理成功后发出 `ledger:backups-changed`
// 无 payload 信号（issue #129，与 `ledger:changed` 平行），本模块订阅后
// 自动刷新备份列表与自动备份状态，无需手动刷新。
const BACKUPS_CHANGED_EVENT = "ledger:backups-changed";

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBackupTime(iso: string): string {
  return iso.slice(0, 16).replace("T", " ");
}

/** 来源展示文案：自动 / 手动；旧数据缺字段按手动（与后端回落一致）。 */
function sourceText(kind: BackupKind): string {
  return kind === "auto" ? t("settings.data.source.auto") : t("settings.data.source.manual");
}

export function useBackup() {
  const store = useAppStore();
  const message = useMessage();

  const backingUp = ref(false);
  const restoring = ref(false);
  const lastBackup = ref("");
  const backups = ref<BackupFileInfo[]>([]);
  const pruning = ref(false);

  // 自动备份设置（issue #128）：开关与上次自动备份时间存 ledger.db，经 IPC 读写。
  const autoBackupEnabled = ref(true);
  const autoBackupLastAt = ref<string | null>(null);

  /** 展示文案：格式化 UTC ISO 为 `YYYY-MM-DD HH:mm`，从未备份时显示「从未」。 */
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
    } catch (e: any) {
      // 状态读取失败不阻断手动备份功能：维持默认开关开启、无时间展示。
      message.error(t("settings.data.msg.autoStateFailed", { msg: errorMessage(e) }));
    }
  }

  async function toggleAutoBackup(enabled: boolean) {
    try {
      await api.setAutoBackupEnabled(enabled);
      autoBackupEnabled.value = enabled;
      message.success(
        enabled ? t("settings.data.msg.autoOn") : t("settings.data.msg.autoOff"),
      );
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

  /// 将备份目录中的受管备份修剪到 `keep` 个，并刷新列表。
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

  /// 上限变更：调小时立即清理到新值（输入框 blur/回车提交，不弹确认，仅提示）。
  function onBackupMaxCountChange(n: number | null) {
    if (n == null) return;
    const prev = store.backupMaxCount;
    store.setBackupMaxCount(n);
    if (store.backupDir && n < prev) {
      void pruneToLimit(n);
    }
  }

  /// 手动立即清理：超过上限时弹确认后执行。
  async function manualPrune() {
    if (!store.backupDir) return;
    const excess = Math.max(0, backups.value.length - store.backupMaxCount);
    if (excess === 0) {
      message.info(t("settings.data.msg.pruneNotNeeded"));
      return;
    }
    const ok = await confirm(t("settings.data.msg.pruneConfirm", { n: excess }), {
      title: t("settings.data.msg.pruneConfirmTitle"),
      kind: "warning",
    });
    if (!ok) return;
    pruning.value = true;
    try {
      await pruneToLimit(store.backupMaxCount);
    } finally {
      pruning.value = false;
    }
  }

  async function doBackup(target: string) {
    backingUp.value = true;
    try {
      const r = await api.createBackup(target);
      lastBackup.value = t("settings.data.msg.lastBackupPath", {
        path: r.path,
        size: `${(r.size_bytes / 1024).toFixed(1)} KB`,
      });
      message.success(t("settings.data.msg.backupOk"));
      if (isManagedBackupPath(target, store.backupDir)) {
        // 受管备份写入后立即滚动清理（一键备份/另存为同规则）。
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
      await doBackup(`${dir}${sep}${defaultBackupFileName()}`);
    } else {
      await backupAs();
    }
  }

  async function backupAs() {
    const path = await save({
      title: t("settings.data.msg.saveAsTitle"),
      defaultPath: store.backupDir
        ? `${store.backupDir}/${defaultBackupFileName()}`
        : defaultBackupFileName(),
      filters: [{ name: t("settings.data.msg.filterName"), extensions: ["zip"] }],
    });
    if (typeof path === "string" && path) await doBackup(path);
  }

  async function pickRestore() {
    const path = await open({
      title: t("settings.data.backup.restoreButton"),
      directory: false,
      multiple: false,
      defaultPath: store.backupDir || undefined,
      filters: [{ name: t("settings.data.msg.filterName"), extensions: ["zip", "db"] }],
    });
    if (typeof path !== "string" || !path) return;
    const ok = await confirm(t("settings.data.msg.restoreConfirm"), {
      title: t("settings.data.msg.restoreConfirmTitle"),
      kind: "warning",
    });
    if (!ok) return;
    restoring.value = true;
    try {
      const r = await api.restoreBackup(path);
      message.success(
        t("settings.data.msg.restoreOk", { version: r.schema_version }),
      );
      setTimeout(() => {
        api.restartApp();
      }, 800);
    } catch (e: any) {
      message.error(t("settings.data.msg.restoreFailed", { msg: errorMessage(e) }));
    } finally {
      restoring.value = false;
    }
  }

  let unlistenBackupsChanged: UnlistenFn | null = null;

  onMounted(async () => {
    void refreshBackups();
    void refreshAutoBackupState();
    // 订阅备份产物变更信号（issue #129）：自动备份完成 / 清理后列表自动刷新。
    try {
      unlistenBackupsChanged = await listen(BACKUPS_CHANGED_EVENT, () => {
        void refreshBackups();
        void refreshAutoBackupState();
      });
    } catch (e) {
      // 订阅失败不影响手动操作路径：列表仍可经按钮刷新。
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
    lastBackup,
    backups,
    pruning,
    backupRows,
    pickBackupDir,
    clearBackupDir,
    onBackupMaxCountChange,
    manualPrune,
    backupOnce,
    backupAs,
    pickRestore,
    autoBackupEnabled,
    autoBackupLastText,
    toggleAutoBackup,
  };
}
