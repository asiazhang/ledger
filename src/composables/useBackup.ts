import { computed, onMounted, onUnmounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/app";
import { api } from "@/api";
import type { AutoBackupState, BackupFileInfo, BackupKind } from "@/types";
import { errorMessage } from "@/utils/errors";
import { t } from "@/i18n";
import { useRestoreFromFile } from "@/composables/useRestoreFromFile";
import {
  defaultBackupFileName,
  isManagedBackupPath,
  normalizeBackupDir,
} from "@/utils/backup-name";

// 备份文件列表与滚动清理。命名规则与后端受管备份规则保持一致
// （前缀集合与受管判定收口在 `src/utils/backup-name.ts`，issue #127）。
// 后端在自动备份完成 / 备份清理成功后发出 `ledger:backups-changed`
// 无 payload 信号（issue #129，与 `ledger:changed` 平行），本模块订阅后
// 自动刷新备份列表与自动备份状态；列表卡头部另有手动刷新按钮（issue #651），
// 供用户在文件管理器手动增删文件后显式同步列表与磁盘。
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

/** 恢复确认弹窗意图（issue #572，ADR-0072 弹窗意图编排）：选中备份的目标载荷。 */
export interface RestoreIntent {
  /** 所选备份文件路径。 */
  path: string;
  /** 备份是否为密文（元数据标记或裸库探测，缺标记视为明文）。 */
  backupEncrypted: boolean;
  /** 当前库是否为密文（启动探测接管，文件即真相）。 */
  currentEncrypted: boolean;
  /** 宿主上下文口令（issue #602/#603）：存在时确认弹窗先自动试开，失败才显出
   *  口令框重输；明文损坏的失败恢复屏无上下文口令，不携带（直接弹口令框）。 */
  contextPassphrase?: string;
}

/** 后端码化错误（issue #572）：密文备份缺主口令——元数据谎报明文而实库为密文
 *  时，确认弹窗据它按需显出口令框（与后端 engine 单源码字面对应）。 */
export const BACKUP_PASSPHRASE_REQUIRED = 'backup.passphrase-required';

/**
 * 跨模式恢复警告文案 key（issue #572 / ADR-0075 决策 7）：当前模式与备份模式
 * 不一致时返回显著警告文案 key，一致返回 null（不警告）。
 */
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
  const backups = ref<BackupFileInfo[]>([]);
  const pruning = ref(false);

  // 手动清理确认弹窗（issue #652 / ADR-0078）：warning 级应用内弹窗替代原生
  // confirm——破坏性但有兜底（删的是可再生备份产物，账本数据不受影响）。
  // 待删数量在开启时锁定为 pruneExcess，供弹窗文案展示；取消路径零副作用。
  const pruneConfirmShow = ref(false);
  const pruneExcess = ref(0);

  // 恢复确认弹窗（issue #572，ADR-0072）：开启/目标/关闭编排、文件选择、
  // 元数据/模式校验、恢复确认与重启均收口在共享恢复流 useRestoreFromFile
  // （与失败恢复屏/解锁屏恢复入口零拷贝），本模块只提供设置页侧的
  // 文件选择器参数。
  const restoreFlow = useRestoreFromFile({
    pickTitleKey: "settings.data.backup.restoreButton",
    defaultPath: () => store.backupDir || undefined,
  });
  const restoring = restoreFlow.restoring;

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

  /// 手动刷新（issue #651）：列表卡头部按钮，重拉使列表与磁盘一致。
  const refreshing = ref(false);
  async function refreshList() {
    refreshing.value = true;
    try {
      await refreshBackups();
    } finally {
      refreshing.value = false;
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

  /// 手动立即清理：超过上限时弹 warning 级确认弹窗（ADR-0078），确认后执行。
  function manualPrune() {
    if (!store.backupDir) return;
    const excess = Math.max(0, backups.value.length - store.backupMaxCount);
    if (excess === 0) {
      message.info(t("settings.data.msg.pruneNotNeeded"));
      return;
    }
    pruneExcess.value = excess;
    pruneConfirmShow.value = true;
  }

  /// 清理确认：执行修剪（弹窗先关，与既有 pruning 加载态衔接）。
  async function confirmPrune() {
    pruneConfirmShow.value = false;
    pruning.value = true;
    try {
      await pruneToLimit(store.backupMaxCount);
    } finally {
      pruning.value = false;
    }
  }

  /// 清理取消：仅关弹窗，零删除副作用。
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
    await restoreFlow.pickRestore();
  }

  /** 恢复确认（弹窗提交）：密文备份附带主口令，明文备份不消费。 */
  async function confirmRestore(passphrase: string) {
    await restoreFlow.confirmRestore(passphrase);
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
      // 订阅失败不影响手动刷新路径：列表可经刷新按钮重拉（refreshList）。
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
    autoBackupEnabled,
    autoBackupLastText,
    toggleAutoBackup,
    refreshing,
    refreshList,
  };
}
