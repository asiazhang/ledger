import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { judgeMinLengthText } from "@ledger/utils/field-error";
import {
  assessPassphraseStrength,
  type PassphraseStrengthAssessment,
} from "@ledger/utils/passphrase-strength";
import { useLoadable } from "@ledger/loadable";
import { restartAppShortly } from "@/backup/restart";
import { useAppStore } from "@/stores/app";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import type { EncryptionStatus } from "@ledger/types";

export const PASSPHRASE_MIN_LENGTH = 8;

function trackPassphraseStrength(source: Ref<string>) {
  const assessment = ref<PassphraseStrengthAssessment | null>(null);
  let latest = 0;
  watch(source, (value) => {
    const seq = ++latest;
    void assessPassphraseStrength(value).then((result) => {
      if (seq === latest) assessment.value = result;
    });
  });
  return assessment;
}

export function useEncryptionTransitions() {
  const message = useMessage();
  const store = useAppStore();
  const { rememberSupport, loadRememberSupport, syncRememberCache, clearRememberCache } =
    useEncryptionGate();

  const status = ref<EncryptionStatus | null>(null);
  const {
    loading: statusLoading,
    error: statusError,
    run: runStatusLoad,
  } = useLoadable(() => api.getEncryptionStatus());

  async function refresh(): Promise<void> {
    const next = await runStatusLoad();
    if (next !== null) status.value = next;
  }

  const passphrase = ref("");
  const confirmPassphrase = ref("");
  const enableRemember = ref(false);
  const passphraseStrength = trackPassphraseStrength(passphrase);

  const passphraseTooShort = computed(
    () => judgeMinLengthText(passphrase.value, PASSPHRASE_MIN_LENGTH).kind === "too-short",
  );

  const mismatch = computed(
    () => confirmPassphrase.value.length > 0 && confirmPassphrase.value !== passphrase.value,
  );

  const enableConfirmShow = ref(false);

  const { loading: submitting, run: runEnable } = useLoadable(async () => {
    await api.enableEncryption(passphrase.value);
    const cached = await syncRememberCache(passphrase.value, enableRemember.value);
    if (!cached) message.warning(t("settings.data.encryption.rememberFailed"));
    passphrase.value = "";
    confirmPassphrase.value = "";
    message.success(t("settings.data.encryption.okToast"));
    restartAppShortly();
  });

  function requestEnable() {
    if (submitting.value || mismatch.value || !passphrase.value || passphraseTooShort.value) return;
    enableConfirmShow.value = true;
  }

  async function confirmEnable() {
    enableConfirmShow.value = false;
    if (submitting.value || mismatch.value || !passphrase.value) return;
    await runEnable();
  }

  const changeOld = ref("");
  const changeNew = ref("");
  const changeConfirm = ref("");
  const changeRemember = ref(store.rememberPassphrase);
  const changeNewStrength = trackPassphraseStrength(changeNew);
  const changeMismatch = computed(
    () => changeConfirm.value.length > 0 && changeConfirm.value !== changeNew.value,
  );
  const changeUnchanged = computed(
    () => changeNew.value.length > 0 && changeNew.value === changeOld.value,
  );
  const changeNewTooShort = computed(
    () => judgeMinLengthText(changeNew.value, PASSPHRASE_MIN_LENGTH).kind === "too-short",
  );
  const changeReady = computed(
    () =>
      changeOld.value.length > 0 &&
      changeNew.value.length > 0 &&
      !changeMismatch.value &&
      !changeUnchanged.value &&
      !changeNewTooShort.value,
  );

  const changeConfirmShow = ref(false);

  const { loading: submittingChange, run: runChange } = useLoadable(async () => {
    await api.changeEncryptionPassphrase(changeOld.value, changeNew.value);
    const cached = await syncRememberCache(changeNew.value, changeRemember.value);
    if (!cached) message.warning(t("settings.data.encryption.rememberFailed"));
    changeOld.value = "";
    changeNew.value = "";
    changeConfirm.value = "";
    message.success(t("settings.data.encryption.changeOkToast"));
    restartAppShortly();
  });

  function requestChange() {
    if (submittingChange.value || !changeReady.value) return;
    changeConfirmShow.value = true;
  }

  async function confirmChange() {
    changeConfirmShow.value = false;
    if (submittingChange.value || !changeReady.value) return;
    await runChange();
  }

  const disablePassphrase = ref("");

  const disableConfirmShow = ref(false);

  const { loading: submittingDisable, run: runDisable } = useLoadable(async () => {
    await api.disableEncryption(disablePassphrase.value);
    await clearRememberCache();
    disablePassphrase.value = "";
    message.success(t("settings.data.encryption.disableOkToast"));
    restartAppShortly();
  });

  function requestDisable() {
    if (submittingDisable.value || !disablePassphrase.value) return;
    disableConfirmShow.value = true;
  }

  async function confirmDisable() {
    disableConfirmShow.value = false;
    if (submittingDisable.value || !disablePassphrase.value) return;
    await runDisable();
  }

  const autoUnlockOn = computed(() => store.rememberPassphrase);

  const autoUnlockModalShow = ref(false);
  const autoUnlockPass = ref("");
  const {
    loading: autoUnlockSubmitting,
    error: autoUnlockError,
    run: runAutoUnlock,
  } = useLoadable(
    async () => {
      await api.setRememberPassphrase(autoUnlockPass.value);
      store.setRememberPassphrase(true);
      autoUnlockModalShow.value = false;
      autoUnlockPass.value = "";
      message.success(t("settings.data.encryption.rememberEnabled"));
    },
    { silent: true },
  );

  function openAutoUnlockModal() {
    autoUnlockPass.value = "";
    autoUnlockError.value = null;
    autoUnlockModalShow.value = true;
  }

  async function confirmAutoUnlock() {
    if (autoUnlockSubmitting.value || !autoUnlockPass.value) return;
    await runAutoUnlock();
  }

  async function disableAutoUnlock() {
    await clearRememberCache();
    message.success(t("settings.data.encryption.rememberDisabledToast"));
  }

  onMounted(() => {
    void refresh();
    void loadRememberSupport();
  });

  return {
    status,
    statusLoading,
    statusError,
    refresh,
    passphrase,
    confirmPassphrase,
    enableRemember,
    passphraseStrength,
    passphraseTooShort,
    mismatch,
    enableConfirmShow,
    requestEnable,
    confirmEnable,
    submitting,
    changeOld,
    changeNew,
    changeConfirm,
    changeRemember,
    changeNewStrength,
    changeMismatch,
    changeUnchanged,
    changeNewTooShort,
    changeReady,
    changeConfirmShow,
    requestChange,
    confirmChange,
    submittingChange,
    disablePassphrase,
    disableConfirmShow,
    requestDisable,
    confirmDisable,
    submittingDisable,
    rememberSupport,
    autoUnlockOn,
    autoUnlockModalShow,
    autoUnlockPass,
    autoUnlockError,
    autoUnlockSubmitting,
    openAutoUnlockModal,
    confirmAutoUnlock,
    disableAutoUnlock,
  };
}
