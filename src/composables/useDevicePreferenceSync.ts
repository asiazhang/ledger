import { watch } from "vue";
import { useAppStore } from "@/stores/app";
import { api } from "@ledger/api";

export function useDevicePreferenceSync() {
  const store = useAppStore();

  watch(
    () => store.backupDir,
    (dir) => {
      void api.setAutoBackupDir(dir);
    },
    { immediate: true },
  );

  watch(
    () => store.autoExecutionEnabled,
    (enabled) => {
      void api.setAutoExecutionEnabled(enabled);
    },
    { immediate: true },
  );
  watch(
    () => store.autoUpdateEnabled,
    (enabled) => {
      void api.setAutoUpdateEnabled(enabled);
    },
    { immediate: true },
  );
}
