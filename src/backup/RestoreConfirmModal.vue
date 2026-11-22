<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { NAlert, NButton, NFormItem, NInput, NSpace, NText } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import { t } from "@ledger/i18n";
import { errorMessage, errorCodeOf } from "@ledger/utils/errors";
import {
  BACKUP_PASSPHRASE_REQUIRED,
  restoreCrossModeWarningKey,
  type RestoreIntent,
} from "@/backup/useBackup";

const props = defineProps<{
  intent: RestoreIntent | null;
  seq: number;
  onConfirm: (passphrase: string) => Promise<void>;
}>();

const emit = defineEmits<{ close: [] }>();

const passphrase = ref("");
const submitting = ref(false);
const error = ref<string | null>(null);
const passphraseRevealedByError = ref(false);

watch(
  () => props.seq,
  () => {
    passphrase.value = "";
    error.value = null;
    passphraseRevealedByError.value = false;
  },
);

const warningText = computed(() => {
  if (!props.intent) return "";
  const key = restoreCrossModeWarningKey(
    props.intent.backupEncrypted,
    props.intent.currentEncrypted,
  );
  return key ? t(key) : "";
});

const contextPassphrase = computed(() => props.intent?.contextPassphrase ?? "");
const hasContextPassphrase = computed(() => contextPassphrase.value.length > 0);

const needsPassphrase = computed(() => props.intent?.backupEncrypted ?? false);
const showPassphrase = computed(
  () => (needsPassphrase.value && !hasContextPassphrase.value) || passphraseRevealedByError.value,
);
const canSubmit = computed(
  () => !submitting.value && (!showPassphrase.value || passphrase.value.length > 0),
);

async function confirm() {
  if (!canSubmit.value || !props.intent) return;
  submitting.value = true;
  error.value = null;
  const attempt = showPassphrase.value ? passphrase.value : contextPassphrase.value;
  try {
    await props.onConfirm(attempt);
  } catch (e) {
    error.value = errorMessage(e);
    if (needsPassphrase.value || errorCodeOf(e) === BACKUP_PASSPHRASE_REQUIRED) {
      passphraseRevealedByError.value = true;
    }
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <AppModal
    :show="intent !== null"
    preset="card"
    card-size="md"
    :title="t('settings.data.msg.restoreConfirmTitle')"
    @update:show="
      (v: boolean) => {
        if (!v) emit('close');
      }
    "
  >
    <NSpace vertical :size="12">
      <NText depth="3" style="white-space: pre-line">
        {{ t("settings.data.msg.restoreConfirm") }}
      </NText>

      <NAlert
        v-if="warningText"
        type="warning"
        :show-icon="true"
        :title="t('settings.data.msg.restoreCrossModeWarnTitle')"
      >
        {{ warningText }}
      </NAlert>

      <NFormItem v-if="showPassphrase" :label="t('settings.data.msg.restorePassphraseLabel')">
        <NInput
          v-model:value="passphrase"
          type="password"
          show-password-on="click"
          :placeholder="t('settings.data.msg.restorePassphraseHint')"
          :disabled="submitting"
          data-testid="restore-passphrase"
          @keyup.enter="confirm"
        />
      </NFormItem>

      <NText v-if="error" type="error">{{ error }}</NText>

      <NSpace justify="end">
        <NButton :disabled="submitting" data-testid="restore-cancel" @click="emit('close')">
          {{ t("settings.data.msg.restoreCancel") }}
        </NButton>
        <NButton
          type="error"
          :loading="submitting"
          :disabled="!canSubmit"
          data-testid="restore-confirm"
          @click="confirm"
        >
          {{ t("settings.data.msg.restoreConfirmAction") }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
