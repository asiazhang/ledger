<script setup lang="ts">
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { computed, ref } from "vue";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import RestoreConfirmModal from "@/backup/RestoreConfirmModal.vue";
import { t } from "@ledger/i18n";
import { RELEASE_PAGE_URL } from "@/composables/useAppUpdate";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import { useFailureRestore } from "@/backup/useFailureRestore";
import { errorMessage } from "@ledger/utils/errors";

const { resetFromFailure, bootErrorCode } = useEncryptionGate();
const { restoreIntent, restoreSeq, closeRestore, confirmRestore, pickRestoreFromFailure } =
  useFailureRestore();
const message = useMessage();

const confirmVisible = ref(false);
const submitting = ref(false);
const errorText = ref("");

const isSchemaDrift = computed(() => bootErrorCode.value === "boot.schema-drift");

const isSchemaAhead = computed(() => bootErrorCode.value === "boot.schema-ahead");

const channels = computed(() =>
  isSchemaAhead.value
    ? (["upgrade"] as const)
    : isSchemaDrift.value
      ? (["restore", "reset"] as const)
      : (["reset", "restore"] as const),
);

async function openReleasePage(): Promise<void> {
  try {
    await openUrl(RELEASE_PAGE_URL);
  } catch (e) {
    console.warn("打开发布页失败", e);
  }
}

function openConfirm() {
  errorText.value = "";
  confirmVisible.value = true;
}

function cancelConfirm() {
  if (submitting.value) return;
  confirmVisible.value = false;
}

async function confirmReset() {
  if (submitting.value) return;
  errorText.value = "";
  submitting.value = true;
  try {
    await resetFromFailure();
    confirmVisible.value = false;
    message.success(t("startupFailure.resetOk"));
  } catch (e) {
    errorText.value = errorMessage(e);
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div class="failure-screen">
    <NCard class="failure-card" :bordered="false">
      <NSpace vertical :size="16" align="center" :style="{ width: '100%' }">
        <!-- 标题/说明按失败码取材（issue #994）：漂移出「结构异常」、超前出
             「需升级」，其余出「打不开」 -->
        <NText class="failure-title">
          {{
            isSchemaAhead
              ? t("startupFailure.aheadTitle")
              : isSchemaDrift
                ? t("startupFailure.driftTitle")
                : t("startupFailure.title")
          }}
        </NText>
        <NText depth="3" class="failure-hint">
          {{
            isSchemaAhead
              ? t("startupFailure.aheadHint")
              : isSchemaDrift
                ? t("startupFailure.driftHint")
                : t("startupFailure.hint")
          }}
        </NText>

        <!-- 恢复通道按失败码排布（issue #994）：库不可读重置优先（重置入口
             error 实心），漂移从备份恢复优先（恢复入口 primary 实心）；超前场景
             仅升级通道（重置会毁掉完好的新库，不呈现） -->
        <template v-for="channel in channels" :key="channel">
          <!-- 升级通道（boot.schema-ahead）：唯一正确出路，替代重置/恢复 -->
          <NSpace v-if="channel === 'upgrade'" vertical :size="8" class="failure-channel">
            <NText strong>{{ t("startupFailure.upgradeChannelTitle") }}</NText>
            <NText depth="3">{{ t("startupFailure.upgradeChannelHint") }}</NText>
            <NButton
              type="primary"
              block
              data-testid="failure-upgrade-open"
              @click="openReleasePage"
            >
              {{ t("startupFailure.upgradeButton") }}
            </NButton>
          </NSpace>

          <!-- 重置为空库（issue #601）：重置是破坏性操作，error 级二次确认不变 -->
          <NSpace v-else-if="channel === 'reset'" vertical :size="8" class="failure-channel">
            <NText strong>{{ t("startupFailure.resetChannelTitle") }}</NText>
            <NText depth="3">{{ t("startupFailure.resetChannelHint") }}</NText>
            <NButton
              :type="isSchemaDrift ? 'default' : 'error'"
              block
              data-testid="failure-reset-open"
              @click="openConfirm"
            >
              {{ t("startupFailure.resetButton") }}
            </NButton>
          </NSpace>

          <!-- 从备份文件恢复（issue #602）：确认弹窗复用 #572 语义面 -->
          <NSpace v-else vertical :size="8" class="failure-channel">
            <NText strong>{{ t("startupFailure.restoreChannelTitle") }}</NText>
            <NText depth="3">{{ t("startupFailure.restoreChannelHint") }}</NText>
            <NButton
              :type="isSchemaDrift ? 'primary' : 'default'"
              block
              data-testid="failure-restore-open"
              @click="pickRestoreFromFailure"
            >
              {{ t("startupFailure.restoreButton") }}
            </NButton>
          </NSpace>
        </template>

        <NText v-if="errorText" type="error" class="failure-error">{{ errorText }}</NText>
      </NSpace>
    </NCard>

    <!-- 二次确认（ADR-0078 error 级）：红色警示块 + 加粗后果句 + 红色确认按钮 -->
    <AppModal
      :show="confirmVisible"
      preset="card"
      :title="t('startupFailure.resetTitle')"
      card-size="md"
      :bordered="false"
      :mask-closable="false"
      @update:show="cancelConfirm"
    >
      <NSpace vertical :size="12">
        <NAlert type="error" :show-icon="true" :title="t('startupFailure.resetWarnTitle')">
          <NText strong type="error">{{ t("startupFailure.resetConsequence") }}</NText>
        </NAlert>
        <NText depth="3">{{ t("startupFailure.resetBody") }}</NText>
        <NSpace justify="end">
          <NButton :disabled="submitting" data-testid="failure-reset-cancel" @click="cancelConfirm">
            {{ t("startupFailure.resetCancel") }}
          </NButton>
          <NButton
            type="error"
            :loading="submitting"
            :disabled="submitting"
            data-testid="failure-reset-confirm"
            @click="confirmReset"
          >
            {{ t("startupFailure.resetConfirm") }}
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>

    <!-- 备份恢复确认弹窗（issue #602）：跨模式警告 + 密文备份口令（#572 语义面） -->
    <RestoreConfirmModal
      :intent="restoreIntent"
      :seq="restoreSeq"
      :on-confirm="confirmRestore"
      @close="closeRestore"
    />
  </div>
</template>

<style scoped>
.failure-screen {
  position: fixed;
  inset: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.failure-card {
  width: 420px;
}

.failure-title {
  font-size: 18px;
  font-weight: 600;
}

.failure-hint,
.failure-error {
  align-self: flex-start;
}

.failure-channel {
  width: 100%;
  align-self: stretch;
}
</style>
