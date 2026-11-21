<script setup lang="ts">
import {
  NButton,
  NCard,
  NCheckbox,
  NInput,
  NSpace,
  NSpin,
  NText,
  useMessage,
  useThemeVars,
} from "naive-ui";
import { onMounted, ref } from "vue";
import RestoreConfirmModal from "@/backup/RestoreConfirmModal.vue";
import { t } from "@ledger/i18n";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import { useRestoreFromFile } from "@/backup/useRestoreFromFile";
import { useAppStore } from "@/stores/app";
import { errorMessage } from "@ledger/utils/errors";
import { restartAppShortly } from "@/backup/restart";
import AppDangerConfirmModal from "@ledger/ui-kit/AppDangerConfirmModal.vue";

const {
  unlock,
  reset,
  rememberSupport,
  unlockWithRemembered,
  loadRememberSupport,
  syncRememberCache,
} = useEncryptionGate();
const store = useAppStore();
const message = useMessage();

const themeVars = useThemeVars();

const passphrase = ref("");
const submitting = ref(false);
const resetting = ref(false);
const errorText = ref("");

const { restoreIntent, restoreSeq, closeRestore, confirmRestore, pickRestore } = useRestoreFromFile(
  {
    pickTitleKey: "unlock.restorePickTitle",
    defaultPath: () => store.backupDir || undefined,
    contextPassphrase: () => passphrase.value,
  },
);

const autoUnlocking = ref(store.rememberPassphrase);
const autoUnlockFallback = ref("");
const rememberChecked = ref(store.rememberPassphrase);

const resetConfirmShow = ref(false);

onMounted(async () => {
  await loadRememberSupport();
  if (!autoUnlocking.value) return;
  if (!rememberSupport.value?.supported) {
    autoUnlocking.value = false;
    return;
  }
  try {
    const wait = await unlockWithRemembered();
    if (wait.status === "timeout") {
      autoUnlocking.value = false;
      autoUnlockFallback.value = t("unlock.autoUnlockTimeout");
      return;
    }
    if (wait.relocated) {
      message.success(t("unlock.relocated"));
      restartAppShortly();
    }
  } catch (e) {
    autoUnlocking.value = false;
    autoUnlockFallback.value = errorMessage(e);
  }
});

async function submit() {
  if (submitting.value) return;
  errorText.value = "";
  submitting.value = true;
  try {
    const relocated = await unlock(passphrase.value);
    const cached = await syncRememberCache(passphrase.value, rememberChecked.value);
    if (!cached) message.warning(t("unlock.rememberFailed"));
    if (relocated) {
      message.success(t("unlock.relocated"));
      restartAppShortly();
    }
    passphrase.value = "";
  } catch (e) {
    errorText.value = errorMessage(e);
  } finally {
    submitting.value = false;
  }
}

function forgotPassphrase() {
  if (resetting.value) return;
  errorText.value = "";
  resetConfirmShow.value = true;
}

async function confirmReset() {
  resetConfirmShow.value = false;
  if (resetting.value) return;
  resetting.value = true;
  try {
    await reset();
    store.setRememberPassphrase(false);
    message.success(t("unlock.resetOk"));
  } catch (e) {
    errorText.value = errorMessage(e);
  } finally {
    resetting.value = false;
  }
}
</script>

<template>
  <div class="unlock-screen" :style="{ backgroundColor: themeVars.bodyColor }">
    <!-- 自动解锁加载态（issue #574）：「记住」开启且正在尝试，认证通过即进入应用。
         等待期不渲染解锁卡片，只渲染居中小提示——空悬的大卡片盖在白底上是
         启动等待期「白屏 + 孤立小框」观感的来源；回退手输后再出卡片。 -->
    <NSpin v-if="autoUnlocking" size="small" :description="t('unlock.autoUnlocking')" />

    <NCard v-else class="unlock-card" :bordered="false">
      <NSpace vertical :size="16" align="center" :style="{ width: '100%' }">
        <NText class="unlock-title">{{ t("unlock.title") }}</NText>
        <NText depth="3">{{ t("unlock.hint") }}</NText>
        <NInput
          v-model:value="passphrase"
          type="password"
          show-password-on="click"
          :placeholder="t('unlock.placeholder')"
          :disabled="submitting"
          autofocus
          @keyup.enter="submit"
        />
        <NText v-if="errorText" type="error" class="unlock-error">{{ errorText }}</NText>
        <!-- 自动解锁回退提示（无缓存 / 生物认证取消）：告知用户回退手输 -->
        <NText v-if="autoUnlockFallback" type="warning" class="unlock-error">
          {{ autoUnlockFallback }}
        </NText>
        <NButton type="primary" block :loading="submitting" :disabled="!passphrase" @click="submit">
          {{ t("unlock.button") }}
        </NButton>
        <!-- 本机记住主口令（issue #574）：平台不支持（v1 非 macOS）时隐藏该选项 -->
        <NCheckbox
          v-if="rememberSupport?.supported"
          v-model:checked="rememberChecked"
          :disabled="submitting"
        >
          <NText depth="3">{{ t("unlock.remember") }}</NText>
        </NCheckbox>
        <!-- 提示按运行形态区分（issue #687）：dev 回退形态不宣称 Touch ID。 -->
        <NText v-if="rememberSupport?.supported" depth="3" class="unlock-remember-hint">
          {{
            rememberSupport?.mode === "dev-fallback"
              ? t("unlock.rememberDevFallbackHint")
              : t("unlock.rememberHint")
          }}
        </NText>
        <!-- 逃生门双入口（issue #573 / #603）：忘记口令重置与从备份文件恢复并列常驻 -->
        <NSpace :size="4" justify="center">
          <NButton
            quaternary
            size="small"
            data-testid="unlock-restore-open"
            :disabled="resetting"
            @click="pickRestore"
          >
            {{ t("unlock.restore") }}
          </NButton>
          <NButton quaternary size="small" :disabled="resetting" @click="forgotPassphrase">
            {{ t("unlock.forgot") }}
          </NButton>
        </NSpace>
      </NSpace>
    </NCard>

    <!-- 备份恢复确认弹窗（issue #603）：跨模式警告 + 密文备份口令，语义面与
         设置页/失败恢复屏同一 RestoreConfirmModal（上下文口令自动试开） -->
    <RestoreConfirmModal
      :intent="restoreIntent"
      :seq="restoreSeq"
      :on-confirm="confirmRestore"
      @close="closeRestore"
    />

    <!-- 忘记口令重置确认（issue #652 / ADR-0078）：error 级——不可逆（清空数据），
         承载无后门后果说明与密文副本保留兜底；确认后重置流程不变 -->
    <AppDangerConfirmModal
      level="error"
      v-model:show="resetConfirmShow"
      :title="t('unlock.resetTitle')"
      :strong-warning="t('unlock.resetStrong')"
      :detail="t('unlock.resetDetail')"
      :confirm-text="t('unlock.resetConfirm')"
      :cancel-text="t('unlock.resetCancel')"
      :submitting="resetting"
      :on-confirm="confirmReset"
      :on-cancel="() => (resetConfirmShow = false)"
    />
  </div>
</template>

<style scoped>
.unlock-screen {
  position: fixed;
  inset: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.unlock-card {
  width: 360px;
}

.unlock-title {
  font-size: 18px;
  font-weight: 600;
}

.unlock-error {
  align-self: flex-start;
}

.unlock-remember-hint {
  align-self: flex-start;
  font-size: 12px;
  line-height: 1.6;
}
</style>
