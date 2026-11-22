<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { NAlert, NButton, NCard, NSpace, NSpin, NText, useMessage } from "naive-ui";
import { onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import AppDangerConfirmModal from "@ledger/ui-kit/AppDangerConfirmModal.vue";
import type { DataLocationChangeOutcome, DataLocationInfo } from "@ledger/types";

const message = useMessage();

const info = ref<DataLocationInfo | null>(null);
const loading = ref(false);
const loadError = ref("");
const submitting = ref(false);

const adoptConfirmShow = ref(false);
let pendingAdopt: (() => Promise<DataLocationChangeOutcome>) | null = null;

async function refresh() {
  loading.value = true;
  loadError.value = "";
  try {
    info.value = await api.getDataLocationInfo();
  } catch (e: any) {
    loadError.value = t("settings.data.msg.loadFailed", { msg: errorMessage(e) });
    message.error(loadError.value);
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);

async function copyText(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    message.success(t("settings.data.location.copyPathOk"));
  } catch (e: any) {
    message.error(t("settings.data.location.copyPathFailed", { msg: errorMessage(e) }));
  }
}

function copyActivePath() {
  if (info.value?.active_dir) void copyText(info.value.active_dir);
}

function copyPendingPath() {
  if (info.value?.configured_dir) void copyText(info.value.configured_dir);
}

async function pickAndSubmit() {
  const dir = await open({
    directory: true,
    multiple: false,
    title: t("settings.data.location.pickTitle"),
  });
  if (typeof dir !== "string" || !dir) return;
  await submitWithChoice((adoptExisting) => api.submitDataLocationChange(dir, adoptExisting));
}

async function restoreDefault() {
  await submitWithChoice((adoptExisting) => api.restoreDefaultDataLocation(adoptExisting));
}

async function applyOutcome(outcome: DataLocationChangeOutcome) {
  if (outcome.committed) {
    message.success(t("settings.data.location.committed"));
  }
  await refresh();
}

async function submitWithChoice(
  submit: (adoptExisting: boolean) => Promise<DataLocationChangeOutcome>,
) {
  submitting.value = true;
  try {
    const outcome = await submit(false);
    if (outcome.requires_choice) {
      pendingAdopt = () => submit(true);
      adoptConfirmShow.value = true;
      return;
    }
    await applyOutcome(outcome);
  } catch (e: any) {
    message.error(t("settings.data.msg.changeFailed", { msg: errorMessage(e) }));
  } finally {
    submitting.value = false;
  }
}

async function confirmAdopt() {
  const submit = pendingAdopt;
  pendingAdopt = null;
  adoptConfirmShow.value = false;
  if (!submit) return;
  submitting.value = true;
  try {
    await applyOutcome(await submit());
  } catch (e: any) {
    message.error(t("settings.data.msg.changeFailed", { msg: errorMessage(e) }));
  } finally {
    submitting.value = false;
  }
}

function cancelAdopt() {
  pendingAdopt = null;
  adoptConfirmShow.value = false;
  message.info(t("settings.data.location.cancelled"));
}
</script>

<template>
  <NCard :title="t('settings.data.location.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t("settings.data.location.hint") }}
      </NText>

      <NAlert
        v-if="info?.fallback_reason"
        type="error"
        :show-icon="true"
        :title="t('settings.data.location.fallbackTitle')"
      >
        {{ t("settings.data.location.fallbackBody", { reason: info.fallback_reason }) }}
      </NAlert>

      <NAlert
        v-if="info?.pending_restart && info.configured_dir"
        type="info"
        :show-icon="true"
        :title="t('settings.data.location.pendingTitle')"
      >
        {{ t("settings.data.location.pendingBody", { dir: info.configured_dir }) }}
        <NButton
          size="tiny"
          style="margin-left: 8px"
          data-testid="copy-pending-path"
          @click="copyPendingPath"
        >
          {{ t("settings.data.location.copyPath") }}
        </NButton>
      </NAlert>

      <NSpin :show="loading">
        <NSpace v-if="loadError" align="center" :size="12">
          <NText type="error">{{ loadError }}</NText>
          <NButton size="small" @click="refresh">{{ t("settings.data.location.retry") }}</NButton>
        </NSpace>
        <NSpace v-else align="center" :size="12">
          <NText>{{ t("settings.data.location.activeLabel") }}</NText>
          <NText style="word-break: break-all">
            {{ info?.active_dir ?? t("settings.data.location.reading") }}
          </NText>
          <NButton
            v-if="info?.active_dir"
            size="small"
            data-testid="copy-active-path"
            @click="copyActivePath"
          >
            {{ t("settings.data.location.copyPath") }}
          </NButton>
        </NSpace>
      </NSpin>

      <NSpace align="center" :size="12">
        <NButton type="primary" :loading="submitting" @click="pickAndSubmit">{{
          t("settings.data.location.change")
        }}</NButton>
        <!-- 未配置任何意图目录即处于默认位置，「恢复默认」无意义故禁用
             （契约不变量：configured_dir 非空 ⇔ 存在自定义位置意图）。 -->
        <NButton :disabled="submitting || !info?.configured_dir" @click="restoreDefault">
          {{ t("settings.data.location.restoreDefault") }}
        </NButton>
      </NSpace>
    </NSpace>

    <!-- 二选一确认弹窗（issue #652 / ADR-0078）：warning 级——原位置库文件保留、
         无破坏；按钮文案即语义（接管该库 / 取消换位），不靠「确定/取消」猜。
         ✕/ESC 关闭同归取消路径：清挂起二次提交并提示状态未变，不让 pendingAdopt 悬挂 -->
    <AppDangerConfirmModal
      level="warning"
      :show="adoptConfirmShow"
      :title="t('settings.data.location.adoptTitle')"
      :lead="t('settings.data.location.adoptLead')"
      :strong-warning="t('settings.data.location.adoptStrong')"
      :detail="t('settings.data.location.adoptDetail')"
      :confirm-text="t('settings.data.location.adoptOk')"
      :cancel-text="t('settings.data.location.adoptCancel')"
      :submitting="submitting"
      :on-confirm="confirmAdopt"
      :on-cancel="cancelAdopt"
      @update:show="cancelAdopt"
    />
  </NCard>
</template>
