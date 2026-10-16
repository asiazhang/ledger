<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NButton, NSpace, NText, NProgress } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import { t } from "@ledger/i18n";
import { useAppUpdate } from "@/composables/useAppUpdate";

/**
 * 应用更新提醒弹层（issue #1431 / ADR-0124 决策 4）：全询问式更新的单一弹层面，
 * 三阶段随编排状态切换——
 * - **remind**：版本号＋发布说明（兼容性警告随发布说明承载，ADR-0124 决策 5）；
 *   确认 → 下载；关闭（✕/ESC/以后再说）即取消，不触发下载。
 * - **downloading**：下载进度（后端进度事件驱动）；关闭只收起弹层，下载在后端
 *   继续，设置卡片状态行提供再入口。
 * - **downloaded**：已下载待安装——安装是全流程的显式二次确认，按分级确认
 *   warning 级形态承载（有兜底：安装确认后、旧版退出前自动创建更新前备份，
 *   ADR-0078 / ADR-0124 决策 6 / #1944）：琥珀警示块承载后果与兜底说明 +
 *   warning 色确认按钮（submitting 态消化备份+安装在途耗时），关闭即不安装。
 *
 * 弹层纪律继承 AppModal 薄封装：遮罩不关、✕/ESC 可关、开/关上报弹层注册表
 * （Overlay Suppression 快捷键抑制零新机制，ADR-0035）。流程内失败就地渲染
 * 错误位（码化错误本地化文本，不走全局 toast）。
 */
const {
  stage,
  version,
  notes,
  downloadPercent,
  modalOpen,
  flowError,
  installing,
  startDownload,
  dismissFlow,
  confirmInstall,
} = useAppUpdate();

const title = computed(() => {
  if (stage.value === "downloading") return t("settings.update.downloadingTitle");
  if (stage.value === "downloaded") return t("settings.update.downloadedTitle");
  return t("settings.update.remindTitle");
});

const versionLabel = computed(() =>
  version.value === null ? "" : t("settings.update.newVersion", { version: version.value }),
);

function onUpdateShow(value: boolean): void {
  // ✕ / ESC 的关闭意图统一走编排 dismiss：remind 阶段 = 取消（不触发下载），
  // 下载/已下载 = 收起弹层（流程继续）。
  if (!value) dismissFlow();
}
</script>

<template>
  <AppModal
    :show="modalOpen"
    preset="card"
    :title="title"
    card-size="md"
    @update:show="onUpdateShow"
  >
    <!-- remind：版本号 + 发布说明（兼容性警告随发布说明展示）+ 就地错误位 -->
    <template v-if="stage === 'remind'">
      <NSpace vertical :size="12">
        <NText data-testid="update-offer-version" strong>{{ versionLabel }}</NText>
        <NText v-if="notes" depth="3" style="white-space: pre-wrap">{{ notes }}</NText>
        <NText v-else depth="3">{{ t("settings.update.noNotes") }}</NText>
        <NText v-if="flowError" data-testid="update-flow-error" type="error">{{ flowError }}</NText>
        <NSpace justify="end" :size="8">
          <NButton @click="dismissFlow">{{ t("settings.update.later") }}</NButton>
          <NButton type="primary" data-testid="update-download-btn" @click="startDownload">
            {{ t("settings.update.download") }}
          </NButton>
        </NSpace>
      </NSpace>
    </template>

    <!-- downloading：进度可见；关闭 = 收起（下载在后端继续） -->
    <template v-else-if="stage === 'downloading'">
      <NSpace vertical :size="12">
        <NProgress
          type="line"
          :percentage="downloadPercent ?? 100"
          :show-indicator="downloadPercent !== null"
          :processing="downloadPercent === null"
        />
        <NText depth="3">{{ t("settings.update.downloadingBody") }}</NText>
        <NSpace justify="end" :size="8">
          <NButton data-testid="update-hide-btn" @click="dismissFlow">
            {{ t("settings.update.hide") }}
          </NButton>
        </NSpace>
      </NSpace>
    </template>

    <!-- downloaded：安装即二次确认（warning 级：后果 + 兜底说明 + warning 色按钮，
         submitting 态消化备份+安装在途耗时） -->
    <template v-else-if="stage === 'downloaded'">
      <NSpace vertical :size="12">
        <NText data-testid="update-downloaded-version" strong>{{ versionLabel }}</NText>
        <NAlert type="warning" :show-icon="true">
          <NText strong>{{ t("settings.update.installWarning") }}</NText>
        </NAlert>
        <NText depth="3">{{ t("settings.update.installBackupNote") }}</NText>
        <NText v-if="flowError" data-testid="update-flow-error" type="error">{{ flowError }}</NText>
        <NSpace justify="end" :size="8">
          <NButton @click="dismissFlow">{{ t("settings.update.later") }}</NButton>
          <NButton
            type="warning"
            data-testid="update-install-btn"
            :loading="installing"
            :disabled="installing"
            @click="confirmInstall"
          >
            {{ t("settings.update.installNow") }}
          </NButton>
        </NSpace>
      </NSpace>
    </template>
  </AppModal>
</template>
