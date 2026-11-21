import { defineStore } from "pinia";
import { ref } from "vue";
import { loadLocal, saveLocal } from "@ledger/storage";
import { getLocaleSetting, setLocaleSetting, type LocaleSetting } from "@ledger/i18n";
import { amountPrivacyEnabled, AMOUNT_PRIVACY_STORAGE_KEY } from "@ledger/money";
import type { Theme } from "@ledger/theme";

export const useAppStore = defineStore("app", () => {
  const theme = ref<Theme>(loadLocal<Theme>("appearance", "dark"));
  const defaultCurrency = ref<string>(loadLocal<string>("default_currency", "CNY"));
  const backupDir = ref<string>(loadLocal<string>("backup_dir", ""));
  const backupMaxCount = ref<number>(loadLocal<number>("backup_max_count", 30));
  const autoExecutionEnabled = ref<boolean>(loadLocal<boolean>("auto_execution_enabled", false));
  const autoUpdateEnabled = ref<boolean>(loadLocal<boolean>("auto_update_enabled", true));
  const updateRemindedVersion = ref<string | null>(
    loadLocal<string | null>("update_reminded_version", null),
  );
  const localeSetting = ref<LocaleSetting>(getLocaleSetting());

  const rememberPassphrase = ref<boolean>(loadLocal<boolean>("remember_passphrase", false));

  amountPrivacyEnabled.value = loadLocal<boolean>(AMOUNT_PRIVACY_STORAGE_KEY, false);

  function setTheme(t: Theme) {
    theme.value = t;
    saveLocal("appearance", t);
  }

  function setDefaultCurrency(code: string) {
    defaultCurrency.value = code;
    saveLocal("default_currency", code);
  }

  function setBackupDir(dir: string) {
    backupDir.value = dir;
    saveLocal("backup_dir", dir);
  }

  function setBackupMaxCount(n: number) {
    backupMaxCount.value = n;
    saveLocal("backup_max_count", n);
  }

  function setAutoExecutionEnabled(enabled: boolean) {
    autoExecutionEnabled.value = enabled;
    saveLocal("auto_execution_enabled", enabled);
  }

  function setAutoUpdateEnabled(enabled: boolean) {
    autoUpdateEnabled.value = enabled;
    saveLocal("auto_update_enabled", enabled);
  }

  function markUpdateReminded(version: string) {
    updateRemindedVersion.value = version;
    saveLocal("update_reminded_version", version);
  }

  async function setLocale(value: LocaleSetting) {
    localeSetting.value = value;
    await setLocaleSetting(value);
  }

  function setAmountPrivacyEnabled(enabled: boolean) {
    amountPrivacyEnabled.value = enabled;
    saveLocal(AMOUNT_PRIVACY_STORAGE_KEY, enabled);
  }

  function setRememberPassphrase(enabled: boolean) {
    rememberPassphrase.value = enabled;
    saveLocal("remember_passphrase", enabled);
  }

  return {
    theme,
    defaultCurrency,
    backupDir,
    backupMaxCount,
    autoExecutionEnabled,
    autoUpdateEnabled,
    updateRemindedVersion,
    localeSetting,
    amountPrivacyEnabled,
    rememberPassphrase,
    setTheme,
    setDefaultCurrency,
    setBackupDir,
    setBackupMaxCount,
    setAutoExecutionEnabled,
    setAutoUpdateEnabled,
    markUpdateReminded,
    setLocale,
    setAmountPrivacyEnabled,
    setRememberPassphrase,
  };
});
